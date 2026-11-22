//! Blocking OpenRGB SDK client over TCP.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::protocol::{self as p, Controller, Mode};

const READ_TIMEOUT: Duration = Duration::from_secs(1);

pub struct Client {
    stream: TcpStream,
    version: u32,
    list_updated: bool,
}

impl Client {
    pub fn connect(addr: &str, timeout: Duration) -> Result<Client, String> {
        let sock = addr
            .to_socket_addrs()
            .map_err(|e| format!("bad address {addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("no address for {addr}"))?;
        let stream = TcpStream::connect_timeout(&sock, timeout)
            .map_err(|e| format!("connect {addr}: {e}"))?;
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(READ_TIMEOUT))
            .map_err(|e| e.to_string())?;
        let mut c = Client {
            stream,
            version: 0,
            list_updated: false,
        };

        c.send(0, p::REQUEST_PROTOCOL_VERSION, &p::PROTOCOL_VERSION.to_le_bytes())?;
        let reply = c.recv()?;
        let server: [u8; 4] = reply
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .ok_or("short protocol version reply")?;
        let server = u32::from_le_bytes(server);
        if server < p::PROTOCOL_VERSION {
            return Err(format!(
                "OpenRGB server protocol {server} is too old (need {})",
                p::PROTOCOL_VERSION
            ));
        }
        c.version = p::PROTOCOL_VERSION;
        c.send(0, p::SET_CLIENT_NAME, b"Open TRCC\0")?;
        Ok(c)
    }

    pub fn controllers(&mut self) -> Result<Vec<Controller>, String> {
        self.send(0, p::REQUEST_CONTROLLER_COUNT, &[])?;
        let reply = self.recv()?;
        let count: [u8; 4] = reply
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .ok_or("short controller count reply")?;
        let count = u32::from_le_bytes(count);
        let mut out = Vec::new();
        for dev in 0..count {
            self.send(dev, p::REQUEST_CONTROLLER_DATA, &self.version.to_le_bytes())?;
            let data = self.recv()?;
            out.push(p::parse_controller(&data, self.version)?);
        }
        Ok(out)
    }

    pub fn set_mode(&mut self, dev: u32, index: i32, mode: &Mode) -> Result<(), String> {
        self.send(dev, p::UPDATEMODE, &p::update_mode_payload(index, mode))
    }

    pub fn resize_zone(&mut self, dev: u32, zone: i32, size: i32) -> Result<(), String> {
        self.send(dev, p::RESIZEZONE, &p::resize_zone_payload(zone, size))
    }

    pub fn set_all_leds(
        &mut self,
        dev: u32,
        count: usize,
        rgb: (u8, u8, u8),
    ) -> Result<(), String> {
        self.send(dev, p::UPDATELEDS, &p::update_leds_payload(count, rgb))
    }

    /// Non-blocking: whether a DEVICE_LIST_UPDATED notification has arrived
    /// since the last call.
    pub fn take_device_list_update(&mut self) -> bool {
        if self.stream.set_nonblocking(true).is_ok() {
            let mut hdr = [0u8; p::HEADER_LEN];
            // Only consume when a whole header is already buffered.
            while matches!(self.stream.peek(&mut hdr), Ok(n) if n == p::HEADER_LEN) {
                if self.stream.set_nonblocking(false).is_err() || self.recv().is_err() {
                    break;
                }
                if self.stream.set_nonblocking(true).is_err() {
                    break;
                }
            }
            self.stream.set_nonblocking(false).ok();
        }
        std::mem::take(&mut self.list_updated)
    }

    fn send(&mut self, dev: u32, id: u32, payload: &[u8]) -> Result<(), String> {
        self.stream
            .write_all(&p::encode(dev, id, payload))
            .map_err(|e| format!("send: {e}"))
    }

    /// Reads the next reply payload, skipping (and recording) device list
    /// update notifications.
    fn recv(&mut self) -> Result<Vec<u8>, String> {
        loop {
            let mut hdr = [0u8; p::HEADER_LEN];
            self.stream
                .read_exact(&mut hdr)
                .map_err(|e| format!("read: {e}"))?;
            if &hdr[..4] != b"ORGB" {
                return Err("bad packet magic".into());
            }
            let id = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
            let len = u32::from_le_bytes(hdr[12..16].try_into().unwrap()) as usize;
            if len > 1 << 20 {
                return Err(format!("packet too large: {len}"));
            }
            let mut payload = vec![0u8; len];
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("read: {e}"))?;
            if id == p::DEVICE_LIST_UPDATED {
                self.list_updated = true;
                continue;
            }
            return Ok(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    const FIXTURE: &[u8] = include_bytes!("fixtures/mystic_light_v3.bin");

    fn read_packet(s: &mut TcpStream) -> Option<(u32, u32, Vec<u8>)> {
        let mut h = [0u8; 16];
        s.read_exact(&mut h).ok()?;
        let dev = u32::from_le_bytes(h[4..8].try_into().unwrap());
        let id = u32::from_le_bytes(h[8..12].try_into().unwrap());
        let len = u32::from_le_bytes(h[12..16].try_into().unwrap()) as usize;
        let mut pl = vec![0u8; len];
        s.read_exact(&mut pl).ok()?;
        Some((dev, id, pl))
    }

    /// Fake server answering the handshake with `version`, then serving one fixture controller.
    fn fake_server(
        version: u32,
        send_update_after_connect: bool,
    ) -> (String, JoinHandle<()>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let h = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            while let Some((dev, id, _)) = read_packet(&mut s) {
                match id {
                    p::REQUEST_PROTOCOL_VERSION => {
                        s.write_all(&p::encode(0, id, &version.to_le_bytes())).unwrap()
                    }
                    p::SET_CLIENT_NAME => {
                        if send_update_after_connect {
                            s.write_all(&p::encode(0, p::DEVICE_LIST_UPDATED, &[])).unwrap();
                        }
                    }
                    p::REQUEST_CONTROLLER_COUNT => {
                        // A list update arriving mid-request must be skipped.
                        if send_update_after_connect {
                            s.write_all(&p::encode(0, p::DEVICE_LIST_UPDATED, &[])).unwrap();
                        }
                        s.write_all(&p::encode(0, id, &1u32.to_le_bytes())).unwrap()
                    }
                    p::REQUEST_CONTROLLER_DATA => {
                        s.write_all(&p::encode(dev, id, FIXTURE)).unwrap()
                    }
                    _ => {}
                }
            }
        });
        (addr, h)
    }

    #[test]
    fn handshake_and_list_controllers() {
        let (addr, _h) = fake_server(6, false);
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        let list = c.controllers().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "MSI B550M PRO-VDH (MS-7C95)");
    }

    #[test]
    fn device_list_update_is_reported() {
        let (addr, _h) = fake_server(6, true);
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        // Skipped while waiting for the controller count reply, but recorded.
        assert_eq!(c.controllers().unwrap().len(), 1);
        assert!(c.take_device_list_update());
        assert!(!c.take_device_list_update());
    }

    #[test]
    fn unsolicited_device_list_update_is_picked_up_without_blocking() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let h = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            read_packet(&mut s).unwrap();
            s.write_all(&p::encode(0, p::REQUEST_PROTOCOL_VERSION, &6u32.to_le_bytes())).unwrap();
            read_packet(&mut s).unwrap();
            s.write_all(&p::encode(0, p::DEVICE_LIST_UPDATED, &[])).unwrap();
            thread::sleep(Duration::from_millis(500));
        });
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        assert!(!c.take_device_list_update());
        thread::sleep(Duration::from_millis(200));
        assert!(c.take_device_list_update());
        assert!(!c.take_device_list_update());
        h.join().unwrap();
    }

    #[test]
    fn old_server_is_rejected() {
        let (addr, _h) = fake_server(2, false);
        assert!(Client::connect(&addr, Duration::from_secs(2)).is_err());
    }

    #[test]
    #[ignore]
    fn live_server_lists_motherboard() {
        let mut c = Client::connect("127.0.0.1:6742", Duration::from_secs(2)).unwrap();
        for (i, ctl) in c.controllers().unwrap().iter().enumerate() {
            println!(
                "{i}: {} (type {}, {} modes, {} zones, {} leds)",
                ctl.name,
                ctl.kind,
                ctl.modes.len(),
                ctl.zones.len(),
                ctl.led_count
            );
        }
    }
}

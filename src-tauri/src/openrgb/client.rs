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
    dead: bool,
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
            dead: false,
        };

        let reply = c.request(0, p::REQUEST_PROTOCOL_VERSION, &p::PROTOCOL_VERSION.to_le_bytes())?;
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
        let reply = self.request(0, p::REQUEST_CONTROLLER_COUNT, &[])?;
        let count: [u8; 4] = reply
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .ok_or("short controller count reply")?;
        let count = u32::from_le_bytes(count);
        let mut out = Vec::new();
        for dev in 0..count {
            let data = self.request(dev, p::REQUEST_CONTROLLER_DATA, &self.version.to_le_bytes())?;
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
    /// since the last call. Only whole packets already buffered are consumed.
    /// A broken connection marks the client dead, so the next call errors.
    pub fn take_device_list_update(&mut self) -> bool {
        if !self.dead && self.drain_buffered().is_err() {
            self.dead = true;
        }
        std::mem::take(&mut self.list_updated)
    }

    /// False once any error has marked the connection dead (a closed peer is
    /// noticed by `take_device_list_update`).
    pub fn is_alive(&self) -> bool {
        !self.dead
    }

    fn drain_buffered(&mut self) -> Result<(), String> {
        self.stream.set_nonblocking(true).map_err(|e| e.to_string())?;
        let r = self.drain_buffered_inner();
        let restore = self.stream.set_nonblocking(false).map_err(|e| e.to_string());
        r.and(restore)
    }

    fn drain_buffered_inner(&mut self) -> Result<(), String> {
        loop {
            let mut hdr = [0u8; p::HEADER_LEN];
            match self.stream.peek(&mut hdr) {
                Ok(0) => return Err("connection closed".into()),
                Ok(n) if n < p::HEADER_LEN => return Ok(()),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(format!("peek: {e}")),
            }
            let (id, len) = parse_header(&hdr)?;
            let mut buf = vec![0u8; p::HEADER_LEN + len];
            match self.stream.peek(&mut buf) {
                Ok(n) if n == buf.len() => {}
                Ok(0) => return Err("connection closed".into()),
                Ok(_) => return Ok(()), // partial packet: leave it for later
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(format!("peek: {e}")),
            }
            // Fully buffered, so this cannot block.
            self.stream
                .read_exact(&mut buf)
                .map_err(|e| format!("read: {e}"))?;
            if id != p::DEVICE_LIST_UPDATED {
                return Err(format!("unexpected unsolicited packet id {id}"));
            }
            self.list_updated = true;
        }
    }

    fn send(&mut self, dev: u32, id: u32, payload: &[u8]) -> Result<(), String> {
        if self.dead {
            return Err("connection is dead".into());
        }
        let r = self
            .stream
            .write_all(&p::encode(dev, id, payload))
            .map_err(|e| format!("send: {e}"));
        if r.is_err() {
            self.dead = true;
        }
        r
    }

    /// Sends a request and returns the payload of the matching reply.
    fn request(&mut self, dev: u32, id: u32, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.send(dev, id, payload)?;
        let r = self.recv(dev, id);
        if r.is_err() {
            self.dead = true;
        }
        r
    }

    /// Reads the next reply, skipping (and recording) device list update
    /// notifications. The reply must echo the request's device and id.
    fn recv(&mut self, dev: u32, want_id: u32) -> Result<Vec<u8>, String> {
        loop {
            let mut hdr = [0u8; p::HEADER_LEN];
            self.stream
                .read_exact(&mut hdr)
                .map_err(|e| format!("read: {e}"))?;
            let (id, len) = parse_header(&hdr)?;
            let mut payload = vec![0u8; len];
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("read: {e}"))?;
            if id == p::DEVICE_LIST_UPDATED {
                self.list_updated = true;
                continue;
            }
            let got_dev = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
            if id != want_id || got_dev != dev {
                return Err(format!(
                    "unexpected reply (dev {got_dev}, id {id}); wanted dev {dev}, id {want_id}"
                ));
            }
            return Ok(payload);
        }
    }
}

/// Validates a packet header, returning `(packet id, payload length)`.
fn parse_header(hdr: &[u8; p::HEADER_LEN]) -> Result<(u32, usize), String> {
    if &hdr[..4] != b"ORGB" {
        return Err("bad packet magic".into());
    }
    let id = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
    let len = u32::from_le_bytes(hdr[12..16].try_into().unwrap()) as usize;
    if len > 1 << 20 {
        return Err(format!("packet too large: {len}"));
    }
    Ok((id, len))
}

#[cfg(test)]
mod tests {
    use super::super::fake::{fake_server, read_packet, scripted_server};
    use super::*;
    use std::thread;

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
        let (addr, done, h) = scripted_server(|s| {
            s.write_all(&p::encode(0, p::DEVICE_LIST_UPDATED, &[])).unwrap();
        });
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(200));
        let t = std::time::Instant::now();
        assert!(c.take_device_list_update());
        assert!(!c.take_device_list_update());
        assert!(t.elapsed() < Duration::from_millis(50), "{:?}", t.elapsed());
        done.send(()).unwrap();
        h.join().unwrap();
    }

    #[test]
    fn partial_packet_does_not_block_or_kill_the_client() {
        let (addr, done, h) = scripted_server(|s| {
            // Header promises 4 payload bytes that never arrive in this test.
            let mut pkt = p::encode(0, p::DEVICE_LIST_UPDATED, &[0; 4]);
            pkt.truncate(p::HEADER_LEN + 1);
            s.write_all(&pkt).unwrap();
        });
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(200));
        let t = std::time::Instant::now();
        assert!(!c.take_device_list_update());
        assert!(t.elapsed() < Duration::from_millis(50), "{:?}", t.elapsed());
        assert!(!c.dead);
        done.send(()).unwrap();
        h.join().unwrap();
    }

    #[test]
    fn closed_connection_marks_client_dead() {
        let (addr, done, h) = scripted_server(|_| {});
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        done.send(()).unwrap();
        h.join().unwrap();
        thread::sleep(Duration::from_millis(100));
        c.take_device_list_update();
        assert!(c.controllers().is_err());
    }

    #[test]
    fn mismatched_reply_id_is_an_error_and_kills_the_client() {
        let (addr, done, h) = scripted_server(|s| {
            let (_, id, _) = read_packet(s).unwrap();
            assert_eq!(id, p::REQUEST_CONTROLLER_COUNT);
            s.write_all(&p::encode(0, p::REQUEST_CONTROLLER_DATA, &1u32.to_le_bytes())).unwrap();
        });
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        assert!(c.controllers().is_err());
        assert!(c.controllers().is_err());
        assert!(c.set_all_leds(0, 1, (0, 0, 0)).is_err());
        done.send(()).unwrap();
        h.join().unwrap();
    }

    #[test]
    fn framing_error_makes_later_calls_fail() {
        let (addr, done, h) = scripted_server(|s| {
            read_packet(s).unwrap();
            s.write_all(b"NOPE0123456789ab").unwrap();
        });
        let mut c = Client::connect(&addr, Duration::from_secs(2)).unwrap();
        assert!(c.controllers().is_err());
        assert!(c.controllers().is_err());
        done.send(()).unwrap();
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

//! Test-only fake OpenRGB servers shared by the client and sync tests.
//! Always bound on 127.0.0.1 (port 0 or a port the test reserved): never the
//! real server on 6742.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::protocol as p;

pub const FIXTURE: &[u8] = include_bytes!("fixtures/mystic_light_v3.bin");

pub fn read_packet(s: &mut TcpStream) -> Option<(u32, u32, Vec<u8>)> {
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
pub fn fake_server(version: u32, send_update_after_connect: bool) -> (String, JoinHandle<()>) {
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
                p::REQUEST_CONTROLLER_DATA => s.write_all(&p::encode(dev, id, FIXTURE)).unwrap(),
                _ => {}
            }
        }
    });
    (addr, h)
}

/// Server that completes the handshake, runs `after`, then holds the
/// socket open until the test signals it is done.
pub fn scripted_server(
    after: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (String, std::sync::mpsc::Sender<()>, JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    let h = thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        read_packet(&mut s).unwrap();
        s.write_all(&p::encode(0, p::REQUEST_PROTOCOL_VERSION, &6u32.to_le_bytes()))
            .unwrap();
        read_packet(&mut s).unwrap();
        after(&mut s);
        rx.recv().ok();
    });
    (addr, tx, h)
}

/// A synthetic controller, encoded in protocol 3 controller-data layout.
#[derive(Clone)]
pub struct Ctl {
    pub kind: i32,
    pub name: String,
    pub modes: Vec<String>,
    /// `(leds_min, leds_max, leds_count)` per zone.
    pub zones: Vec<(u32, u32, u32)>,
}

impl Ctl {
    pub fn new(kind: i32, name: &str, modes: &[&str], zones: &[(u32, u32, u32)]) -> Ctl {
        Ctl {
            kind,
            name: name.into(),
            modes: modes.iter().map(|m| m.to_string()).collect(),
            zones: zones.to_vec(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        fn s(out: &mut Vec<u8>, v: &str) {
            out.extend_from_slice(&((v.len() + 1) as u16).to_le_bytes());
            out.extend_from_slice(v.as_bytes());
            out.push(0);
        }
        let mut b = Vec::new();
        b.extend_from_slice(&self.kind.to_le_bytes());
        s(&mut b, &self.name);
        for _ in 0..5 {
            s(&mut b, "");
        }
        b.extend_from_slice(&(self.modes.len() as u16).to_le_bytes());
        b.extend_from_slice(&0i32.to_le_bytes()); // active mode
        for m in &self.modes {
            s(&mut b, m);
            b.extend_from_slice(&[0; 4 + 4 + 10 * 4]);
            b.extend_from_slice(&0u16.to_le_bytes()); // mode colours
        }
        b.extend_from_slice(&(self.zones.len() as u16).to_le_bytes());
        for (i, &(min, max, count)) in self.zones.iter().enumerate() {
            s(&mut b, &format!("Zone {i}"));
            b.extend_from_slice(&1i32.to_le_bytes()); // linear
            b.extend_from_slice(&min.to_le_bytes());
            b.extend_from_slice(&max.to_le_bytes());
            b.extend_from_slice(&count.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes()); // no matrix
        }
        let leds: u32 = self.zones.iter().map(|z| z.2).sum();
        b.extend_from_slice(&(leds as u16).to_le_bytes());
        for i in 0..leds {
            s(&mut b, &format!("LED {i}"));
            b.extend_from_slice(&0u32.to_le_bytes());
        }
        b.extend_from_slice(&(leds as u16).to_le_bytes());
        b.extend_from_slice(&vec![0; leds as usize * 4]);
        let mut out = ((b.len() + 4) as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&b);
        out
    }
}

#[derive(Clone)]
pub enum Dev {
    /// The captured MSI motherboard, served verbatim (RESIZEZONE is ignored).
    Fixture,
    /// A synthetic controller; RESIZEZONE updates its zone's LED count.
    Synthetic(Ctl),
}

#[derive(Default)]
struct State {
    devices: Vec<Dev>,
    packets: Vec<(u32, u32, Vec<u8>)>,
    connections: usize,
    closed: usize,
    stall_on_updateleds: bool,
    stalled: bool,
    stopping: bool,
    current: Option<TcpStream>,
}

/// A fake OpenRGB server speaking the real request/reply shapes, serving
/// connections one at a time and recording every packet it receives.
pub struct FakeServer {
    pub addr: String,
    state: Arc<(Mutex<State>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl FakeServer {
    pub fn start(devices: Vec<Dev>) -> FakeServer {
        Self::start_on("127.0.0.1:0", devices, false)
    }

    /// `stall_on_updateleds`: stop reading once the first UPDATELEDS arrives.
    pub fn start_on(addr: &str, devices: Vec<Dev>, stall_on_updateleds: bool) -> FakeServer {
        assert!(addr.starts_with("127.0.0.1:") && !addr.ends_with(":6742"));
        let l = TcpListener::bind(addr).unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let state = Arc::new((
            Mutex::new(State {
                devices,
                stall_on_updateleds,
                ..State::default()
            }),
            Condvar::new(),
        ));
        let st = state.clone();
        let handle = thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(s) = conn else { continue };
                {
                    let mut g = st.0.lock().unwrap();
                    if g.stopping {
                        return;
                    }
                    g.connections += 1;
                    g.current = s.try_clone().ok();
                    st.1.notify_all();
                }
                serve(s, &st);
                let mut g = st.0.lock().unwrap();
                g.current = None;
                g.closed += 1;
                st.1.notify_all();
                if g.stopping {
                    return;
                }
            }
        });
        FakeServer {
            addr,
            state,
            handle: Some(handle),
        }
    }

    /// Waits (condvar, bounded) until `pred` holds over the recorded packets
    /// and counters `(packets, connections, closed, stalled)`.
    pub fn wait_for(
        &self,
        timeout: Duration,
        mut pred: impl FnMut(&[(u32, u32, Vec<u8>)], usize, usize, bool) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.state.0.lock().unwrap();
        loop {
            if pred(&g.packets, g.connections, g.closed, g.stalled) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            g = self.state.1.wait_timeout(g, deadline - now).unwrap().0;
        }
    }

    /// Waits until `n` connections have been closed by the client, so every
    /// packet sent on them has been recorded.
    pub fn wait_closed(&self, n: usize, timeout: Duration) -> bool {
        self.wait_for(timeout, |_, _, closed, _| closed >= n)
    }

    pub fn packets(&self) -> Vec<(u32, u32, Vec<u8>)> {
        self.state.0.lock().unwrap().packets.clone()
    }

    pub fn connections(&self) -> usize {
        self.state.0.lock().unwrap().connections
    }

    /// Drops the current connection and the listener, freeing the port.
    pub fn stop(&mut self) {
        let Some(h) = self.handle.take() else { return };
        {
            let mut g = self.state.0.lock().unwrap();
            g.stopping = true;
            if let Some(s) = g.current.take() {
                s.shutdown(Shutdown::Both).ok();
            }
            self.state.1.notify_all();
        }
        // Unblock accept().
        TcpStream::connect(&self.addr).ok();
        h.join().unwrap();
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(mut s: TcpStream, st: &Arc<(Mutex<State>, Condvar)>) {
    while let Some((dev, id, pl)) = read_packet(&mut s) {
        let mut g = st.0.lock().unwrap();
        if g.stopping {
            return;
        }
        g.packets.push((dev, id, pl.clone()));
        st.1.notify_all();
        let reply = match id {
            p::REQUEST_PROTOCOL_VERSION => Some(p::encode(0, id, &6u32.to_le_bytes())),
            p::REQUEST_CONTROLLER_COUNT => {
                Some(p::encode(0, id, &(g.devices.len() as u32).to_le_bytes()))
            }
            p::REQUEST_CONTROLLER_DATA => {
                let data = match g.devices.get(dev as usize) {
                    Some(Dev::Fixture) => FIXTURE.to_vec(),
                    Some(Dev::Synthetic(c)) => c.encode(),
                    None => Vec::new(),
                };
                Some(p::encode(dev, id, &data))
            }
            p::RESIZEZONE => {
                let zone = i32::from_le_bytes(pl[0..4].try_into().unwrap()) as usize;
                let size = i32::from_le_bytes(pl[4..8].try_into().unwrap()) as u32;
                if let Some(Dev::Synthetic(c)) = g.devices.get_mut(dev as usize)
                    && let Some(z) = c.zones.get_mut(zone)
                {
                    z.2 = size;
                }
                None
            }
            p::UPDATELEDS if g.stall_on_updateleds => {
                g.stalled = true;
                st.1.notify_all();
                while !g.stopping {
                    g = st.1.wait(g).unwrap();
                }
                return;
            }
            _ => None,
        };
        drop(g);
        if let Some(r) = reply
            && s.write_all(&r).is_err()
        {
            return;
        }
    }
}

/// Reserves a free loopback port with nothing listening on it.
/// Only the Linux-only spawn tests need it.
#[cfg(target_os = "linux")]
pub fn free_addr() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().to_string()
}

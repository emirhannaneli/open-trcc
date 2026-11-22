//! Case lighting sync thread: owns the OpenRGB connection and, if needed, a
//! spawned `openrgb --server` child. The device loop only drops colours into
//! a shared slot and never waits on the network.
//!
//! Locking: one mutex (`Shared::slot`) guards everything the device loop and
//! the UI touch. The sync thread copies what it needs out of it and releases
//! it before any network I/O or process handling.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::client::Client;

const SERVER_ADDR: &str = "127.0.0.1:6742";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// Longest idle wait; also how often a live connection is polled for
/// device list updates and a closed peer.
const IDLE_WAIT: Duration = Duration::from_secs(1);
const SPAWN_RETRY: Duration = Duration::from_millis(500);
#[cfg(not(test))]
const SPAWN_WINDOW: Duration = Duration::from_secs(15);
/// Shortened so the give-up test stays fast.
#[cfg(test)]
const SPAWN_WINDOW: Duration = Duration::from_millis(1500);
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

const CONNECTING: &str = "Connecting…";
const NOT_FOUND: &str = "OpenRGB not found";
const NO_MOTHERBOARD: &str = "No supported motherboard lighting";
const OFF: &str = "Off";
const BLACK: (u8, u8, u8) = (0, 0, 0);

struct Slot {
    enabled: bool,
    shutdown: bool,
    /// Newest colour from the device loop.
    color: Option<(u8, u8, u8)>,
    /// Set by any change the thread must react to.
    dirty: bool,
    status: String,
    // Observed by tests: spawn count, last child pid, child not yet reaped.
    #[cfg_attr(not(test), allow(dead_code))]
    spawns: u32,
    #[cfg_attr(not(test), allow(dead_code))]
    child_pid: Option<u32>,
    #[cfg_attr(not(test), allow(dead_code))]
    child_running: bool,
}

struct Shared {
    slot: Mutex<Slot>,
    /// Wakes the sync thread.
    wake: Condvar,
    /// Signals status / child changes (used by tests to wait without sleeps).
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_status(&self, s: &str) {
        let mut g = self.lock();
        if g.status != s {
            g.status = s.to_string();
            self.changed.notify_all();
        }
    }
}

struct Inner {
    shared: Arc<Shared>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        stop(&self.shared, &self.handle);
    }
}

fn stop(shared: &Shared, handle: &Mutex<Option<JoinHandle<()>>>) {
    {
        let mut g = shared.lock();
        g.shutdown = true;
        g.dirty = true;
        shared.wake.notify_all();
    }
    let h = handle.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(h) = h {
        h.join().ok();
    }
}

/// Handle to the sync thread. Cheap to clone; the thread stops when
/// `shutdown` is called or the last clone is dropped.
#[derive(Clone)]
pub struct CaseSync {
    inner: Arc<Inner>,
}

impl CaseSync {
    pub fn start(enabled: bool) -> CaseSync {
        let spawn = (cfg!(target_os = "linux") && on_path("openrgb"))
            .then(|| vec!["openrgb".to_string(), "--server".to_string()]);
        Self::start_with(enabled, SERVER_ADDR.to_string(), spawn)
    }

    pub(crate) fn start_with(enabled: bool, addr: String, spawn: Option<Vec<String>>) -> CaseSync {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot {
                enabled,
                shutdown: false,
                color: None,
                dirty: true,
                status: CONNECTING.to_string(),
                spawns: 0,
                child_pid: None,
                child_running: false,
            }),
            wake: Condvar::new(),
            changed: Condvar::new(),
        });
        let worker = Worker::new(shared.clone(), addr, spawn);
        let handle = thread::Builder::new()
            .name("openrgb-sync".into())
            .spawn(move || worker.run())
            .expect("spawn openrgb sync thread");
        CaseSync {
            inner: Arc::new(Inner {
                shared,
                handle: Mutex::new(Some(handle)),
            }),
        }
    }

    pub fn set_enabled(&self, on: bool) {
        let sh = &self.inner.shared;
        let mut g = sh.lock();
        if g.enabled != on {
            g.enabled = on;
            if on {
                g.status = CONNECTING.to_string();
            }
            g.dirty = true;
            sh.wake.notify_all();
            sh.changed.notify_all();
        }
    }

    /// Stores the newest colour; only takes the slot lock, never waits on I/O.
    pub fn push_color(&self, rgb: (u8, u8, u8)) {
        let sh = &self.inner.shared;
        let mut g = sh.lock();
        if g.color != Some(rgb) {
            g.color = Some(rgb);
            g.dirty = true;
            sh.wake.notify_one();
        }
    }

    pub fn status(&self) -> String {
        let g = self.inner.shared.lock();
        if g.enabled { g.status.clone() } else { OFF.to_string() }
    }

    /// Sends black once if connected, stops a child we spawned and joins the
    /// thread. Idempotent.
    pub fn shutdown(&self) {
        stop(&self.inner.shared, &self.inner.handle);
    }

    /// Waits until `pred(status)` holds or `timeout` passes; returns the
    /// last status seen.
    #[cfg(test)]
    fn wait_status(&self, timeout: Duration, pred: impl Fn(&str) -> bool) -> String {
        let sh = &self.inner.shared;
        let deadline = Instant::now() + timeout;
        let mut g = sh.lock();
        loop {
            let st = if g.enabled { g.status.clone() } else { OFF.to_string() };
            let now = Instant::now();
            if pred(&st) || now >= deadline {
                return st;
            }
            g = sh.changed.wait_timeout(g, deadline - now).unwrap().0;
        }
    }

    /// `(times spawned, last child pid, child still unreaped)`.
    #[cfg(test)]
    fn child_info(&self) -> (u32, Option<u32>, bool) {
        let g = self.inner.shared.lock();
        (g.spawns, g.child_pid, g.child_running)
    }
}

fn on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|d| d.join(exe).is_file()))
        .unwrap_or(false)
}

/// A motherboard controller we drive: device index, LED count, name.
struct Target {
    dev: u32,
    leds: usize,
    name: String,
}

/// State owned by the sync thread only.
struct Worker {
    shared: Arc<Shared>,
    addr: String,
    spawn: Option<Vec<String>>,
    client: Option<Client>,
    targets: Vec<Target>,
    last_sent: Option<(u8, u8, u8)>,
    child: Option<Child>,
    /// Whether this enable episode already spawned (we spawn at most once).
    spawned: bool,
    /// Whether this enable episode ever connected (a later drop reconnects
    /// to whatever comes back instead of spawning our own server).
    connected_once: bool,
    spawn_deadline: Option<Instant>,
    next_attempt: Instant,
    backoff: Duration,
}

impl Worker {
    fn new(shared: Arc<Shared>, addr: String, spawn: Option<Vec<String>>) -> Worker {
        Worker {
            shared,
            addr,
            spawn: spawn.filter(|c| !c.is_empty()),
            client: None,
            targets: Vec::new(),
            last_sent: None,
            child: None,
            spawned: false,
            connected_once: false,
            spawn_deadline: None,
            next_attempt: Instant::now(),
            backoff: BACKOFF_MIN,
        }
    }

    fn run(mut self) {
        loop {
            let (enabled, shutdown, color) = self.wait();
            if shutdown {
                self.disconnect();
                self.stop_child();
                return;
            }
            if !enabled {
                self.disconnect();
                self.stop_child();
                self.reset_episode();
                continue;
            }
            if self.client.is_none() {
                if Instant::now() >= self.next_attempt {
                    self.attempt();
                }
                if self.client.is_none() {
                    continue;
                }
            }
            self.service(color);
        }
    }

    /// Waits for a change or the next deadline, then snapshots the slot.
    fn wait(&self) -> (bool, bool, Option<(u8, u8, u8)>) {
        let mut g = self.shared.lock();
        if !g.dirty && !g.shutdown {
            let timeout = if g.enabled && self.client.is_none() {
                self.next_attempt
                    .saturating_duration_since(Instant::now())
                    .min(IDLE_WAIT)
            } else {
                IDLE_WAIT
            };
            g = self
                .shared
                .wake
                .wait_timeout(g, timeout)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        g.dirty = false;
        (g.enabled, g.shutdown, g.color)
    }

    fn reset_episode(&mut self) {
        self.spawned = false;
        self.connected_once = false;
        self.spawn_deadline = None;
        self.next_attempt = Instant::now();
        self.backoff = BACKOFF_MIN;
    }

    /// One connection attempt, spawning our server once if nothing listens.
    fn attempt(&mut self) {
        match Client::connect(&self.addr, CONNECT_TIMEOUT) {
            Ok(mut c) => match self.setup(&mut c) {
                Ok(()) => {
                    self.client = Some(c);
                    self.connected_once = true;
                    self.spawn_deadline = None;
                    self.backoff = BACKOFF_MIN;
                    self.last_sent = None;
                    self.publish_connected();
                }
                Err(_) => self.fail(CONNECTING),
            },
            Err(_) => self.connect_failed(),
        }
    }

    fn connect_failed(&mut self) {
        let now = Instant::now();
        self.reap_exited_child();
        if let Some(deadline) = self.spawn_deadline {
            if now < deadline {
                self.next_attempt = now + SPAWN_RETRY;
            } else {
                self.spawn_deadline = None;
                self.stop_child();
                self.fail(NOT_FOUND);
            }
            return;
        }
        if !self.spawned && !self.connected_once && self.spawn.is_some() && !self.listening() {
            self.spawned = true;
            if self.start_child() {
                self.spawn_deadline = Some(now + SPAWN_WINDOW);
                self.next_attempt = now + SPAWN_RETRY;
                self.shared.set_status(CONNECTING);
                return;
            }
        }
        self.fail(if self.connected_once { CONNECTING } else { NOT_FOUND });
    }

    /// Anything accepting TCP on the address (e.g. an OpenRGB we can't use).
    fn listening(&self) -> bool {
        let addrs: Vec<SocketAddr> = self.addr.to_socket_addrs().map(|a| a.collect()).unwrap_or_default();
        addrs
            .first()
            .is_some_and(|a| TcpStream::connect_timeout(a, CONNECT_TIMEOUT).is_ok())
    }

    /// Schedules the next attempt with exponential backoff.
    fn fail(&mut self, status: &str) {
        self.shared.set_status(status);
        self.next_attempt = Instant::now() + self.backoff;
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
    }

    /// Loads controllers, switches each motherboard with a `Direct` mode to
    /// it, sizes empty resizable zones and records the resulting LED counts.
    fn setup(&mut self, c: &mut Client) -> Result<(), String> {
        let ctls = c.controllers()?;
        let mut keep = Vec::new();
        let mut resized = false;
        for (i, ctl) in ctls.iter().enumerate() {
            let Some(mi) = direct_mode(ctl) else { continue };
            let dev = i as u32;
            c.set_mode(dev, mi as i32, &ctl.modes[mi])?;
            for (zi, z) in ctl.zones.iter().enumerate() {
                if z.leds_min != z.leds_max && z.leds_count == 0 {
                    c.resize_zone(dev, zi as i32, z.leds_max as i32)?;
                    resized = true;
                }
            }
            keep.push(i);
        }
        let ctls = if resized { c.controllers()? } else { ctls };
        self.targets = keep
            .into_iter()
            .filter_map(|i| {
                let ctl = ctls.get(i)?;
                direct_mode(ctl)?;
                Some(Target {
                    dev: i as u32,
                    leds: ctl.led_count,
                    name: ctl.name.clone(),
                })
            })
            .collect();
        Ok(())
    }

    fn publish_connected(&self) {
        if self.targets.is_empty() {
            self.shared.set_status(NO_MOTHERBOARD);
        } else {
            let names: Vec<&str> = self.targets.iter().map(|t| t.name.as_str()).collect();
            self.shared.set_status(&format!("Connected: {}", names.join(", ")));
        }
    }

    /// Handles device list updates and sends the colour if it changed.
    fn service(&mut self, color: Option<(u8, u8, u8)>) {
        let Some(mut c) = self.client.take() else { return };
        if c.take_device_list_update() {
            if self.setup(&mut c).is_err() {
                return self.fail(CONNECTING);
            }
            self.last_sent = None;
            self.publish_connected();
        }
        if !c.is_alive() {
            return self.fail(CONNECTING);
        }
        if let Some(rgb) = color
            && self.last_sent != Some(rgb)
        {
            if self.send_all(&mut c, rgb).is_err() {
                return self.fail(CONNECTING);
            }
            self.last_sent = Some(rgb);
        }
        self.client = Some(c);
    }

    fn send_all(&self, c: &mut Client, rgb: (u8, u8, u8)) -> Result<(), String> {
        for t in &self.targets {
            c.set_all_leds(t.dev, t.leds, rgb)?;
        }
        Ok(())
    }

    /// Drops the connection, sending one black frame first if it is alive.
    fn disconnect(&mut self) {
        if let Some(mut c) = self.client.take()
            && c.is_alive()
        {
            self.send_all(&mut c, BLACK).ok();
        }
        self.targets.clear();
        self.last_sent = None;
    }

    fn start_child(&mut self) -> bool {
        let Some(cmd) = &self.spawn else { return false };
        let child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(child) = child else { return false };
        {
            let mut g = self.shared.lock();
            g.spawns += 1;
            g.child_pid = Some(child.id());
            g.child_running = true;
            self.shared.changed.notify_all();
        }
        self.child = Some(child);
        true
    }

    /// Reaps our child if it already exited on its own.
    fn reap_exited_child(&mut self) {
        if let Some(ch) = &mut self.child
            && !matches!(ch.try_wait(), Ok(None))
        {
            self.child = None;
            self.mark_child_gone();
        }
    }

    /// Kills and reaps the child we spawned (never any other process).
    fn stop_child(&mut self) {
        if let Some(mut ch) = self.child.take() {
            ch.kill().ok();
            ch.wait().ok();
            self.mark_child_gone();
        }
    }

    fn mark_child_gone(&self) {
        let mut g = self.shared.lock();
        g.child_running = false;
        self.shared.changed.notify_all();
    }
}

fn direct_mode(ctl: &super::protocol::Controller) -> Option<usize> {
    if ctl.kind != 0 {
        return None;
    }
    ctl.modes.iter().position(|m| m.name == "Direct")
}

#[cfg(test)]
mod tests {
    use super::super::fake::{Ctl, Dev, FakeServer, free_addr};
    use super::super::protocol as p;
    use super::*;
    use std::time::Instant;

    const T: Duration = Duration::from_secs(5);
    const MB: &str = "MSI B550M PRO-VDH (MS-7C95)";

    fn leds(pkts: &[(u32, u32, Vec<u8>)]) -> Vec<(u32, Vec<u8>)> {
        pkts.iter()
            .filter(|(_, id, _)| *id == p::UPDATELEDS)
            .map(|(d, _, pl)| (*d, pl.clone()))
            .collect()
    }

    /// Colours of an UPDATELEDS payload: `(count, every LED equals rgb)`.
    fn colour_of(pl: &[u8]) -> (usize, Option<(u8, u8, u8)>) {
        let n = u16::from_le_bytes(pl[4..6].try_into().unwrap()) as usize;
        let c = &pl[6..];
        assert_eq!(c.len(), n * 4);
        let first = c.get(..4).map(|b| (b[0], b[1], b[2]));
        let same = c.chunks(4).all(|b| Some((b[0], b[1], b[2])) == first && b[3] == 0);
        (n, if same { first } else { None })
    }

    fn proc_alive(pid: u32) -> bool {
        // A reaped child has no /proc entry; a zombie would still show one.
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }

    #[test]
    fn pushes_colour_to_motherboard_only() {
        let kb = Ctl::new(5, "Keyboard", &["Direct"], &[(10, 10, 10)]);
        let srv = FakeServer::start(vec![Dev::Fixture, Dev::Synthetic(kb)]);
        let s = CaseSync::start_with(true, srv.addr.clone(), None);
        assert_eq!(s.wait_status(T, |st| st.starts_with("Connected")), format!("Connected: {MB}"));
        s.push_color((10, 20, 30));
        assert!(srv.wait_for(T, |pk, _, _, _| !leds(pk).is_empty()));
        s.shutdown();
        assert!(srv.wait_closed(1, T));
        let pk = srv.packets();
        let l = leds(&pk);
        assert!(l.iter().all(|(d, _)| *d == 0), "{:?}", l.iter().map(|x| x.0).collect::<Vec<_>>());
        let (n, c) = colour_of(&l[0].1);
        assert!(n > 0);
        assert_eq!(c, Some((10, 20, 30)));
        // UPDATEMODE only for the motherboard, with its Direct mode.
        let modes: Vec<_> = pk.iter().filter(|x| x.1 == p::UPDATEMODE).collect();
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].0, 0);
        let ctl = p::parse_controller(super::super::fake::FIXTURE, 3).unwrap();
        let direct = ctl.modes.iter().position(|m| m.name == "Direct").unwrap();
        assert_eq!(modes[0].2, p::update_mode_payload(direct as i32, &ctl.modes[direct]));
    }

    #[test]
    fn skips_motherboard_without_direct() {
        let mb = Ctl::new(0, "Board", &["Static", "Rainbow"], &[(0, 120, 0)]);
        let srv = FakeServer::start(vec![Dev::Synthetic(mb)]);
        let s = CaseSync::start_with(true, srv.addr.clone(), None);
        assert_eq!(
            s.wait_status(T, |st| st != "Connecting…"),
            "No supported motherboard lighting"
        );
        s.push_color((1, 2, 3));
        s.shutdown();
        assert!(srv.wait_closed(1, T));
        let pk = srv.packets();
        assert!(
            pk.iter()
                .all(|x| ![p::UPDATEMODE, p::UPDATELEDS, p::RESIZEZONE].contains(&x.1)),
            "{:?}",
            pk.iter().map(|x| x.1).collect::<Vec<_>>()
        );
    }

    #[test]
    fn resizes_empty_resizable_zones() {
        let mb = Ctl::new(
            0,
            "Board",
            &["Static", "Direct"],
            &[(0, 120, 0), (1, 1, 1), (0, 60, 8)],
        );
        let srv = FakeServer::start(vec![Dev::Synthetic(mb)]);
        let s = CaseSync::start_with(true, srv.addr.clone(), None);
        assert_eq!(s.wait_status(T, |st| st.starts_with("Connected")), "Connected: Board");
        s.push_color((9, 8, 7));
        assert!(srv.wait_for(T, |pk, _, _, _| !leds(pk).is_empty()));
        s.shutdown();
        assert!(srv.wait_closed(1, T));
        let pk = srv.packets();
        let resizes: Vec<_> = pk.iter().filter(|x| x.1 == p::RESIZEZONE).collect();
        assert_eq!(resizes.len(), 1);
        assert_eq!(resizes[0].2, p::resize_zone_payload(0, 120));
        // Reloaded after the resize: 120 + 1 + 8 LEDs.
        assert_eq!(colour_of(&leds(&pk)[0].1), (129, Some((9, 8, 7))));
    }

    #[test]
    fn reconnects_after_server_drops() {
        let mut a = FakeServer::start(vec![Dev::Fixture]);
        let addr = a.addr.clone();
        let s = CaseSync::start_with(true, addr.clone(), None);
        s.wait_status(T, |st| st.starts_with("Connected"));
        a.stop();
        assert_eq!(s.wait_status(T, |st| st != format!("Connected: {MB}")), "Connecting…");
        let b = FakeServer::start_on(&addr, vec![Dev::Fixture], false);
        assert_eq!(
            s.wait_status(Duration::from_secs(10), |st| st.starts_with("Connected")),
            format!("Connected: {MB}")
        );
        assert_eq!(b.connections(), 1);
        s.shutdown();
    }

    #[test]
    fn uses_existing_server_without_spawning() {
        let srv = FakeServer::start(vec![Dev::Fixture]);
        let s = CaseSync::start_with(true, srv.addr.clone(), Some(vec!["false".into()]));
        assert_eq!(s.wait_status(T, |st| st.starts_with("Connected")), format!("Connected: {MB}"));
        s.shutdown();
        assert_eq!(s.child_info().0, 0);
    }

    #[test]
    fn gives_up_after_spawn_window() {
        let addr = free_addr();
        let s = CaseSync::start_with(true, addr, Some(vec!["sleep".into(), "60".into()]));
        let t = Instant::now();
        assert_eq!(s.wait_status(T, |st| st == "OpenRGB not found"), "OpenRGB not found");
        assert!(t.elapsed() >= SPAWN_WINDOW, "{:?}", t.elapsed());
        let (spawns, pid, running) = s.child_info();
        assert_eq!(spawns, 1);
        assert!(!running);
        assert!(!proc_alive(pid.unwrap()));
        s.shutdown();
    }

    #[test]
    fn shutdown_sends_black_and_stops_child() {
        let addr = free_addr();
        let s = CaseSync::start_with(true, addr.clone(), Some(vec!["sleep".into(), "60".into()]));
        // Child spawned because nothing listened; the "server" comes up later.
        let deadline = Instant::now() + T;
        while s.child_info().0 == 0 {
            assert!(Instant::now() < deadline);
            s.wait_status(Duration::from_millis(50), |_| false);
        }
        let srv = FakeServer::start_on(&addr, vec![Dev::Fixture], false);
        s.wait_status(T, |st| st.starts_with("Connected"));
        s.push_color((200, 100, 50));
        assert!(srv.wait_for(T, |pk, _, _, _| !leds(pk).is_empty()));
        let pid = s.child_info().1.unwrap();
        assert!(proc_alive(pid));
        let t = Instant::now();
        s.shutdown();
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        assert!(srv.wait_closed(1, T));
        let l = leds(&srv.packets());
        assert_eq!(colour_of(&l.last().unwrap().1).1, Some((0, 0, 0)));
        assert_eq!(colour_of(&l[l.len() - 2].1).1, Some((200, 100, 50)));
        assert!(!s.child_info().2);
        assert!(!proc_alive(pid));
    }

    #[test]
    fn disabling_sends_black_and_reports_off() {
        let srv = FakeServer::start(vec![Dev::Fixture]);
        let s = CaseSync::start_with(true, srv.addr.clone(), None);
        s.wait_status(T, |st| st.starts_with("Connected"));
        s.push_color((5, 5, 5));
        assert!(srv.wait_for(T, |pk, _, _, _| !leds(pk).is_empty()));
        s.set_enabled(false);
        assert_eq!(s.status(), "Off");
        assert!(srv.wait_closed(1, T));
        let l = leds(&srv.packets());
        assert_eq!(colour_of(&l.last().unwrap().1).1, Some((0, 0, 0)));
        s.set_enabled(true);
        assert_eq!(s.wait_status(T, |st| st.starts_with("Connected")), format!("Connected: {MB}"));
        s.shutdown();
    }

    #[test]
    fn push_color_never_blocks() {
        let srv = FakeServer::start_on("127.0.0.1:0", vec![Dev::Fixture], true);
        let s = CaseSync::start_with(true, srv.addr.clone(), None);
        s.wait_status(T, |st| st.starts_with("Connected"));
        s.push_color((1, 1, 1));
        assert!(srv.wait_for(T, |_, _, _, stalled| stalled));
        let t = Instant::now();
        for i in 0..1000u32 {
            s.push_color(((i % 256) as u8, (i / 256) as u8, 7));
        }
        assert!(t.elapsed() < Duration::from_millis(50), "{:?}", t.elapsed());
        s.shutdown();
    }
}

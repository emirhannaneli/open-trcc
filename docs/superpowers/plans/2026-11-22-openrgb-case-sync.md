# Case Lighting Sync Through OpenRGB Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Motherboard ARGB/RGB zones follow the cooler display's colour every frame, through an OpenRGB server Open TRCC finds or starts itself.

**Architecture:** A small blocking OpenRGB SDK client (our own MIT code) in `src-tauri/src/openrgb/`. A dedicated sync thread owns the connection and the optional spawned `openrgb --server` child; the 50 ms device loop only drops the newest colour into a shared slot and never waits. `PacketBuilder` gains `sync_color(frame)` so the case colour comes from the same effect logic as the display.

**Tech Stack:** Rust std (`TcpStream`, `std::process::Command`, `Mutex`/`Condvar`), Tauri 2, React, bash.

**Spec:** `docs/superpowers/specs/2026-11-22-openrgb-sync-design.md`

## Global Constraints

- OpenRGB server address `127.0.0.1:6742`. Spawn command: `openrgb --server` (Linux only, only if nothing listens and `openrgb` is on PATH).
- Protocol: request version **3** (OpenRGB 0.9); use `min(server, 3)`; a server below 3 is treated as "OpenRGB not found".
- Only controllers with type `0` (Motherboard) **and** a mode named exactly `Direct`. Never use SETCUSTOMMODE. Never write a controller without `Direct`.
- Resize a zone only if `leds_min != leds_max` and `leds_count == 0`; new size = `leds_max`.
- Sync colour: display colour after brightness, **without** `TRCC_OUTPUT_SCALE`; send only when it changed.
- Setting `case_sync: bool`, persisted, default **true**. Old settings files without the field load as `true`.
- Status strings, exact: `Connected: <names joined by ", ">`, `Connecting…`, `OpenRGB not found`, `No supported motherboard lighting`, `Off`.
- Reconnect backoff 2 s doubling, capped at 30 s. Spawned server gets up to 15 s to start listening.
- The display loop must never block on OpenRGB.
- Never kill an OpenRGB process we didn't spawn.

## Review Focus

1. OpenRGB killed or restarted mid-session → status goes to `Connecting…`, reconnects by itself, display frames keep their 50 ms pace. (Task 3 test `reconnects_after_server_drops`.)
2. User already runs OpenRGB (GUI with server, or `openrgb --server`) → used as is, never spawned twice, never killed on quit. (Task 3 test `uses_existing_server_without_spawning`.)
3. OpenRGB takes seconds to detect devices after spawn → keeps retrying up to 15 s, then `OpenRGB not found` with backoff. (Task 3 test `gives_up_after_spawn_window`.)
4. Motherboard hot-plug / OpenRGB rescans → server sends DEVICE_LIST_UPDATED (packet 100); client reloads controllers. (Task 2 test `device_list_update_is_reported`.)
5. Quit while connected → one black frame sent, spawned child killed, no orphan `openrgb`. (Task 3 test `shutdown_sends_black_and_stops_child`.)

---

### Task 1: Sync colour from the effect engine

**Files:**
- Modify: `src-tauri/src/thermalright/protocol.rs` (`build_display_packet` colour logic, tests)

**Interfaces:**
- Produces: `impl PacketBuilder { pub fn sync_color(&self, cpu_temp: f32, cpu_usage: f32, gpu_temp: f32, gpu_usage: f32, frame: u64) -> (u8, u8, u8) }`

Move the per-mode colour computation into one private helper used by both `build_display_packet` and `sync_color` so they can't drift. `sync_color` applies `self.brightness` but not `TRCC_OUTPUT_SCALE`; Rainbow uses the hue with offset of group `LED_GROUPS / 2`; Off or brightness ≈ 0 → `(0,0,0)`; `test_mode` → `self.color` at brightness.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn sync_color_static_is_full_scale() {
    let b = PacketBuilder { mode: LedMode::Static, color: (200, 100, 0), ..Default::default() };
    assert_eq!(b.sync_color(50.0, 0.0, 0.0, 0.0, 0), (200, 100, 0));
}
#[test]
fn sync_color_off_is_black() {
    let b = PacketBuilder { mode: LedMode::Off, ..Default::default() };
    assert_eq!(b.sync_color(50.0, 0.0, 0.0, 0.0, 7), (0, 0, 0));
}
#[test]
fn sync_color_matches_display_led_hue() {
    // display LED colour == sync colour * 0.4 for Static and Breathing
    for mode in [LedMode::Static, LedMode::Breathing] {
        let b = PacketBuilder { mode, color: (250, 0, 0), ..Default::default() };
        let p = b.build_display_packet(88.0, 0.0, 0.0, 0.0, 30);
        assert_eq!(p[20], (b.sync_color(88.0, 0.0, 0.0, 0.0, 30).0 as f32 * 0.4) as u8);
    }
}
```

- [ ] **Step 2:** `cd src-tauri && cargo test sync_color` → FAIL (no method `sync_color`).
- [ ] **Step 3:** Implement as described above.
- [ ] **Step 4:** `cargo test` → all pass (existing 8 + 3 new).
- [ ] **Step 5:** Commit `feat(led): expose the frame's colour for case sync`.

---

### Task 2: OpenRGB SDK protocol and client

**Files:**
- Create: `src-tauri/src/openrgb/mod.rs`, `src-tauri/src/openrgb/protocol.rs`, `src-tauri/src/openrgb/client.rs`
- Create: `src-tauri/src/openrgb/fixtures/mystic_light_v3.bin` (captured, anonymised)
- Modify: `src-tauri/src/lib.rs` (`mod openrgb;`)

**Interfaces:**
- Produces (protocol.rs, pure):
  - `pub const PROTOCOL_VERSION: u32 = 3;`
  - packet ids: `REQUEST_CONTROLLER_COUNT = 0`, `REQUEST_CONTROLLER_DATA = 1`, `REQUEST_PROTOCOL_VERSION = 40`, `SET_CLIENT_NAME = 50`, `DEVICE_LIST_UPDATED = 100`, `RESIZEZONE = 1000`, `UPDATELEDS = 1050`, `UPDATEMODE = 1101`
  - `pub fn encode(dev: u32, id: u32, payload: &[u8]) -> Vec<u8>` — header `b"ORGB"`, then `dev`, `id`, `payload.len()` as u32 LE.
  - `pub struct Controller { pub kind: i32, pub name: String, pub modes: Vec<Mode>, pub zones: Vec<Zone>, pub led_count: usize }`
  - `pub struct Mode { pub name: String, pub raw: Vec<u8> }` (`raw` = the mode's exact serialized bytes, re-sent by UPDATEMODE)
  - `pub struct Zone { pub leds_min: u32, pub leds_max: u32, pub leds_count: u32 }`
  - `pub fn parse_controller(data: &[u8], version: u32) -> Result<Controller, String>`
  - `pub fn update_mode_payload(mode_index: i32, mode: &Mode) -> Vec<u8>` — `u32 size, i32 index, raw`.
  - `pub fn resize_zone_payload(zone: i32, size: i32) -> Vec<u8>`
  - `pub fn update_leds_payload(count: usize, rgb: (u8, u8, u8)) -> Vec<u8>` — `u32 size, u16 count, count × [r,g,b,0]`.
- Produces (client.rs, blocking):
  - `pub struct Client` with `pub fn connect(addr: &str, timeout: Duration) -> Result<Client, String>` (handshake: version, client name `"Open TRCC"`)
  - `pub fn controllers(&mut self) -> Result<Vec<Controller>, String>`
  - `pub fn set_mode(&mut self, dev: u32, index: i32, mode: &Mode) -> Result<(), String>`
  - `pub fn resize_zone(&mut self, dev: u32, zone: i32, size: i32) -> Result<(), String>`
  - `pub fn set_all_leds(&mut self, dev: u32, count: usize, rgb: (u8, u8, u8)) -> Result<(), String>`
  - `pub fn take_device_list_update(&mut self) -> bool` — non-blocking check whether a DEVICE_LIST_UPDATED arrived.

Controller data layout at v3 (all LE; strings = `u16 len` incl. NUL + bytes): `u32 size, i32 type, name, vendor, description, version, serial, location, u16 num_modes, i32 active_mode`, modes × (`name, i32 value, u32 flags, speed_min, speed_max, brightness_min, brightness_max, colors_min, colors_max, speed, brightness, direction, color_mode, u16 num_colors, colors × u32`), `u16 num_zones`, zones × (`name, i32 type, u32 leds_min, leds_max, leds_count, u16 matrix_len, matrix_len bytes`), `u16 num_leds`, leds × (`name, u32 value`), `u16 num_colors`, colors. Bounds-check every read; a short buffer is an `Err`, never a panic.

- [ ] **Step 1: Capture the fixture.** With `openrgb --server` running, request controller 0 at version 3 (`REQUEST_CONTROLLER_DATA` payload = `3u32`) and save the reply payload. Replace the serial and location strings with same-length `X` runs. Commit as `fixtures/mystic_light_v3.bin`.
- [ ] **Step 2: Write failing tests** in `protocol.rs`:

```rust
#[test]
fn parses_captured_mystic_light() {
    let c = parse_controller(include_bytes!("fixtures/mystic_light_v3.bin"), 3).unwrap();
    assert_eq!(c.kind, 0);
    assert_eq!(c.name, "MSI B550M PRO-VDH (MS-7C95)");
    assert!(c.modes.iter().any(|m| m.name == "Direct"));
    let zones: Vec<_> = c.zones.iter().map(|z| z.leds_max).collect();
    assert_eq!(zones.len(), 5); // JRGB1, JRGB2, JRAINBOW1, JRAINBOW2, ONBOARD
}
#[test]
fn truncated_controller_is_an_error() {
    let d = include_bytes!("fixtures/mystic_light_v3.bin");
    for cut in [0, 10, d.len() / 2, d.len() - 1] {
        assert!(parse_controller(&d[..cut], 3).is_err());
    }
}
#[test]
fn update_leds_payload_layout() {
    assert_eq!(update_leds_payload(2, (1, 2, 3)), vec![14,0,0,0, 2,0, 1,2,3,0, 1,2,3,0]);
}
#[test]
fn header_layout() {
    assert_eq!(encode(5, 1050, &[9]), b"ORGB\x05\0\0\0\x1a\x04\0\0\x01\0\0\0\x09".to_vec());
}
```

  And in `client.rs`, against a fake server (`TcpListener` on `127.0.0.1:0` in a thread that answers the handshake and serves the fixture):

```rust
#[test] fn handshake_and_list_controllers() { /* controllers().len() == 1, name matches */ }
#[test] fn device_list_update_is_reported() { /* fake server sends id 100; take_device_list_update() == true once, then false */ }
#[test] fn old_server_is_rejected() { /* server answers version 2 → connect() is Err */ }
```

- [ ] **Step 3:** `cargo test openrgb` → FAIL (module missing).
- [ ] **Step 4:** Implement protocol.rs and client.rs. Client reads with a 1 s timeout; any packet with id 100 received while waiting for a reply sets the update flag and is skipped.
- [ ] **Step 5:** `cargo test` → all pass. Also run against the real server once: a throwaway `#[ignore]` test `live_server_lists_motherboard` that connects to `127.0.0.1:6742` and prints controllers; `cargo test live_server -- --ignored --nocapture` shows the MSI board.
- [ ] **Step 6:** Commit `feat(openrgb): minimal SDK client`.

---

### Task 3: Sync thread and server lifecycle

**Files:**
- Create: `src-tauri/src/openrgb/sync.rs`

**Interfaces:**
- Consumes: Task 2 `Client`, `Controller`, `Mode`.
- Produces:
  - `pub struct CaseSync` (cheap to clone, `Arc` inside)
  - `pub fn CaseSync::start(enabled: bool) -> CaseSync` — spawns the thread.
  - `pub fn set_enabled(&self, on: bool)`
  - `pub fn push_color(&self, rgb: (u8, u8, u8))` — stores the newest colour, never blocks beyond a slot lock.
  - `pub fn status(&self) -> String` — one of the Global Constraints strings.
  - `pub fn shutdown(&self)` — sends black once if connected, kills the spawned child, joins the thread (≤ 2 s).
  - For tests: `pub(crate) fn start_with(enabled: bool, addr: String, spawn: Option<Vec<String>>) -> CaseSync` (`spawn` = command line or `None` to never spawn).

Thread loop: wait on a `Condvar` for a new colour/enabled change (timeout 1 s). When enabled and disconnected → connect; on refusal spawn the child once (if `spawn` is set and we haven't), retry every 500 ms up to 15 s, then backoff 2→30 s. On connect: load controllers, keep kind 0 with `Direct`, `set_mode` to it, resize zones per Global Constraints, reload controllers to get the new LED counts. Each new colour → `set_all_leds` on every kept controller. I/O error → drop client, status `Connecting…`, backoff. `take_device_list_update()` → redo the connect setup. Disabled → send black once, disconnect, stop our child.

- [ ] **Step 1: Write failing tests** with the fake server from Task 2 (move it to a shared `#[cfg(test)] mod fake;` in `openrgb/`):

```rust
#[test] fn pushes_colour_to_motherboard_only() { /* fixture motherboard + a keyboard (kind 5): UPDATELEDS only for dev 0, colour bytes match */ }
#[test] fn skips_motherboard_without_direct() { /* fixture with "Direct" renamed: no UPDATEMODE/UPDATELEDS, status == "No supported motherboard lighting" */ }
#[test] fn resizes_empty_resizable_zones() { /* zone with leds_count 0, min 0, max 120 → RESIZEZONE(zone, 120) */ }
#[test] fn reconnects_after_server_drops() { /* fake server closes; status "Connecting…"; second fake on same port → "Connected: …" within backoff */ }
#[test] fn uses_existing_server_without_spawning() { /* spawn = Some(["false"]); server already listening → child never started */ }
#[test] fn gives_up_after_spawn_window() { /* no server, spawn = Some(["sleep","60"]) → status "OpenRGB not found" after 15 s; child killed. Use a shortened window via a test-only constant. */ }
#[test] fn shutdown_sends_black_and_stops_child() { /* last UPDATELEDS before close is [0,0,0,0]; spawned child has exited */ }
#[test] fn push_color_never_blocks() { /* server that never reads: 1000 push_color calls finish < 50 ms */ }
```

- [ ] **Step 2:** `cargo test openrgb::sync` → FAIL.
- [ ] **Step 3:** Implement. Spawn with stdout/stderr to null and kill + wait the child on shutdown/disable/give-up.
- [ ] **Step 4:** `cargo test` → all pass.
- [ ] **Step 5:** Commit `feat(openrgb): case sync thread with server lifecycle`.

---

### Task 4: Wire into the app and UI

**Files:**
- Modify: `src-tauri/src/lib.rs` (`DeviceStatus`, `Settings`, `AppState`, device loop, new command, exit handler)
- Modify: `src/App.tsx` (toggle row after "Start on login", status line)

**Interfaces:**
- Consumes: Task 1 `sync_color`, Task 3 `CaseSync`.
- Produces: `DeviceStatus.case_sync: bool`, `DeviceStatus.case_sync_status: String`; command `set_case_sync(enabled: bool) -> Result<DeviceStatus, String>`.

Details: `Settings.case_sync` with `#[serde(default = "default_true")]`. `AppState.case_sync: CaseSync` started with the loaded setting. Device loop: every frame, outside the `connected && supports_led_layout()` branch, `push_color(builder.sync_color(...))` when `status.case_sync`. Every stats frame, `status.case_sync_status = case_sync.status()` (or `"Off"` when disabled). `RunEvent::Exit` → `case_sync.shutdown()` after `blank_display`. UI: toggle labelled "Case lighting sync", button text `On`/`Off`, below it a muted line with `case_sync_status`.

- [ ] **Step 1: Write failing test** in lib.rs:

```rust
#[test]
fn settings_without_case_sync_default_to_on() {
    let s: Settings = serde_json::from_str(r#"{"led_mode":"Rainbow","display_source":"CpuTemp","color":[255,0,0],"brightness":1.0,"speed":1.0,"cycle_interval_secs":4,"temp_unit":"Celsius","is_on":true}"#).unwrap();
    assert!(s.case_sync);
}
```
  (Match field names/enum spellings to the existing serde representation.)
- [ ] **Step 2:** `cargo test settings_without` → FAIL.
- [ ] **Step 3:** Implement the wiring and UI.
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets` (no new warnings), `pnpm build` → pass.
- [ ] **Step 5: Hardware check.** Hub in MB sync mode. Build `pnpm tauri build --no-bundle`, stop any running Open TRCC, start the build:
  - with `openrgb --server` already running → case follows the display; Rainbow/Breathing look smooth; quitting the app leaves that server running.
  - with no OpenRGB running → app starts `openrgb --server` itself, status `Connected: MSI B550M PRO-VDH (MS-7C95)`; quitting kills it (`pgrep -x openrgb` empty).
  - `pkill openrgb` while running → status `Connecting…`, display keeps animating, sync returns after the app restarts the server or the user does.
  - toggle Off → case goes black, status `Off`.
- [ ] **Step 6:** Commit `feat: sync case lighting with the display through OpenRGB`.

---

### Task 5: Installer and packaging

**Files:**
- Modify: `install.sh`, `src-tauri/tauri.conf.json`, `README.md`

**Interfaces:** none (shell).

`install.sh`: new `ensure_openrgb` called after `install_udev_rule`, skipped when `--no-openrgb` is passed or `openrgb` is on PATH. Package per manager, same shape as `ensure_fuse2`: pacman `openrgb`, apt `openrgb`, dnf `openrgb`, zypper `OpenRGB`, xbps `OpenRGB`, apk none. On missing package or failure: `warn` and continue. Then load i2c-dev: write `i2c-dev` to `/etc/modules-load.d/open-trcc.conf` (sudo) and `modprobe i2c-dev`; reload udev rules. Uninstall removes `/etc/modules-load.d/open-trcc.conf`, leaves OpenRGB. Arg parsing accepts `--no-openrgb` with or without `--install`. `tauri.conf.json`: `bundle.linux.deb.recommends: ["openrgb"]`. README: one paragraph on case sync and the hub's MB-sync mode.

- [ ] **Step 1:** `bash -n install.sh` and `shellcheck install.sh` (if available) → clean.
- [ ] **Step 2:** Dry run on this machine with OpenRGB already installed: `bash install.sh --no-openrgb` → no OpenRGB step; `bash install.sh` → "OpenRGB already installed" path, i2c-dev conf written, app restarts.
- [ ] **Step 3:** In a container without OpenRGB (`docker run --rm -it archlinux` and `ubuntu:24.04`), run the `ensure_openrgb` function alone (source the script with a guard or extract it) → package installs.
- [ ] **Step 4:** Commit `feat(install): install OpenRGB and load i2c-dev for case sync`.

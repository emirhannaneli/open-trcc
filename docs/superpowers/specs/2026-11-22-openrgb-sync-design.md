# Case lighting sync through OpenRGB

## Intent

The Thermalright display's inner LEDs are driven by Open TRCC, but the ring
around it (and the fans) hang off a case ARGB hub. In "MB sync" mode that hub
follows the motherboard's ARGB header (JRAINBOW1 on the author's MSI
B550M PRO-VDH). Today nothing drives that header on Linux, so the ring is
dark or shows the hub's own effect, and the colours never match the display.

Goal: the case LEDs on the motherboard's ARGB headers show the same colour as
the display, frame by frame, on any motherboard, with no extra setup for users.

Verified on hardware (2026-11-22): with the hub in MB sync mode, OpenRGB 1.1
drives JRAINBOW1 on MS-7C95 once the zone is resized (it starts at 0 LEDs).
Its OpenRGB server speaks SDK protocol 6 and reports the board as type 0
(Motherboard), "MSI Mystic Light Device (185-byte)", with a `Direct` mode and
zones JRGB1, JRGB2, JRAINBOW1, JRAINBOW2, ONBOARD.

## Goals

- Every motherboard ARGB zone OpenRGB exposes follows the display colour at the
  display's frame rate (125 ms, see `FRAME_MS`).
- Works on every motherboard OpenRGB supports. Open TRCC holds no vendor protocol.
- Users do nothing beyond running the installer and flipping one toggle.
- Without OpenRGB, Open TRCC behaves exactly as today: no errors, no prompts.

## Non-goals

- A rainbow that flows across the case LEDs. Every LED gets one colour.
- Driving the keyboard, GPU, RAM or other non-motherboard OpenRGB devices.
- Choosing individual zones or devices in the UI.
- Spawning OpenRGB on Windows/macOS. There the sync connects only if the user
  already runs an OpenRGB server.

## Anti-goals

- Writing a vendor RGB protocol ourselves (MSI's has bricked controllers, and
  OpenRGB's code is GPL-2.0 while Open TRCC is MIT).
- Sync trouble affecting the display: a missing, slow or crashed OpenRGB must
  never stall or delay the cooler's LED frames.

## Approaches

1. **OpenRGB SDK client (picked).** OpenRGB runs as a local server; Open TRCC
   sends colours over its TCP SDK protocol. Every OpenRGB-supported board, no
   vendor code, low risk. Costs: OpenRGB must be installed and running.
2. **Talk to MSI Mystic Light directly.** No dependency, but only MSI boards
   (each family a different packet format), real bricking risk, and no way to
   reuse OpenRGB's GPL code under MIT.
3. **Manual matching.** User sets the hub and the app to the same static colour.
   No work, but no animation sync.

The user picked 1 once it was clear 2 can't cover other boards. Their concern
was the burden of running OpenRGB, which the installer and app lifecycle below
remove.

## Design

### Behaviour

- New persisted setting `case_sync: bool`, default **on** (user's decision:
  zero setup). Without OpenRGB it silently does nothing. Consequence accepted:
  users who already set motherboard lighting in OpenRGB get it overridden until
  they turn the toggle off. UI: a toggle row
  "Case lighting sync (OpenRGB)" next to "Start on login", with a status line:
  one of: "Connected: <device names>", "Connecting…", "OpenRGB not found"
  (no server reachable and no binary to start), "No supported motherboard
  lighting" (connected, but no Motherboard controller with Direct mode).
- Colour sent each frame = the display's colour for that frame, after brightness
  but **before** TRCC's 40% output scale (the scale compensates for the cooler's
  bright LEDs; ARGB strips should get full range). For Rainbow use the hue of the
  display's middle group. Static/Breathing/ColorCycle/CpuTemp/CpuLoad use
  `base_color`. Off/power off → black. Test mode → the test colour.
- Only send when the colour changed since the last send (static modes cost
  nothing).

### Code layout

- `src-tauri/src/openrgb.rs`: a small blocking client for the OpenRGB SDK
  protocol (documented, our own MIT code). Needs: protocol version handshake,
  set client name, controller count, controller data (parse type, name, modes,
  zones with type and leds_min/max/count), resize zone, set custom (direct) mode,
  update zone LEDs. TCP with short connect/read timeouts.
- `PacketBuilder` gains a function returning the sync colour for a frame
  (shares the colour logic already in `build_display_packet`).
- A dedicated sync thread owns the client. The device loop hands it the latest
  colour through a channel or a shared slot; it never waits on the sync thread.
  The sync thread drops stale colours and keeps only the newest.

### Connection lifecycle

1. When sync is on and not connected, try `127.0.0.1:6742`.
2. If nothing listens and the `openrgb` binary is on PATH (Linux), spawn
   `openrgb --server` headless as a child, then retry for a few seconds.
   An already-running OpenRGB (GUI or server) is used as is and never killed.
3. On connect: list controllers, keep those of type Motherboard (0) that
   have a mode named `Direct`; switch each to that mode (not SETCUSTOMMODE,
   which silently falls back to modes that may write controller flash); resize
   any resizable zone whose LED count is 0 to its `leds_max`; then drive all
   zones. Motherboard controllers without `Direct` are skipped, never written.
4. Any I/O error → drop the connection, retry with backoff (2 s, capped at 30 s).
5. Sync runs whether or not the cooler is connected: the case follows the
   chosen effect even when the display is unplugged or unsupported. The
   colour is computed every frame outside the display-send branch.
6. Turning sync off or quitting: send black once, disconnect, and stop the
   child if we spawned it.

### Installer (`install.sh`)

- Install OpenRGB with the system package manager, the same way `ensure_fuse2`
  does (pacman, apt, dnf, zypper, xbps, apk). Skip if `openrgb` is already on
  PATH. If no package is available, warn and continue: sync just stays off.
- The OpenRGB package ships its udev rules (`60-openrgb.rules`), which give the
  desktop user access to the motherboard controller; reload udev after install.
- Load `i2c-dev` now and on every boot (`/etc/modules-load.d/open-trcc.conf`),
  so boards whose RGB controller sits on SMBus (many ASUS/Gigabyte/ASRock)
  are visible to OpenRGB. No bootloader changes (`acpi_enforce_resources=lax`
  stays a manual step for the few boards that need it).
- Any OpenRGB version the distro ships is accepted; the client requests the
  lowest protocol version that has the calls above, so 0.9 servers work.
- `--no-openrgb` flag skips this step.
- Uninstall leaves OpenRGB alone (the user may use it for other things) but
  removes `/etc/modules-load.d/open-trcc.conf`.
- The .deb gets `Recommends: openrgb`. .rpm, Windows and macOS packages are
  unchanged; there the user installs OpenRGB themselves.

## Left to the builder

- Exact package names per distro, and whether to fall back to OpenRGB's
  official AppImage where no package exists.
- Which SDK protocol version to request (lowest that supports the calls above).
- How the status line is delivered (extend `DeviceStatus` vs. its own event).
- Test strategy for the client: protocol encoding/decoding against captured
  byte sequences; lifecycle tested by hand on the MS-7C95.

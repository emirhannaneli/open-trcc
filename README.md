# Open TRCC

A lightweight, open-source control center for Thermalright Digital ARGB and Vision coolers on **Linux**, **Windows**, and **macOS**. Built with Rust, Tauri, and React for real-time hardware monitoring and lighting control with minimal overhead.

> **Note:** Open TRCC is an independent, unofficial open-source project and is not affiliated with Thermalright.

## Supported Hardware

### HID Digital LED Coolers (Full Control)
- **Thermalright Frozen Magic 360 Digital ARGB Black V2** (`0416:8001`)
- **Thermalright Digital LED / Air Coolers** (`0416:8001`, `0418:5303`, `0418:5304`)

### Bulk USB Vision LCD Coolers (Detection)
- **Thermalright Vision / Warframe LCD Series** (`87ad:70db`, `0416:5302`, `0416:5406`, `0416:5408`, `0416:5409`)

## Key Features

- **Cross-Platform USB HID:** Direct communication via `/dev/hidraw*` on Linux and `hidapi` on Windows/macOS.
- **Live Telemetry:** Real-time CPU and GPU temperature/utilization monitoring.
- **Display Control:** Switch between CPU/GPU temperature, load percentage, configurable Auto Cycle (`1–15s`), `°C`/`°F`, and a full 7-segment test mode (`88`).
- **ARGB Lighting:** Rainbow, Color Cycle, Breathing, Static, Thermal Sync, and Load Sync with custom RGB/hex color, brightness, and speed controls.

## Installation

### Linux (any distribution)

```bash
curl -fsSL https://raw.githubusercontent.com/emirhannaneli/open-trcc/main/install.sh | bash
```

The installer puts the latest AppImage in `~/.local/share/open-trcc`, adds an app-menu entry, installs FUSE 2 if it is missing, and installs a udev rule (asks for `sudo` once) so the cooler works without root. Uninstall with `install.sh --uninstall`.

`.deb` and `.rpm` packages from the [Releases](https://github.com/emirhannaneli/open-trcc/releases) page include the udev rule as well.

**Case lighting sync.** The installer also installs [OpenRGB](https://openrgb.org) and loads the `i2c-dev` kernel module (skip with `install.sh --no-openrgb`). Open TRCC then starts an OpenRGB server on its own and mirrors the display's lighting to your case fans. Set the case ARGB hub to its "MB sync" mode and connect it to the motherboard's 5V 3-pin ARGB header (not the 12V 4-pin RGB header). You can turn the sync off in the app.

### Windows / macOS

Download the installer (`.exe`, `.msi`, or `.dmg`) from the [Releases](https://github.com/emirhannaneli/open-trcc/releases) page.

### Background and start on login

Closing the window keeps Open TRCC running in the system tray; use **Quit** in the tray menu to exit. It starts on login (straight to the tray) by default, which you can turn off under **Start on login**. Your lighting settings are saved and restored on every start.

## Development

```bash
pnpm install
pnpm tauri dev
```

## License

Released under the [MIT License](LICENSE).

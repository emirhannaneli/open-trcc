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

Download the latest installer for **Windows** (`.exe`, `.msi`), **macOS** (`.dmg`), or **Linux** (`.deb`, `.rpm`, `.AppImage`) from the [Releases](https://github.com/emirhannaneli/open-trcc/releases) page.

### Linux USB Permissions (`udev`)

On Linux, grant non-root access to the USB HID interface:

```bash
echo 'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="8001", MODE="0666"' | sudo tee /etc/udev/rules.d/99-thermalright.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
```

## Development

```bash
pnpm install
pnpm tauri dev
```

## License

Released under the [MIT License](LICENSE).

# Open TRCC

An open-source, unofficial Linux control center for Thermalright Digital ARGB coolers, built with **Tauri v2**, **Rust**, and **React**.

> **Disclaimer:** This is an independent community project and is not affiliated with or endorsed by Thermalright.

## Supported Hardware

- **Thermalright Frozen Magic 360 Digital ARGB Black V2** (`0416:8001` Nuvoton HID Transfer)
- Compatible 30-LED (14-segment + unit + 15-LED ring) `0416:8001` Digital coolers

## Features

- **Native Linux HID Communication:** Direct communication via `/dev/hidraw*` (`HIDIOCGRAWINFO`) with no Wine, Windows services, or external dependencies.
- **Live Hardware Telemetry:**
  - CPU temperature via `hwmon` (`k10temp`, `coretemp`, `cpu_thermal`) and CPU load via `sysinfo`.
  - GPU temperature & load via `nvidia-smi` or AMD/DRM sysfs (`amdgpu`, `gpu_busy_percent`).
- **Display Modes:** CPU Temp, CPU Usage, GPU Temp, GPU Usage, configurable Auto Cycle (`1–15s`), `°C`/`°F` switching, and Segment Test Mode (`88`).
- **ARGB Lighting:** Rainbow, Color Cycle, Breathing, Static, Thermal Sync, Load Sync, custom RGB/hex input, brightness (`0–100%`), and speed (`0.2x–3.0x`).

## Linux Setup (udev Rule)

Grant non-root access to the USB HID interface:

```bash
echo 'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="8001", MODE="0666"' | sudo tee /etc/udev/rules.d/99-thermalright.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
```

## Development & Build

```bash
pnpm install
pnpm tauri dev
```

To build a release bundle:

```bash
pnpm tauri build
```

## License

Licensed under the [MIT License](LICENSE).

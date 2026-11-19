use super::registry::{
    DeviceTransport, KnownDeviceSpec, SUPPORTED_DEVICES, is_supported_led_style, lookup_device,
};

#[cfg(target_os = "linux")]
mod platform {
    use super::{DeviceTransport, KnownDeviceSpec, SUPPORTED_DEVICES, is_supported_led_style, lookup_device};
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    use std::path::PathBuf;
    use std::thread::sleep;
    use std::time::Duration;

    pub struct PlatformDevice {
        device_path: Option<PathBuf>,
        detected_spec: Option<&'static KnownDeviceSpec>,
        /// Path the init handshake has been sent to; re-sent when the device reappears.
        initialized_path: Option<PathBuf>,
        led_style: Option<u8>,
    }

    impl PlatformDevice {
        pub fn new() -> Self {
            let mut dev = Self {
                device_path: None,
                detected_spec: None,
                initialized_path: None,
                led_style: None,
            };
            dev.refresh();
            dev
        }

        pub fn is_connected(&self) -> bool {
            self.detected_spec.is_some()
        }

        pub fn detected_spec(&self) -> Option<&'static KnownDeviceSpec> {
            self.detected_spec
        }

        pub fn led_style(&self) -> Option<u8> {
            self.led_style
        }

        pub fn refresh(&mut self) -> bool {
            if let Some((path, spec)) = find_hidraw_device() {
                if self.initialized_path.as_ref() != Some(&path) {
                    self.led_style = handshake(&path);
                    self.initialized_path = Some(path.clone());
                }
                self.device_path = Some(path);
                self.detected_spec = Some(spec);
                return true;
            }
            self.initialized_path = None;
            self.led_style = None;

            if let Some(spec) = find_nusb_lcd_device() {
                self.device_path = None;
                self.detected_spec = Some(spec);
                return true;
            }

            self.device_path = None;
            self.detected_spec = None;
            false
        }

        pub fn send_packet(&mut self, data: &[u8]) -> Result<(), String> {
            let spec = match self.detected_spec {
                Some(s) => s,
                None => {
                    if !self.refresh() {
                        return Err("Thermalright device not connected".to_string());
                    }
                    self.detected_spec.unwrap()
                }
            };

            if spec.transport != DeviceTransport::HidDigital {
                return Ok(());
            }

            let path = self
                .device_path
                .clone()
                .ok_or_else(|| "HID device path unavailable".to_string())?;

            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .map_err(|e| format!("Failed to open {:?}: {}", path, e))?;

            let chunk_size = 64;
            let mut offset = 0;

            while offset < data.len() {
                // hidraw: byte 0 is the report number (0 = unnumbered), payload follows.
                let mut chunk = [0u8; 65];
                let take = (data.len() - offset).min(chunk_size);
                chunk[1..1 + take].copy_from_slice(&data[offset..offset + take]);

                file.write_all(&chunk)
                    .map_err(|e| format!("Write error on {:?}: {}", path, e))?;

                offset += take;
                if offset < data.len() {
                    sleep(Duration::from_millis(30));
                }
            }

            Ok(())
        }
    }

    /// Sends TRCC's init command and reads back the reply. The reply's byte 5
    /// (byte 6 on Windows, after the report id) is the LED style / model number
    /// TRCC passes to FormLEDInit. Without this the device stays in its built-in demo.
    fn handshake(path: &PathBuf) -> Option<u8> {
        use std::io::Read;

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .ok()?;

        let mut init = [0u8; 65];
        init[1..5].copy_from_slice(&[0xda, 0xdb, 0xdc, 0xdd]);
        init[1 + 12] = 0x01;
        file.write_all(&init).ok()?;

        let mut buf = [0u8; 64];
        for _ in 0..20 {
            sleep(Duration::from_millis(25));
            match file.read(&mut buf) {
                Ok(n) if n >= 13 && buf[..4] == [0xda, 0xdb, 0xdc, 0xdd] && buf[12] == 0x01 => {
                    let style = buf[5];
                    eprintln!("[open-trcc] LED device handshake ok, style/model = {}", style);
                    if !is_supported_led_style(style) {
                        eprintln!(
                            "[open-trcc] warning: model {} is not the 30-LED layout; display mapping may be wrong",
                            style
                        );
                    }
                    return Some(style);
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => break,
            }
        }
        eprintln!("[open-trcc] LED device handshake sent, no reply");
        None
    }

    fn find_hidraw_device() -> Option<(PathBuf, &'static KnownDeviceSpec)> {
        const HIDIOCGRAWINFO: libc::c_ulong = 0x80084803;

        #[repr(C)]
        struct HidrawDevinfo {
            bustype: u32,
            vendor: i16,
            product: i16,
        }

        let entries = std::fs::read_dir("/dev").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("hidraw") {
                continue;
            }
            let path = entry.path();
            if let Ok(file) = OpenOptions::new().read(true).open(&path) {
                let mut info = HidrawDevinfo {
                    bustype: 0,
                    vendor: 0,
                    product: 0,
                };
                let ret = unsafe { libc::ioctl(file.as_raw_fd(), HIDIOCGRAWINFO, &mut info) };
                if ret >= 0
                    && let Some(spec) = lookup_device(info.vendor as u16, info.product as u16)
                    && spec.transport == DeviceTransport::HidDigital
                {
                    return Some((path, spec));
                }
            }
        }
        None
    }

    fn find_nusb_lcd_device() -> Option<&'static KnownDeviceSpec> {
        use nusb::MaybeFuture;
        let devices = nusb::list_devices().wait().ok()?;
        for dev in devices {
            if let Some(spec) = SUPPORTED_DEVICES.iter().find(|s| {
                s.transport != DeviceTransport::HidDigital
                    && s.vid == dev.vendor_id()
                    && s.pid == dev.product_id()
            }) {
                return Some(spec);
            }
        }
        None
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::{DeviceTransport, KnownDeviceSpec, SUPPORTED_DEVICES, is_supported_led_style, lookup_device};
    use hidapi::HidApi;
    use std::thread::sleep;
    use std::time::Duration;

    /// TRCC init command; the reply's byte 5 (after hidapi strips the report id)
    /// is the LED model number. Without it the device stays in its built-in demo.
    fn handshake(api: &HidApi, spec: &KnownDeviceSpec) -> Option<u8> {
        let device = api.open(spec.vid, spec.pid).ok()?;
        let mut init = [0u8; 65];
        init[1..5].copy_from_slice(&[0xda, 0xdb, 0xdc, 0xdd]);
        init[1 + 12] = 0x01;
        device.write(&init).ok()?;

        let mut buf = [0u8; 65];
        for _ in 0..5 {
            let n = device.read_timeout(&mut buf, 100).ok()?;
            // Some backends keep the report id in front; accept both layouts.
            for start in [0usize, 1] {
                let r = &buf[start..n.max(start)];
                if r.len() >= 13 && r[..4] == [0xda, 0xdb, 0xdc, 0xdd] && r[12] == 0x01 {
                    let style = r[5];
                    eprintln!("[open-trcc] LED device handshake ok, style/model = {}", style);
                    if !is_supported_led_style(style) {
                        eprintln!(
                            "[open-trcc] warning: model {} is not the 30-LED layout; not driving it",
                            style
                        );
                    }
                    return Some(style);
                }
            }
        }
        eprintln!("[open-trcc] LED device handshake sent, no reply");
        None
    }

    pub struct PlatformDevice {
        detected_spec: Option<&'static KnownDeviceSpec>,
        initialized: bool,
        led_style: Option<u8>,
    }

    impl PlatformDevice {
        pub fn new() -> Self {
            let mut dev = Self {
                detected_spec: None,
                initialized: false,
                led_style: None,
            };
            dev.refresh();
            dev
        }

        pub fn is_connected(&self) -> bool {
            self.detected_spec.is_some()
        }

        pub fn detected_spec(&self) -> Option<&'static KnownDeviceSpec> {
            self.detected_spec
        }

        pub fn led_style(&self) -> Option<u8> {
            self.led_style
        }

        pub fn refresh(&mut self) -> bool {
            if let Ok(api) = HidApi::new() {
                for dev in api.device_list() {
                    if let Some(spec) = lookup_device(dev.vendor_id(), dev.product_id())
                        && spec.transport == DeviceTransport::HidDigital
                    {
                        if !self.initialized {
                            self.led_style = handshake(&api, spec);
                            self.initialized = true;
                        }
                        self.detected_spec = Some(spec);
                        return true;
                    }
                }
            }
            self.initialized = false;
            self.led_style = None;

            if let Ok(devices) = {
                use nusb::MaybeFuture;
                nusb::list_devices().wait()
            } {
                for dev in devices {
                    if let Some(spec) = SUPPORTED_DEVICES.iter().find(|s| {
                        s.transport != DeviceTransport::HidDigital
                            && s.vid == dev.vendor_id()
                            && s.pid == dev.product_id()
                    }) {
                        self.detected_spec = Some(spec);
                        return true;
                    }
                }
            }

            self.detected_spec = None;
            false
        }

        pub fn send_packet(&mut self, data: &[u8]) -> Result<(), String> {
            let spec = match self.detected_spec {
                Some(s) => s,
                None => {
                    if !self.refresh() {
                        return Err("Thermalright device not connected".to_string());
                    }
                    self.detected_spec.unwrap()
                }
            };

            if spec.transport != DeviceTransport::HidDigital {
                return Ok(());
            }

            let api = HidApi::new().map_err(|e| format!("HidApi init error: {}", e))?;
            let device = api
                .open(spec.vid, spec.pid)
                .map_err(|e| format!("Failed to open {:04x}:{:04x}: {}", spec.vid, spec.pid, e))?;

            let chunk_size = 64;
            let mut offset = 0;

            while offset < data.len() {
                let mut report = [0u8; 65];
                let take = (data.len() - offset).min(chunk_size);
                report[1..1 + take].copy_from_slice(&data[offset..offset + take]);

                device
                    .write(&report)
                    .map_err(|e| format!("HID write error: {}", e))?;

                offset += take;
                if offset < data.len() {
                    sleep(Duration::from_millis(30));
                }
            }

            Ok(())
        }
    }
}

pub struct ThermalrightDevice {
    inner: platform::PlatformDevice,
}

impl ThermalrightDevice {
    pub fn new() -> Self {
        Self {
            inner: platform::PlatformDevice::new(),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    pub fn detected_spec(&self) -> Option<&'static KnownDeviceSpec> {
        self.inner.detected_spec()
    }

    /// Model number from the init handshake, if the device answered.
    pub fn led_style(&self) -> Option<u8> {
        self.inner.led_style()
    }

    /// False when the device reported a model whose LED layout we don't map yet.
    pub fn supports_led_layout(&self) -> bool {
        self.led_style().is_none_or(is_supported_led_style)
    }

    pub fn refresh_device(&mut self) -> bool {
        self.inner.refresh()
    }

    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), String> {
        self.inner.send_packet(data)
    }
}

impl Default for ThermalrightDevice {
    fn default() -> Self {
        Self::new()
    }
}

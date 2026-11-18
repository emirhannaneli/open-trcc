use super::registry::{DeviceTransport, KnownDeviceSpec, SUPPORTED_DEVICES, lookup_device};

#[cfg(target_os = "linux")]
mod platform {
    use super::{DeviceTransport, KnownDeviceSpec, SUPPORTED_DEVICES, lookup_device};
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
    }

    impl PlatformDevice {
        pub fn new() -> Self {
            let mut dev = Self {
                device_path: None,
                detected_spec: None,
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

        pub fn refresh(&mut self) -> bool {
            if let Some((path, spec)) = find_hidraw_device() {
                self.device_path = Some(path);
                self.detected_spec = Some(spec);
                return true;
            }

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
                let mut chunk = [0u8; 64];
                let take = (data.len() - offset).min(chunk_size);
                chunk[..take].copy_from_slice(&data[offset..offset + take]);

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
                s.transport == DeviceTransport::BulkLcd
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
    use super::{DeviceTransport, KnownDeviceSpec, SUPPORTED_DEVICES, lookup_device};
    use hidapi::HidApi;
    use std::thread::sleep;
    use std::time::Duration;

    pub struct PlatformDevice {
        detected_spec: Option<&'static KnownDeviceSpec>,
    }

    impl PlatformDevice {
        pub fn new() -> Self {
            let mut dev = Self {
                detected_spec: None,
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

        pub fn refresh(&mut self) -> bool {
            if let Ok(api) = HidApi::new() {
                for dev in api.device_list() {
                    if let Some(spec) = lookup_device(dev.vendor_id(), dev.product_id())
                        && spec.transport == DeviceTransport::HidDigital
                    {
                        self.detected_spec = Some(spec);
                        return true;
                    }
                }
            }

            if let Ok(devices) = {
                use nusb::MaybeFuture;
                nusb::list_devices().wait()
            } {
                for dev in devices {
                    if let Some(spec) = SUPPORTED_DEVICES.iter().find(|s| {
                        s.transport == DeviceTransport::BulkLcd
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

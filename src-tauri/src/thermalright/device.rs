use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

pub const VID: u16 = 0x0416;
pub const PID: u16 = 0x8001;

pub struct ThermalrightDevice {
    device_path: Option<PathBuf>,
}

impl ThermalrightDevice {
    pub fn new() -> Self {
        let mut dev = Self { device_path: None };
        dev.refresh_device();
        dev
    }

    pub fn is_connected(&self) -> bool {
        self.device_path.is_some()
    }

    pub fn refresh_device(&mut self) -> bool {
        // Find matching hidraw node
        if let Some(path) = find_hidraw_device(VID, PID) {
            self.device_path = Some(path);
            true
        } else {
            self.device_path = None;
            false
        }
    }

    /// Sends a packet (e.g. 110-byte display packet) to the device in 64-byte chunks with 30ms throttling.
    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), String> {
        let path = match &self.device_path {
            Some(p) => p.clone(),
            None => {
                if !self.refresh_device() {
                    return Err("Thermalright device not connected".to_string());
                }
                self.device_path.as_ref().unwrap().clone()
            }
        };

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
            let remaining = data.len() - offset;
            let take = remaining.min(chunk_size);
            chunk[..take].copy_from_slice(&data[offset..offset + take]);

            file.write_all(&chunk)
                .map_err(|e| {
                    // Invalidate path on failure so next attempt re-detects
                    format!("Write error on {:?}: {}", path, e)
                })?;

            offset += take;

            if offset < data.len() {
                sleep(Duration::from_millis(30));
            }
        }

        Ok(())
    }
}

// Scans /dev/hidraw* using HIDIOCGRAWINFO ioctl to find device matching VID:PID
fn find_hidraw_device(target_vid: u16, target_pid: u16) -> Option<PathBuf> {
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
        let name_str = name.to_string_lossy();
        if name_str.starts_with("hidraw") {
            let path = entry.path();
            if let Ok(file) = OpenOptions::new().read(true).open(&path) {
                use std::os::unix::io::AsRawFd;
                let mut info = HidrawDevinfo {
                    bustype: 0,
                    vendor: 0,
                    product: 0,
                };
                let ret = unsafe {
                    libc::ioctl(
                        file.as_raw_fd(),
                        HIDIOCGRAWINFO,
                        &mut info as *mut _ as *mut libc::c_void,
                    )
                };
                if ret >= 0 {
                    let vid = info.vendor as u16;
                    let pid = info.product as u16;
                    if vid == target_vid && pid == target_pid {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

use std::fs;
use std::path::PathBuf;
use std::process::Command;
#[cfg(not(target_os = "linux"))]
use sysinfo::Components;
use sysinfo::System;

pub struct SystemStats {
    sys: System,
    #[cfg(not(target_os = "linux"))]
    components: Components,
    hwmon_cpu_temp_path: Option<PathBuf>,
    hwmon_amdgpu_temp_path: Option<PathBuf>,
    drm_gpu_busy_path: Option<PathBuf>,
    has_nvidia_smi: bool,
    last_gpu_temp: f32,
    last_gpu_usage: f32,
}

impl SystemStats {
    pub fn new() -> Self {
        let has_nvidia_smi = Command::new("nvidia-smi")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);

        let mut stats = Self {
            sys: System::new_all().expect("failed to init sysinfo"),
            #[cfg(not(target_os = "linux"))]
            components: Components::new_with_refreshed_list()
                .expect("failed to init sysinfo components"),
            hwmon_cpu_temp_path: None,
            hwmon_amdgpu_temp_path: None,
            drm_gpu_busy_path: None,
            has_nvidia_smi,
            last_gpu_temp: 0.0,
            last_gpu_usage: 0.0,
        };
        stats.detect_sensors();
        stats
    }

    fn detect_sensors(&mut self) {
        if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(name) = fs::read_to_string(path.join("name")) {
                    let name = name.trim();
                    let temp_input = path.join("temp1_input");
                    if !temp_input.exists() {
                        continue;
                    }
                    if (name == "k10temp" || name == "coretemp" || name == "cpu_thermal")
                        && self.hwmon_cpu_temp_path.is_none()
                    {
                        self.hwmon_cpu_temp_path = Some(temp_input);
                    } else if name == "amdgpu" && self.hwmon_amdgpu_temp_path.is_none() {
                        self.hwmon_amdgpu_temp_path = Some(temp_input);
                    }
                }
            }
        }

        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let busy = entry.path().join("device/gpu_busy_percent");
                if busy.exists() {
                    self.drm_gpu_busy_path = Some(busy);
                    break;
                }
            }
        }
    }

    pub fn get_cpu_temp(&mut self) -> f32 {
        #[cfg(target_os = "linux")]
        {
            if let Some(ref path) = self.hwmon_cpu_temp_path
                && let Some(val) = read_sysfs_f32(path)
            {
                return val / 1000.0;
            }
            self.detect_sensors();
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = self.components.refresh(false);
            for comp in &self.components {
                let label = comp.label().to_lowercase();
                if (label.contains("cpu") || label.contains("package") || label.contains("tctl"))
                    && let Some(temp) = comp.temperature()
                {
                    return temp;
                }
            }
            if let Some(temp) = self.components.iter().filter_map(|c| c.temperature()).next() {
                return temp;
            }
        }

        45.0
    }

    pub fn get_cpu_usage(&mut self) -> f32 {
        self.sys.refresh_cpu_usage();
        self.sys.global_cpu_usage()
    }

    pub fn refresh_gpu(&mut self) -> (f32, f32) {
        if self.has_nvidia_smi
            && let Ok(output) = Command::new("nvidia-smi")
                .args([
                    "--query-gpu=temperature.gpu,utilization.gpu",
                    "--format=csv,noheader,nounits",
                ])
                .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = text.lines().next() {
                let mut parts = line.split(',').map(|s| s.trim().parse::<f32>());
                if let (Some(Ok(temp)), Some(Ok(usage))) = (parts.next(), parts.next()) {
                    self.last_gpu_temp = temp;
                    self.last_gpu_usage = usage;
                    return (temp, usage);
                }
            }
        }

        if let Some(ref path) = self.hwmon_amdgpu_temp_path
            && let Some(val) = read_sysfs_f32(path)
        {
            self.last_gpu_temp = val / 1000.0;
        }

        if let Some(ref path) = self.drm_gpu_busy_path
            && let Some(val) = read_sysfs_f32(path)
        {
            self.last_gpu_usage = val;
        }

        (self.last_gpu_temp, self.last_gpu_usage)
    }

    pub fn cached_gpu(&self) -> (f32, f32) {
        (self.last_gpu_temp, self.last_gpu_usage)
    }
}

impl Default for SystemStats {
    fn default() -> Self {
        Self::new()
    }
}

fn read_sysfs_f32(path: &PathBuf) -> Option<f32> {
    fs::read_to_string(path).ok()?.trim().parse::<f32>().ok()
}

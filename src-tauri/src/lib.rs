pub mod thermalright;

use std::sync::{Arc, Mutex};
use std::thread::{sleep, spawn};
use std::time::Duration;
use tauri::{Emitter, State};

use thermalright::{
    DisplaySource, LedMode, PacketBuilder, SystemStats, TempUnit, ThermalrightDevice,
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DeviceStatus {
    pub connected: bool,
    pub device_name: String,
    pub device_usb_id: String,
    pub cpu_temp: f32,
    pub cpu_usage: f32,
    pub gpu_temp: f32,
    pub gpu_usage: f32,
    pub led_mode: LedMode,
    pub display_source: DisplaySource,
    pub active_source: DisplaySource,
    pub color: (u8, u8, u8),
    pub brightness: f32,
    pub speed: f32,
    pub cycle_interval_secs: u32,
    pub test_mode: bool,
    pub temp_unit: TempUnit,
    pub is_on: bool,
}

pub struct AppState {
    pub status: DeviceStatus,
    pub builder: PacketBuilder,
    pub device: ThermalrightDevice,
    pub stats: SystemStats,
    pub tick: u64,
}

#[tauri::command]
fn get_device_status(state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok(app.status.clone())
}

#[tauri::command]
fn set_led_mode(mode: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let new_mode = match mode.to_lowercase().as_str() {
        "static" => LedMode::Static,
        "breathing" => LedMode::Breathing,
        "colorcycle" | "color_cycle" => LedMode::ColorCycle,
        "rainbow" => LedMode::Rainbow,
        "cputemp" | "cpu_temp" => LedMode::CpuTemp,
        "cpuload" | "cpu_load" => LedMode::CpuLoad,
        "off" => LedMode::Off,
        _ => return Err("Invalid LED mode".into()),
    };
    app.status.led_mode = new_mode;
    app.builder.mode = new_mode;
    Ok(app.status.clone())
}

#[tauri::command]
fn set_display_source(
    source: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let new_source = match source.to_lowercase().as_str() {
        "cputemp" | "cpu_temp" => DisplaySource::CpuTemp,
        "cpuusage" | "cpu_usage" => DisplaySource::CpuUsage,
        "gputemp" | "gpu_temp" => DisplaySource::GpuTemp,
        "gpuusage" | "gpu_usage" => DisplaySource::GpuUsage,
        "autocycle" | "auto_cycle" => DisplaySource::AutoCycle,
        _ => return Err("Invalid display source".into()),
    };
    app.status.display_source = new_source;
    app.builder.display_source = new_source;
    app.status.active_source = app.builder.resolve_active_source(app.tick);
    Ok(app.status.clone())
}

#[tauri::command]
fn set_led_color(r: u8, g: u8, b: u8, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.status.color = (r, g, b);
    app.builder.color = (r, g, b);
    Ok(app.status.clone())
}

#[tauri::command]
fn set_brightness(val: f32, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let clamped = val.clamp(0.0, 1.0);
    app.status.brightness = clamped;
    app.builder.brightness = clamped;
    Ok(app.status.clone())
}

#[tauri::command]
fn set_speed(val: f32, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let clamped = val.clamp(0.2, 3.0);
    app.status.speed = clamped;
    app.builder.speed = clamped;
    Ok(app.status.clone())
}

#[tauri::command]
fn set_cycle_interval(
    secs: u32,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let clamped = secs.clamp(1, 30);
    app.status.cycle_interval_secs = clamped;
    app.builder.cycle_interval_secs = clamped;
    app.status.active_source = app.builder.resolve_active_source(app.tick);
    Ok(app.status.clone())
}

#[tauri::command]
fn set_test_mode(enabled: bool, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.status.test_mode = enabled;
    app.builder.test_mode = enabled;
    Ok(app.status.clone())
}

#[tauri::command]
fn set_power(on: bool, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.status.is_on = on;
    app.builder.mode = if on { app.status.led_mode } else { LedMode::Off };
    Ok(app.status.clone())
}

#[tauri::command]
fn set_temp_unit(unit: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<DeviceStatus, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let new_unit = match unit.to_lowercase().as_str() {
        "c" | "celsius" => TempUnit::Celsius,
        "f" | "fahrenheit" => TempUnit::Fahrenheit,
        _ => return Err("Invalid temperature unit".into()),
    };
    app.status.temp_unit = new_unit;
    app.builder.temp_unit = new_unit;
    Ok(app.status.clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_status = DeviceStatus {
        connected: false,
        device_name: "Frozen Magic 360 Digital".to_string(),
        device_usb_id: "0416:8001".to_string(),
        cpu_temp: 0.0,
        cpu_usage: 0.0,
        gpu_temp: 0.0,
        gpu_usage: 0.0,
        led_mode: LedMode::Rainbow,
        display_source: DisplaySource::CpuTemp,
        active_source: DisplaySource::CpuTemp,
        color: (255, 106, 0),
        brightness: 1.0,
        speed: 1.0,
        cycle_interval_secs: 4,
        test_mode: false,
        temp_unit: TempUnit::Celsius,
        is_on: true,
    };

    let app_state = Arc::new(Mutex::new(AppState {
        status: initial_status,
        builder: PacketBuilder::default(),
        device: ThermalrightDevice::new(),
        stats: SystemStats::new(),
        tick: 0,
    }));

    let background_state = Arc::clone(&app_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_device_status,
            set_led_mode,
            set_display_source,
            set_led_color,
            set_brightness,
            set_speed,
            set_cycle_interval,
            set_test_mode,
            set_power,
            set_temp_unit,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            spawn(move || {
                loop {
                    sleep(Duration::from_millis(500));

                    if let Ok(mut state) = background_state.lock() {
                        state.tick = state.tick.wrapping_add(1);

                        let cpu_temp = state.stats.get_cpu_temp();
                        let cpu_usage = state.stats.get_cpu_usage();
                        let (gpu_temp, gpu_usage) = if state.tick % 2 == 1 {
                            state.stats.refresh_gpu()
                        } else {
                            state.stats.cached_gpu()
                        };

                        state.status.cpu_temp = cpu_temp;
                        state.status.cpu_usage = cpu_usage;
                        state.status.gpu_temp = gpu_temp;
                        state.status.gpu_usage = gpu_usage;
                        state.status.active_source = state.builder.resolve_active_source(state.tick);

                        let is_conn = state.device.refresh_device();
                        state.status.connected = is_conn;
                        if let Some(spec) = state.device.detected_spec() {
                            state.status.device_name = spec.name.to_string();
                            state.status.device_usb_id = format!("{:04x}:{:04x}", spec.vid, spec.pid);
                        }

                        if is_conn && state.status.is_on {
                            let packet = state.builder.build_display_packet(
                                cpu_temp,
                                cpu_usage,
                                gpu_temp,
                                gpu_usage,
                                state.tick,
                            );
                            let _ = state.device.send_packet(&packet);
                        }

                        let _ = handle.emit("stats-update", state.status.clone());
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

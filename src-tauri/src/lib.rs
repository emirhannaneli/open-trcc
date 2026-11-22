#[cfg(target_os = "linux")]
mod session_autostart;
pub mod thermalright;

use std::sync::{Arc, Mutex};
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, State};

use thermalright::{
    DisplaySource, FRAME_MS, LedMode, PacketBuilder, SystemStats, TempUnit, ThermalrightDevice,
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
    /// LED frame counter; the device loop advances it every `FRAME_MS`.
    pub frame: u64,
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
    // While powered off, remember the mode but keep the display dark.
    if app.status.is_on {
        app.builder.mode = new_mode;
    }
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
    app.status.active_source = app.builder.resolve_active_source(app.frame);
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
    app.status.active_source = app.builder.resolve_active_source(app.frame);
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

/// User choices that survive restarts (stored as JSON in the app config dir).
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
struct Settings {
    led_mode: LedMode,
    display_source: DisplaySource,
    color: (u8, u8, u8),
    brightness: f32,
    speed: f32,
    cycle_interval_secs: u32,
    temp_unit: TempUnit,
    is_on: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            led_mode: LedMode::Rainbow,
            display_source: DisplaySource::CpuTemp,
            color: (255, 106, 0),
            brightness: 1.0,
            speed: 1.0,
            cycle_interval_secs: 4,
            temp_unit: TempUnit::Celsius,
            is_on: true,
        }
    }
}

impl Settings {
    fn from_status(s: &DeviceStatus) -> Self {
        Self {
            led_mode: s.led_mode,
            display_source: s.display_source,
            color: s.color,
            brightness: s.brightness,
            speed: s.speed,
            cycle_interval_secs: s.cycle_interval_secs,
            temp_unit: s.temp_unit,
            is_on: s.is_on,
        }
    }

    fn apply(&self, app: &mut AppState) {
        let st = &mut app.status;
        st.led_mode = self.led_mode;
        st.display_source = self.display_source;
        st.color = self.color;
        st.brightness = self.brightness.clamp(0.0, 1.0);
        st.speed = self.speed.clamp(0.2, 3.0);
        st.cycle_interval_secs = self.cycle_interval_secs.clamp(1, 30);
        st.temp_unit = self.temp_unit;
        st.is_on = self.is_on;

        let b = &mut app.builder;
        b.mode = if st.is_on { st.led_mode } else { LedMode::Off };
        b.display_source = st.display_source;
        b.color = st.color;
        b.brightness = st.brightness;
        b.speed = st.speed;
        b.cycle_interval_secs = st.cycle_interval_secs;
        b.temp_unit = st.temp_unit;
    }

    fn load(path: &std::path::Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    fn save(&self, path: &std::path::Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Passed by the autostart entry so the app boots straight into the tray.
const HIDDEN_ARG: &str = "--hidden";

/// Sensors, device detection and UI updates run every 10th LED frame (500 ms).
const STATS_EVERY_FRAMES: u64 = 10;

/// Mirror the XDG autostart state into a systemd user unit, so compositors that
/// skip `~/.config/autostart` (Hyprland, Sway, ...) still launch us on login.
#[cfg(target_os = "linux")]
fn sync_session_unit<R: tauri::Runtime>(app: &impl Manager<R>, enabled: bool) {
    let result = if enabled {
        let appimage = app.env().appimage;
        session_autostart::enable(appimage.as_deref().map(std::path::Path::new), HIDDEN_ARG)
    } else {
        session_autostart::disable()
    };
    if let Err(e) = result {
        eprintln!("[open-trcc] could not update systemd autostart unit: {}", e);
    }
}

#[cfg(not(target_os = "linux"))]
fn sync_session_unit<R: tauri::Runtime>(_app: &impl Manager<R>, _enabled: bool) {}

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(enabled: bool, app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())?;
    let enabled = launcher.is_enabled().map_err(|e| e.to_string())?;
    sync_session_unit(&app, enabled);
    Ok(enabled)
}

/// Turn autostart on the first time the app runs; afterwards respect the user's choice.
fn enable_autostart_on_first_run(app: &tauri::App, config_dir: &std::path::Path) {
    use tauri_plugin_autostart::ManagerExt;
    let marker = config_dir.join("autostart-initialized");
    if !marker.exists() {
        match app.autolaunch().enable() {
            Ok(()) => {
                let _ = std::fs::create_dir_all(config_dir);
                let _ = std::fs::write(&marker, "");
            }
            Err(e) => eprintln!("[open-trcc] could not enable autostart: {}", e),
        }
    }
    // Every start: brings existing installs up to date and refreshes the unit's
    // path if the AppImage moved.
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        sync_session_unit(app, enabled);
    }
}

/// Sends an all-black frame so the cooler doesn't freeze on the last frame
/// once we stop updating it. Saved settings are untouched, so the next start
/// lights up with the user's choices again.
fn blank_display(app: &tauri::AppHandle) {
    let state = app.state::<Arc<Mutex<AppState>>>();
    let Ok(mut st) = state.lock() else { return };
    st.builder.mode = LedMode::Off;
    if st.device.supports_led_layout() {
        let packet = st.builder.build_display_packet(0.0, 0.0, 0.0, 0.0, st.frame);
        let _ = st.device.send_packet(&packet);
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "Show Open TRCC", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &separator, &quit])?;

    // Linux trays (Waybar, KDE, ...) cache icons by file name, which tauri derives
    // from this id; versioning it makes an updated icon show up after upgrades.
    let tray_id = concat!("open-trcc-", env!("CARGO_PKG_VERSION"));
    let mut builder = TrayIconBuilder::with_id(tray_id)
        .tooltip("Open TRCC")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "hide" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
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
        frame: 0,
    }));

    let background_state = Arc::clone(&app_state);

    tauri::Builder::default()
        // Must be registered first: a second launch just focuses the running one,
        // so two processes never fight over the HID device.
        // A login launch (XDG entry and systemd unit can both fire) stays in the tray.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == HIDDEN_ARG) {
                show_main_window(app);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([HIDDEN_ARG])
                .build(),
        )
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
            get_autostart,
            set_autostart,
        ])
        .on_window_event(|window, event| {
            // Closing the window only hides it; the LED loop keeps running in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            setup_tray(app)?;
            let handle = app.handle().clone();

            let config_dir = app.path().app_config_dir()?;
            let settings_path = config_dir.join("settings.json");
            if let Some(saved) = Settings::load(&settings_path)
                && let Ok(mut state) = background_state.lock()
            {
                saved.apply(&mut state);
            }
            enable_autostart_on_first_run(app, &config_dir);

            // The window starts hidden (tauri.conf.json) to avoid a flash on login.
            if !std::env::args().any(|a| a == HIDDEN_ARG) {
                show_main_window(&handle);
            }

            spawn(move || {
                let mut saved_settings = background_state
                    .lock()
                    .map(|st| Settings::from_status(&st.status))
                    .ok();
                let frame_len = Duration::from_millis(FRAME_MS);
                let mut next_frame = Instant::now();
                loop {
                    next_frame += frame_len;
                    sleep(next_frame.saturating_duration_since(Instant::now()));

                    let Ok(mut guard) = background_state.lock() else { continue };
                    let state = &mut *guard;
                    state.frame = state.frame.wrapping_add(1);

                    // LEDs animate every frame; sensors, device detection and the UI
                    // keep the slower pace.
                    if state.frame % STATS_EVERY_FRAMES == 0 {
                        let cpu_temp = state.stats.get_cpu_temp();
                        let cpu_usage = state.stats.get_cpu_usage();
                        let (gpu_temp, gpu_usage) = if state.frame % (2 * STATS_EVERY_FRAMES) == 0 {
                            state.stats.refresh_gpu()
                        } else {
                            state.stats.cached_gpu()
                        };

                        state.status.cpu_temp = cpu_temp;
                        state.status.cpu_usage = cpu_usage;
                        state.status.gpu_temp = gpu_temp;
                        state.status.gpu_usage = gpu_usage;
                        state.status.active_source = state.builder.resolve_active_source(state.frame);

                        let is_conn = state.device.refresh_device();
                        state.status.connected = is_conn;
                        let supported = state.device.supports_led_layout();
                        if let Some(spec) = state.device.detected_spec() {
                            state.status.device_name = match state.device.led_style() {
                                Some(style) if !supported => {
                                    format!("{} (model {}, LED layout not supported yet)", spec.name, style)
                                }
                                _ => spec.name.to_string(),
                            };
                            state.status.device_usb_id = format!("{:04x}:{:04x}", spec.vid, spec.pid);
                        }

                        let current = Settings::from_status(&state.status);
                        if saved_settings.as_ref() != Some(&current) {
                            current.save(&settings_path);
                            saved_settings = Some(current);
                        }

                        let _ = handle.emit("stats-update", state.status.clone());
                    }

                    // Sending the 30-LED frame to another layout lights random LEDs.
                    // Keep sending while powered off: builder.mode is Off, so the frame is all black.
                    if state.status.connected && state.device.supports_led_layout() {
                        let s = &state.status;
                        let packet = state.builder.build_display_packet(
                            s.cpu_temp,
                            s.cpu_usage,
                            s.gpu_temp,
                            s.gpu_usage,
                            state.frame,
                        );
                        let _ = state.device.send_packet(&packet);
                    }
                    drop(guard);
                    // After a slow send, wait a full frame instead of bursting to catch up.
                    next_frame = next_frame.max(Instant::now());
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Covers tray Quit and any other orderly shutdown.
            if let tauri::RunEvent::Exit = event {
                blank_display(app);
            }
        });
}

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./App.css";

import thermalrightLogo from "./assets/thermalright-logo.svg";

const REPO_URL = "https://github.com/emirhannaneli/open-trcc";
const WEBSITE_URL = "https://emirman.dev";

type LedMode = "Static" | "Breathing" | "ColorCycle" | "Rainbow" | "CpuTemp" | "CpuLoad" | "Off";
type DisplaySource = "CpuTemp" | "CpuUsage" | "GpuTemp" | "GpuUsage" | "AutoCycle";

interface DeviceStatus {
  connected: boolean;
  device_name: string;
  device_usb_id: string;
  cpu_temp: number;
  cpu_usage: number;
  gpu_temp: number;
  gpu_usage: number;
  led_mode: LedMode;
  display_source: DisplaySource;
  active_source: DisplaySource;
  color: [number, number, number];
  brightness: number;
  speed: number;
  cycle_interval_secs: number;
  test_mode: boolean;
  temp_unit: "Celsius" | "Fahrenheit";
  is_on: boolean;
  case_sync: boolean;
  case_sync_status: string;
}

const PRESET_COLORS: { name: string; rgb: [number, number, number]; hex: string }[] = [
  { name: "Thermalright Orange", rgb: [255, 106, 0], hex: "#ff6a00" },
  { name: "Crimson", rgb: [239, 68, 68], hex: "#ef4444" },
  { name: "Cyan", rgb: [6, 182, 212], hex: "#06b6d4" },
  { name: "Cobalt", rgb: [59, 130, 246], hex: "#3b82f6" },
  { name: "Emerald", rgb: [16, 185, 129], hex: "#10b981" },
  { name: "Violet", rgb: [168, 85, 247], hex: "#a855f7" },
  { name: "Amber", rgb: [245, 158, 11], hex: "#f59e0b" },
  { name: "White", rgb: [255, 255, 255], hex: "#ffffff" },
];

const LIGHTING_MODES: { id: LedMode; label: string; hint: string }[] = [
  { id: "Rainbow", label: "Rainbow", hint: "Flowing spectrum" },
  { id: "ColorCycle", label: "Color cycle", hint: "7-color smooth fade" },
  { id: "Breathing", label: "Breathing", hint: "Single color pulse" },
  { id: "Static", label: "Static", hint: "Solid color" },
  { id: "CpuTemp", label: "Thermal sync", hint: "Follows temperature" },
  { id: "CpuLoad", label: "Load sync", hint: "Follows utilization" },
  { id: "Off", label: "Off", hint: "Ring disabled" },
];

const DISPLAY_SOURCES: { id: DisplaySource; label: string; hint: string }[] = [
  { id: "CpuTemp", label: "CPU Temp", hint: "Processor temperature" },
  { id: "CpuUsage", label: "CPU Usage", hint: "Processor load %" },
  { id: "GpuTemp", label: "GPU Temp", hint: "Graphics temperature" },
  { id: "GpuUsage", label: "GPU Usage", hint: "Graphics load %" },
  { id: "AutoCycle", label: "Auto Cycle", hint: "Rotates every 4s" },
];

function rgbToHex(r: number, g: number, b: number): string {
  return (
    "#" +
    [r, g, b]
      .map((channel) => Math.max(0, Math.min(255, channel)).toString(16).padStart(2, "0"))
      .join("")
  );
}

function hexToRgb(hex: string): [number, number, number] {
  const num = parseInt(hex.replace("#", ""), 16);
  return [(num >> 16) & 255, (num >> 8) & 255, num & 255];
}

function formatTemperature(celsius: number, unit: "Celsius" | "Fahrenheit"): number {
  return unit === "Celsius" ? Math.round(celsius) : Math.round((celsius * 9) / 5 + 32);
}

export default function App() {
  const [status, setStatus] = useState<DeviceStatus>({
    connected: false,
    device_name: "Frozen Magic 360 Digital",
    device_usb_id: "0416:8001",
    cpu_temp: 0,
    cpu_usage: 0,
    gpu_temp: 0,
    gpu_usage: 0,
    led_mode: "Rainbow",
    display_source: "CpuTemp",
    active_source: "CpuTemp",
    color: [255, 106, 0],
    brightness: 1.0,
    speed: 1.0,
    cycle_interval_secs: 4,
    test_mode: false,
    temp_unit: "Celsius",
    is_on: true,
    case_sync: true,
    case_sync_status: "",
  });

  const [activeTab, setActiveTab] = useState<"display" | "lighting">("display");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [showCaseHelp, setShowCaseHelp] = useState(false);
  const [autostart, setAutostart] = useState<boolean | null>(null);

  useEffect(() => {
    if (!showCaseHelp) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setShowCaseHelp(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showCaseHelp]);

  useEffect(() => {
    invoke<boolean>("get_autostart")
      .then(setAutostart)
      .catch((error) => console.error("Autostart status error:", error));

    invoke<DeviceStatus>("get_device_status")
      .then(setStatus)
      .catch((error) => console.error("Initial status error:", error));

    const unlistenPromise = listen<DeviceStatus>("stats-update", (event) => {
      setStatus(event.payload);
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const sendCommand = async (command: string, args: Record<string, unknown>) => {
    try {
      const updated = await invoke<DeviceStatus>(command, args);
      setStatus(updated);
    } catch (error) {
      console.error(`${command} error:`, error);
    }
  };

  const toggleAutostart = async () => {
    try {
      setAutostart(await invoke<boolean>("set_autostart", { enabled: !autostart }));
    } catch (error) {
      console.error("set_autostart error:", error);
    }
  };

  const cpuDisplayTemp = formatTemperature(status.cpu_temp, status.temp_unit);
  const gpuDisplayTemp = formatTemperature(status.gpu_temp, status.temp_unit);
  const unitSymbol = status.temp_unit === "Celsius" ? "C" : "F";
  const currentHex = rgbToHex(status.color[0], status.color[1], status.color[2]);

  const getActiveReadout = () => {
    switch (status.active_source) {
      case "GpuTemp":
        return { value: gpuDisplayTemp, unit: `°${unitSymbol}`, label: "GPU TEMP" };
      case "CpuUsage":
        return { value: Math.round(status.cpu_usage), unit: "%", label: "CPU LOAD" };
      case "GpuUsage":
        return { value: Math.round(status.gpu_usage), unit: "%", label: "GPU LOAD" };
      case "CpuTemp":
      default:
        return { value: cpuDisplayTemp, unit: `°${unitSymbol}`, label: "CPU TEMP" };
    }
  };

  const activeReadout = getActiveReadout();

  const getRingColor = () => {
    if (!status.is_on || status.led_mode === "Off") return "transparent";
    if (status.led_mode === "CpuTemp") {
      const refTemp = status.active_source === "GpuTemp" ? status.gpu_temp : status.cpu_temp;
      if (refTemp < 45) return "#06b6d4";
      if (refTemp < 65) return "#10b981";
      if (refTemp < 80) return "#f59e0b";
      return "#ef4444";
    }
    if (status.led_mode === "CpuLoad") {
      const refLoad = status.active_source === "GpuUsage" ? status.gpu_usage : status.cpu_usage;
      if (refLoad < 30) return "#06b6d4";
      if (refLoad < 70) return "#f59e0b";
      return "#ef4444";
    }
    return `rgb(${status.color[0]}, ${status.color[1]}, ${status.color[2]})`;
  };

  const ringColor = getRingColor();

  return (
    <div className="app">
      <header className="header">
        <div className="header-brand">
          <img src={thermalrightLogo} alt="Thermalright" className="brand-logo" />
          <span className="header-divider" />
          <span className="device-title">{status.device_name}</span>
        </div>

        <div className="header-actions">
          <div className={`connection-indicator ${status.connected ? "online" : "offline"}`}>
            <span className="indicator-dot" />
            <span>{status.connected ? "Connected" : "Searching device"}</span>
          </div>

          <button
            className={`power-button ${status.is_on ? "on" : ""}`}
            onClick={() => sendCommand("set_power", { on: !status.is_on })}
            title={status.is_on ? "Turn off display & lighting" : "Turn on display & lighting"}
            aria-label="Toggle cooler display power"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
              <line x1="12" y1="2" x2="12" y2="12" />
            </svg>
          </button>
        </div>
      </header>

      <div className="main-layout">
        <section className="stage">
          <div className="pump-showcase">
            <div className="pump-frame">
              <div className="pump-chassis" />

              <div
                className={`pump-ring ${status.is_on ? status.led_mode.toLowerCase() : "off"}`}
                style={{
                  opacity:
                    status.is_on && status.led_mode !== "Off"
                      ? Math.max(0.2, status.brightness)
                      : 0,
                  borderColor: status.led_mode === "Rainbow" ? "transparent" : ringColor,
                  boxShadow:
                    status.led_mode === "Rainbow"
                      ? "0 0 28px rgba(249, 115, 22, 0.3), inset 0 0 14px rgba(6, 182, 212, 0.3)"
                      : `0 0 24px ${ringColor}, inset 0 0 12px ${ringColor}`,
                  animationDuration:
                    status.led_mode === "Rainbow"
                      ? `${(4.5 / Math.max(0.2, status.speed)).toFixed(2)}s`
                      : status.led_mode === "Breathing"
                        ? `${(3.2 / Math.max(0.2, status.speed)).toFixed(2)}s`
                        : undefined,
                }}
              />

              <div className={`pump-display ${!status.is_on ? "dimmed" : ""}`}>
                <div className="display-temp">
                  <span className="temp-digits">
                    {!status.is_on
                      ? "--"
                      : status.test_mode
                        ? "88"
                        : activeReadout.value < 10
                          ? `0${activeReadout.value}`
                          : activeReadout.value}
                  </span>
                  <span className="temp-unit">
                    {status.test_mode ? `°${unitSymbol}` : activeReadout.unit}
                  </span>
                </div>
                <div className="display-load">
                  <span>{status.test_mode ? "LED TEST" : activeReadout.label}</span>
                </div>
              </div>
            </div>

            <div className="telemetry-strip">
              <div className="telemetry-item">
                <span className="telemetry-label">CPU temp</span>
                <span className="telemetry-value">
                  {cpuDisplayTemp}°{unitSymbol}
                </span>
              </div>
              <span className="telemetry-sep" />
              <div className="telemetry-item">
                <span className="telemetry-label">CPU load</span>
                <span className="telemetry-value">{Math.round(status.cpu_usage)}%</span>
              </div>
              <span className="telemetry-sep" />
              <div className="telemetry-item">
                <span className="telemetry-label">GPU temp</span>
                <span className="telemetry-value">
                  {gpuDisplayTemp}°{unitSymbol}
                </span>
              </div>
              <span className="telemetry-sep" />
              <div className="telemetry-item">
                <span className="telemetry-label">GPU load</span>
                <span className="telemetry-value">{Math.round(status.gpu_usage)}%</span>
              </div>
            </div>
          </div>

          <div className="unofficial-notice">
            <span>Unofficial open-source client (MIT)</span>
            <div className="notice-links">
              <button
                type="button"
                className="notice-link"
                onClick={() => openUrl(REPO_URL)}
                title={REPO_URL}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                  <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0 0 24 12c0-6.63-5.37-12-12-12z" />
                </svg>
                <span>GitHub</span>
              </button>
              <button
                type="button"
                className="notice-link"
                onClick={() => openUrl(WEBSITE_URL)}
                title={WEBSITE_URL}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <circle cx="12" cy="12" r="10" />
                  <line x1="2" y1="12" x2="22" y2="12" />
                  <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
                </svg>
                <span>emirman.dev</span>
              </button>
            </div>
          </div>
        </section>

        <aside className="controls">
          <nav className="tabs" aria-label="Control categories">
            <button
              className={`tab-btn ${activeTab === "display" ? "active" : ""}`}
              onClick={() => setActiveTab("display")}
            >
              Display
            </button>
            <button
              className={`tab-btn ${activeTab === "lighting" ? "active" : ""}`}
              onClick={() => setActiveTab("lighting")}
            >
              Lighting
            </button>
          </nav>

          {activeTab === "display" ? (
            <div className="controls-body">
              <div className="control-section">
                <h2 className="section-title">Display readout</h2>
                <div className="mode-grid">
                  {DISPLAY_SOURCES.map((source) => (
                    <button
                      key={source.id}
                      className={`mode-btn ${status.display_source === source.id ? "active" : ""}`}
                      onClick={() => sendCommand("set_display_source", { source: source.id })}
                    >
                      <span className="mode-name">{source.label}</span>
                      <span className="mode-hint">{source.hint}</span>
                    </button>
                  ))}
                </div>
              </div>

              {status.display_source === "AutoCycle" && (
                <div className="control-section">
                  <div className="section-row">
                    <h2 className="section-title">Cycle interval</h2>
                    <span className="section-value">{status.cycle_interval_secs}s</span>
                  </div>
                  <input
                    type="range"
                    min="1"
                    max="15"
                    step="1"
                    value={status.cycle_interval_secs}
                    onChange={(event) =>
                      sendCommand("set_cycle_interval", {
                        secs: parseInt(event.target.value, 10),
                      })
                    }
                    className="slider"
                    aria-label="Auto cycle interval in seconds"
                  />
                </div>
              )}

              <div className="control-section">
                <h2 className="section-title">Temperature unit</h2>
                <div className="unit-switch">
                  <button
                    className={`unit-btn ${status.temp_unit === "Celsius" ? "active" : ""}`}
                    onClick={() => sendCommand("set_temp_unit", { unit: "c" })}
                  >
                    Celsius (°C)
                  </button>
                  <button
                    className={`unit-btn ${status.temp_unit === "Fahrenheit" ? "active" : ""}`}
                    onClick={() => sendCommand("set_temp_unit", { unit: "f" })}
                  >
                    Fahrenheit (°F)
                  </button>
                </div>
              </div>

              <div className="control-section">
                <div className="section-row">
                  <h2 className="section-title">Segment test mode</h2>
                  <button
                    className={`test-toggle-btn ${status.test_mode ? "active" : ""}`}
                    onClick={() => sendCommand("set_test_mode", { enabled: !status.test_mode })}
                  >
                    {status.test_mode ? "Running (88)" : "Run test"}
                  </button>
                </div>
              </div>

              <div className="control-section">
                <div className="section-row">
                  <h2 className="section-title">Start on login</h2>
                  <button
                    className={`test-toggle-btn ${autostart ? "active" : ""}`}
                    onClick={toggleAutostart}
                    disabled={autostart === null}
                  >
                    {autostart ? "On (starts in tray)" : "Off"}
                  </button>
                </div>
              </div>

              <div className="control-section">
                <div className="section-row">
                  <div className="title-with-help">
                    <h2 className="section-title">Case lighting sync</h2>
                    <button
                      type="button"
                      className="help-btn"
                      aria-label="About case lighting sync"
                      aria-expanded={showCaseHelp}
                      aria-controls="case-sync-help"
                      onClick={() => setShowCaseHelp((prev) => !prev)}
                    >
                      ?
                    </button>
                  </div>
                  <button
                    className={`test-toggle-btn ${status.case_sync ? "active" : ""}`}
                    onClick={() => sendCommand("set_case_sync", { enabled: !status.case_sync })}
                  >
                    {status.case_sync ? "On" : "Off"}
                  </button>
                </div>
                {status.case_sync_status && <p className="control-hint">{status.case_sync_status}</p>}
                {showCaseHelp && (
                  <div id="case-sync-help" className="help-panel">
                    <p className="help-panel-title">Case lighting sync</p>
                    <p>Matches your case's motherboard-controlled lighting to the display, through OpenRGB.</p>
                    <ul>
                      <li>
                        Set your case lighting hub to <strong>M/B SYNC</strong> mode and connect it to the
                        motherboard's <strong>5V 3-pin ARGB</strong> header (not the 12V 4-pin RGB header).
                      </li>
                      <li>
                        Your motherboard must be supported by{" "}
                        <a
                          href="https://openrgb.org/devices.html"
                          onClick={(e) => {
                            e.preventDefault();
                            openUrl("https://openrgb.org/devices.html");
                          }}
                        >
                          OpenRGB
                        </a>{" "}
                        and offer a <strong>Direct</strong> mode. If it doesn't, the status line says so and
                        nothing is changed.
                      </li>
                      <li>OpenRGB is installed by the installer; Open TRCC starts and stops it for you.</li>
                    </ul>
                  </div>
                )}
              </div>

              <div className="control-section">
                <button
                  type="button"
                  className="advanced-toggle"
                  onClick={() => setShowAdvanced((prev) => !prev)}
                  aria-expanded={showAdvanced}
                >
                  <span>Advanced hardware details</span>
                  <span className="advanced-chevron">{showAdvanced ? "−" : "+"}</span>
                </button>
                {showAdvanced && (
                  <dl className="details-list">
                    <div className="details-row">
                      <dt>Device</dt>
                      <dd>{status.device_name}</dd>
                    </div>
                    <div className="details-row">
                      <dt>USB ID</dt>
                      <dd>{status.device_usb_id}</dd>
                    </div>
                    <div className="details-row">
                      <dt>Transport</dt>
                      <dd>USB HID (hidraw / WinHID)</dd>
                    </div>
                    <div className="details-row">
                      <dt>LED layout</dt>
                      <dd>14 digit + 1 unit + 15 ring</dd>
                    </div>
                  </dl>
                )}
              </div>
            </div>
          ) : (
            <div className="controls-body">
              <div className="control-section">
                <h2 className="section-title">Effect</h2>
                <div className="mode-grid">
                  {LIGHTING_MODES.map((mode) => (
                    <button
                      key={mode.id}
                      className={`mode-btn ${status.led_mode === mode.id ? "active" : ""}`}
                      onClick={() => sendCommand("set_led_mode", { mode: mode.id })}
                    >
                      <span className="mode-name">{mode.label}</span>
                      <span className="mode-hint">{mode.hint}</span>
                    </button>
                  ))}
                </div>
              </div>

              {status.led_mode !== "Rainbow" &&
                status.led_mode !== "ColorCycle" &&
                status.led_mode !== "Off" && (
                  <div className="control-section">
                    <div className="section-row">
                      <h2 className="section-title">Color</h2>
                      <span className="section-value">{currentHex.toUpperCase()}</span>
                    </div>
                    <div className="color-grid">
                      {PRESET_COLORS.map((preset) => {
                        const isSelected =
                          status.color[0] === preset.rgb[0] &&
                          status.color[1] === preset.rgb[1] &&
                          status.color[2] === preset.rgb[2];
                        return (
                          <button
                            key={preset.name}
                            className={`swatch ${isSelected ? "selected" : ""}`}
                            style={{ backgroundColor: preset.hex }}
                            onClick={() =>
                              sendCommand("set_led_color", {
                                r: preset.rgb[0],
                                g: preset.rgb[1],
                                b: preset.rgb[2],
                              })
                            }
                            title={preset.name}
                            aria-label={preset.name}
                          />
                        );
                      })}
                      <label className="swatch-custom" title="Custom color">
                        <input
                          type="color"
                          value={currentHex}
                          onChange={(event) => {
                            const [r, g, b] = hexToRgb(event.target.value);
                            sendCommand("set_led_color", { r, g, b });
                          }}
                        />
                        <span>+</span>
                      </label>
                    </div>
                    <div className="rgb-inputs">
                      {(["R", "G", "B"] as const).map((channel, index) => (
                        <label key={channel} className="rgb-field">
                          <span>{channel}</span>
                          <input
                            type="number"
                            min={0}
                            max={255}
                            value={status.color[index]}
                            onChange={(event) => {
                              const next = [...status.color] as [number, number, number];
                              const parsed = parseInt(event.target.value, 10);
                              next[index] = Number.isNaN(parsed)
                                ? 0
                                : Math.max(0, Math.min(255, parsed));
                              sendCommand("set_led_color", {
                                r: next[0],
                                g: next[1],
                                b: next[2],
                              });
                            }}
                          />
                        </label>
                      ))}
                    </div>
                  </div>
                )}

              <div className="control-section">
                <div className="section-row">
                  <h2 className="section-title">Brightness</h2>
                  <span className="section-value">{Math.round(status.brightness * 100)}%</span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={status.brightness}
                  onChange={(event) =>
                    sendCommand("set_brightness", { val: parseFloat(event.target.value) })
                  }
                  className="slider"
                  aria-label="LED brightness"
                />
              </div>

              {(status.led_mode === "Rainbow" ||
                status.led_mode === "ColorCycle" ||
                status.led_mode === "Breathing") && (
                <div className="control-section">
                  <div className="section-row">
                    <h2 className="section-title">Effect speed</h2>
                    <span className="section-value">{status.speed.toFixed(1)}x</span>
                  </div>
                  <input
                    type="range"
                    min="0.2"
                    max="3.0"
                    step="0.2"
                    value={status.speed}
                    onChange={(event) =>
                      sendCommand("set_speed", { val: parseFloat(event.target.value) })
                    }
                    className="slider"
                    aria-label="Effect speed"
                  />
                </div>
              )}
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}

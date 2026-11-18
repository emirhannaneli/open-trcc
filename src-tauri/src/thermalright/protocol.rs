use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedMode {
    Static,
    Breathing,
    ColorCycle,
    Rainbow,
    CpuTemp,
    CpuLoad,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TempUnit {
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplaySource {
    CpuTemp,
    CpuUsage,
    GpuTemp,
    GpuUsage,
    AutoCycle,
}

const DIGIT_SEGMENTS: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],      // 0
    [false, true, true, false, false, false, false],  // 1
    [true, true, false, true, true, false, true],      // 2
    [true, true, true, true, false, false, true],      // 3
    [false, true, true, false, false, true, true],     // 4
    [true, false, true, true, false, true, true],     // 5
    [true, false, true, true, true, true, true],      // 6
    [true, true, true, false, false, false, false],   // 7
    [true, true, true, true, true, true, true],       // 8
    [true, true, true, true, false, true, true],      // 9
];

pub struct PacketBuilder {
    pub mode: LedMode,
    pub color: (u8, u8, u8),
    pub brightness: f32,
    pub speed: f32,
    pub temp_unit: TempUnit,
    pub display_source: DisplaySource,
    pub cycle_interval_secs: u32,
    pub test_mode: bool,
}

impl Default for PacketBuilder {
    fn default() -> Self {
        Self {
            mode: LedMode::Rainbow,
            color: (255, 106, 0),
            brightness: 1.0,
            speed: 1.0,
            temp_unit: TempUnit::Celsius,
            display_source: DisplaySource::CpuTemp,
            cycle_interval_secs: 4,
            test_mode: false,
        }
    }
}

impl PacketBuilder {
    pub fn resolve_active_source(&self, tick: u64) -> DisplaySource {
        match self.display_source {
            DisplaySource::AutoCycle => {
                // tick runs every 500ms, so 2 ticks per second
                let ticks_per_slot = (self.cycle_interval_secs.max(1) as u64) * 2;
                match (tick / ticks_per_slot) % 4 {
                    0 => DisplaySource::CpuTemp,
                    1 => DisplaySource::GpuTemp,
                    2 => DisplaySource::CpuUsage,
                    _ => DisplaySource::GpuUsage,
                }
            }
            other => other,
        }
    }

    pub fn build_display_packet(
        &self,
        cpu_temp: f32,
        cpu_usage: f32,
        gpu_temp: f32,
        gpu_usage: f32,
        tick: u64,
    ) -> Vec<u8> {
        let mut packet = vec![0u8; 110];

        packet[0] = 0xda;
        packet[1] = 0xdb;
        packet[2] = 0xdc;
        packet[3] = 0xdd;
        packet[12] = 0x02;
        packet[16] = 90;

        if self.mode == LedMode::Off || self.brightness <= 0.001 {
            return packet;
        }

        let mut leds = [(0u8, 0u8, 0u8); 30];

        if self.test_mode {
            // Light up all segments ("88"), unit indicator, and ring at full brightness
            for led in leds.iter_mut() {
                *led = self.color;
            }
            for (i, &(r, g, b)) in leds.iter().enumerate() {
                packet[20 + i * 3] = r;
                packet[20 + i * 3 + 1] = g;
                packet[20 + i * 3 + 2] = b;
            }
            return packet;
        }

        let active_source = self.resolve_active_source(tick);
        let (display_value, is_temperature) = match active_source {
            DisplaySource::CpuTemp => (self.format_temp(cpu_temp), true),
            DisplaySource::GpuTemp => (self.format_temp(gpu_temp), true),
            DisplaySource::CpuUsage => ((cpu_usage.round() as u32).clamp(0, 99), false),
            DisplaySource::GpuUsage => ((gpu_usage.round() as u32).clamp(0, 99), false),
            DisplaySource::AutoCycle => unreachable!(),
        };

        let clamped = display_value.clamp(0, 999);
        let hundreds = (clamped / 100) as usize;
        let tens = ((clamped % 100) / 10) as usize;
        let ones = (clamped % 10) as usize;

        let speed = self.speed.clamp(0.2, 3.0);

        let current_color = match self.mode {
            LedMode::Static => self.color,
            LedMode::Breathing => {
                let phase = (tick as f32 * 0.1 * speed).sin().abs();
                let factor = 0.2 + 0.8 * phase;
                (
                    (self.color.0 as f32 * factor) as u8,
                    (self.color.1 as f32 * factor) as u8,
                    (self.color.2 as f32 * factor) as u8,
                )
            }
            LedMode::ColorCycle | LedMode::Rainbow => {
                let hue = ((tick as f32 * 4.0 * speed) % 360.0 + 360.0) % 360.0;
                hsv_to_rgb(hue, 1.0, 1.0)
            }
            LedMode::CpuTemp => {
                let ref_temp = if active_source == DisplaySource::GpuTemp {
                    gpu_temp
                } else {
                    cpu_temp
                };
                if ref_temp < 40.0 {
                    (0, 180, 255)
                } else if ref_temp < 65.0 {
                    (0, 255, 100)
                } else if ref_temp < 80.0 {
                    (255, 200, 0)
                } else {
                    (255, 30, 0)
                }
            }
            LedMode::CpuLoad => {
                let ref_load = if active_source == DisplaySource::GpuUsage {
                    gpu_usage
                } else {
                    cpu_usage
                };
                if ref_load < 30.0 {
                    (0, 255, 120)
                } else if ref_load < 70.0 {
                    (255, 180, 0)
                } else {
                    (255, 20, 20)
                }
            }
            LedMode::Off => (0, 0, 0),
        };

        let scaled_color = (
            (current_color.0 as f32 * self.brightness) as u8,
            (current_color.1 as f32 * self.brightness) as u8,
            (current_color.2 as f32 * self.brightness) as u8,
        );

        if tens > 0 || hundreds > 0 {
            let segs = DIGIT_SEGMENTS[tens];
            for i in 0..7 {
                if segs[i] {
                    leds[i] = scaled_color;
                }
            }
        }

        let segs_ones = DIGIT_SEGMENTS[ones];
        for i in 0..7 {
            if segs_ones[i] {
                leds[7 + i] = scaled_color;
            }
        }

        if is_temperature {
            leds[14] = scaled_color;
        }

        for (i, led) in leds.iter_mut().enumerate().skip(15) {
            if self.mode == LedMode::Rainbow {
                let offset = (i - 15) as f32 * (360.0 / 15.0);
                let hue = ((tick as f32 * 5.0 * speed + offset) % 360.0 + 360.0) % 360.0;
                *led = hsv_to_rgb(hue, 1.0, self.brightness);
            } else {
                *led = scaled_color;
            }
        }

        for (i, &(r, g, b)) in leds.iter().enumerate() {
            packet[20 + i * 3] = r;
            packet[20 + i * 3 + 1] = g;
            packet[20 + i * 3 + 2] = b;
        }

        packet
    }

    fn format_temp(&self, celsius: f32) -> u32 {
        match self.temp_unit {
            TempUnit::Celsius => celsius.round() as u32,
            TempUnit::Fahrenheit => ((celsius * 9.0 / 5.0) + 32.0).round() as u32,
        }
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

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

/// LED layout of the 30-LED digital display (TRCC "style 1": Frozen Horizon Pro,
/// Frozen Magic Pro, AX120 Digital), taken from TRCC's UCScreenLED/FormLED.
const LED_COUNT: usize = 30;
const LED_LOGO_1: usize = 0;
const LED_LOGO_2: usize = 1;
const LED_CPU_1: usize = 2;
const LED_CPU_2: usize = 3;
const LED_GPU_1: usize = 4;
const LED_GPU_2: usize = 5;
const LED_CELSIUS: usize = 6;
const LED_FAHRENHEIT: usize = 7;
const LED_PERCENT: usize = 8;
/// First LED (segment a) of each digit; segments follow in a..g order.
const LED_DIGIT_1: usize = 9;
const LED_DIGIT_2: usize = 16;
const LED_DIGIT_3: usize = 23;
/// Color group (0..10, left to right) of each LED, from TRCC's LedValToScreenLed.
/// Effects color groups, not single LEDs, so a digit's segments share a color.
const LED_GROUP: [usize; LED_COUNT] = [
    4, 4, // logo
    1, 2, // CPU
    6, 7, // GPU
    9, 9, 9, // °C °F %
    1, 2, 2, 1, 0, 0, 1, // digit 1 a..g
    4, 5, 5, 4, 3, 3, 4, // digit 2 a..g
    7, 8, 8, 7, 6, 6, 7, // digit 3 a..g
];
const LED_GROUPS: usize = 10;
/// TRCC's rainbow offsets neighbouring groups by 768/10/6 steps of a 768-step
/// hue wheel (~5.6°), giving a gentle gradient rather than a full spectrum.
const RAINBOW_GROUP_STEP_DEG: f32 = (768 / LED_GROUPS / 6) as f32 * 360.0 / 768.0;
/// TRCC multiplies every channel by 0.4 before sending; full 255 is far too bright.
const TRCC_OUTPUT_SCALE: f32 = 0.4;

/// One LED frame every 50 ms (20 per second); a send takes ~30 ms. The device loop ticks at this rate.
pub const FRAME_MS: u64 = 50;
/// TRCC steps its LED effects every 10th tick of a 15 ms form timer.
const TRCC_FRAME_MS: f32 = 150.0;
/// TRCC effect lengths, in its own frames: breathing ramps up and down over 66,
/// colour cycle crossfades 6 hue edges of 28 steps, rainbow moves 4 of 768 hue steps.
const BREATHING_PERIOD: f32 = 66.0;
const COLOR_CYCLE_DEG_PER_FRAME: f32 = 360.0 / (6.0 * 28.0);
const RAINBOW_DEG_PER_FRAME: f32 = 4.0 * 360.0 / 768.0;

/// Segments a..g for each digit.
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
    pub fn resolve_active_source(&self, frame: u64) -> DisplaySource {
        match self.display_source {
            DisplaySource::AutoCycle => {
                let frames_per_slot = self.cycle_interval_secs.max(1) as u64 * 1000 / FRAME_MS;
                match (frame / frames_per_slot) % 4 {
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
        frame: u64,
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

        let mut leds = [(0u8, 0u8, 0u8); LED_COUNT];

        if self.test_mode {
            // Light every LED (logo, labels, units, "888") in the chosen color.
            let c = scale(self.color, self.brightness * TRCC_OUTPUT_SCALE);
            leds.fill(c);
            write_leds(&mut packet, &leds);
            return packet;
        }

        let active_source = self.resolve_active_source(frame);
        let (display_value, is_temperature, is_gpu) = match active_source {
            DisplaySource::CpuTemp => (self.format_temp(cpu_temp), true, false),
            DisplaySource::GpuTemp => (self.format_temp(gpu_temp), true, true),
            DisplaySource::CpuUsage => ((cpu_usage.round().max(0.0) as u32).min(100), false, false),
            DisplaySource::GpuUsage => ((gpu_usage.round().max(0.0) as u32).min(100), false, true),
            DisplaySource::AutoCycle => unreachable!(),
        };

        // Which LEDs are lit, mirroring TRCC's FormLED.GetVal for the 30-LED layout.
        let mut on = [false; LED_COUNT];
        on[LED_LOGO_1] = true;
        on[LED_LOGO_2] = true;
        if is_gpu {
            on[LED_GPU_1] = true;
            on[LED_GPU_2] = true;
        } else {
            on[LED_CPU_1] = true;
            on[LED_CPU_2] = true;
        }
        if is_temperature {
            match self.temp_unit {
                TempUnit::Celsius => on[LED_CELSIUS] = true,
                TempUnit::Fahrenheit => on[LED_FAHRENHEIT] = true,
            }
        } else {
            on[LED_PERCENT] = true;
        }

        let clamped = display_value.min(199) as usize;
        let hundreds = clamped / 100;
        let tens = (clamped % 100) / 10;
        let ones = clamped % 10;
        if hundreds > 0 {
            set_digit(&mut on, LED_DIGIT_1, hundreds);
        }
        if hundreds > 0 || tens > 0 {
            set_digit(&mut on, LED_DIGIT_2, tens);
        }
        set_digit(&mut on, LED_DIGIT_3, ones);

        // Effect time in TRCC frames, so speed 1.0 matches the Windows app.
        let t = frame as f32 * FRAME_MS as f32 / TRCC_FRAME_MS * self.speed.clamp(0.2, 3.0);
        let base_color = match self.mode {
            LedMode::Static | LedMode::Rainbow => self.color,
            LedMode::Breathing => {
                // Linear ramp up then down, never below 20% (TRCC's DSHX effect).
                let phase = (t % BREATHING_PERIOD) / (BREATHING_PERIOD / 2.0);
                let factor = 0.2 + 0.8 * if phase < 1.0 { phase } else { 2.0 - phase };
                (
                    (self.color.0 as f32 * factor) as u8,
                    (self.color.1 as f32 * factor) as u8,
                    (self.color.2 as f32 * factor) as u8,
                )
            }
            LedMode::ColorCycle => {
                let hue = (t * COLOR_CYCLE_DEG_PER_FRAME).rem_euclid(360.0);
                hsv_to_rgb(hue, 1.0, 1.0)
            }
            LedMode::CpuTemp => {
                let ref_temp = if is_gpu { gpu_temp } else { cpu_temp };
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
                let ref_load = if is_gpu { gpu_usage } else { cpu_usage };
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

        let level = self.brightness * TRCC_OUTPUT_SCALE;
        for (i, led) in leds.iter_mut().enumerate() {
            if !on[i] {
                continue;
            }
            let color = if self.mode == LedMode::Rainbow {
                let offset = (LED_GROUPS - 1 - LED_GROUP[i]) as f32 * RAINBOW_GROUP_STEP_DEG;
                hsv_to_rgb((t * RAINBOW_DEG_PER_FRAME + offset).rem_euclid(360.0), 1.0, 1.0)
            } else {
                base_color
            };
            *led = scale(color, level);
        }

        write_leds(&mut packet, &leds);
        packet
    }

    fn format_temp(&self, celsius: f32) -> u32 {
        match self.temp_unit {
            TempUnit::Celsius => celsius.round() as u32,
            TempUnit::Fahrenheit => ((celsius * 9.0 / 5.0) + 32.0).round() as u32,
        }
    }
}

fn set_digit(on: &mut [bool; LED_COUNT], first: usize, digit: usize) {
    for (seg, &lit) in DIGIT_SEGMENTS[digit].iter().enumerate() {
        on[first + seg] = lit;
    }
}

fn scale(color: (u8, u8, u8), level: f32) -> (u8, u8, u8) {
    let level = level.clamp(0.0, 1.0);
    (
        (color.0 as f32 * level) as u8,
        (color.1 as f32 * level) as u8,
        (color.2 as f32 * level) as u8,
    )
}

fn write_leds(packet: &mut [u8], leds: &[(u8, u8, u8); LED_COUNT]) {
    for (i, &(r, g, b)) in leds.iter().enumerate() {
        packet[20 + i * 3] = r;
        packet[20 + i * 3 + 1] = g;
        packet[20 + i * 3 + 2] = b;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(packet: &[u8]) -> Vec<usize> {
        (0..LED_COUNT)
            .filter(|i| packet[20 + i * 3..23 + i * 3].iter().any(|&c| c > 0))
            .collect()
    }

    #[test]
    fn cpu_temp_45c_lights_only_expected_leds() {
        let b = PacketBuilder { mode: LedMode::Static, ..Default::default() };
        let p = b.build_display_packet(45.0, 10.0, 0.0, 0.0, 0);
        assert_eq!(p.len(), 110);
        assert_eq!(&p[..4], &[0xda, 0xdb, 0xdc, 0xdd]);
        // logo, CPU label, °C, "4" (b c f g) in digit 2, "5" (a c d f g) in digit 3
        let expected = vec![0, 1, 2, 3, 6, 17, 18, 21, 22, 23, 25, 26, 28, 29];
        assert_eq!(lit(&p), expected);
        // TRCC scales output to 40%
        assert_eq!(p[20], (255.0 * 0.4) as u8);
    }

    #[test]
    fn gpu_usage_shows_percent_and_gpu_label() {
        let b = PacketBuilder {
            mode: LedMode::Static,
            display_source: DisplaySource::GpuUsage,
            ..Default::default()
        };
        let on = lit(&b.build_display_packet(0.0, 0.0, 0.0, 7.0, 0));
        assert!(on.contains(&4) && on.contains(&5) && on.contains(&8));
        assert!(!on.contains(&2) && !on.contains(&6));
        assert!(on.iter().all(|&i| i < 16 || i >= 23)); // no leading digits
    }

    #[test]
    fn static_mode_uses_one_color_everywhere() {
        let b = PacketBuilder { mode: LedMode::Static, color: (10, 200, 90), ..Default::default() };
        let p = b.build_display_packet(88.0, 0.0, 0.0, 0.0, 3);
        let colors: std::collections::HashSet<_> =
            lit(&p).iter().map(|&i| (p[20 + i * 3], p[21 + i * 3], p[22 + i * 3])).collect();
        assert_eq!(colors.len(), 1);
    }

    #[test]
    fn rainbow_segments_of_a_group_match_and_spread_is_small() {
        let b = PacketBuilder::default(); // Rainbow
        let p = b.build_display_packet(88.0, 0.0, 0.0, 0.0, 0);
        let c = |i: usize| (p[20 + i * 3], p[21 + i * 3], p[22 + i * 3]);
        // digit 2 segment a and d are both group 4
        assert_eq!(c(16), c(19));
        // whole display spans < 60° of hue
        assert!(9.0 * RAINBOW_GROUP_STEP_DEG < 60.0);
    }

    fn first_led(b: &PacketBuilder, frame: u64) -> (u8, u8, u8) {
        let p = b.build_display_packet(88.0, 0.0, 0.0, 0.0, frame);
        (p[20], p[21], p[22])
    }

    #[test]
    fn effects_change_gradually_between_frames() {
        for mode in [LedMode::Rainbow, LedMode::ColorCycle, LedMode::Breathing] {
            let b = PacketBuilder { mode, color: (255, 0, 0), ..Default::default() };
            let max_step = (0..400)
                .map(|f| {
                    let (a, c) = (first_led(&b, f), first_led(&b, f + 1));
                    a.0.abs_diff(c.0).max(a.1.abs_diff(c.1)).max(a.2.abs_diff(c.2))
                })
                .max()
                .unwrap();
            // Full scale is 102 after TRCC's 40% output; a few counts per frame reads as smooth.
            assert!(max_step <= 4, "{:?} jumps {} per frame", mode, max_step);
        }
    }

    #[test]
    fn breathing_matches_trcc_period_and_floor() {
        let b = PacketBuilder { mode: LedMode::Breathing, color: (250, 0, 0), ..Default::default() };
        assert_eq!(first_led(&b, 0).0, (250.0 * 0.2 * 0.4) as u8);
        // Peak at half of 66 TRCC frames (150 ms each) = 99 of ours.
        assert!(first_led(&b, 99).0 >= 98);
    }
}

#[cfg(test)]
mod power_tests {
    use super::*;

    #[test]
    fn off_mode_sends_all_black_frame() {
        let b = PacketBuilder { mode: LedMode::Off, test_mode: true, ..Default::default() };
        let p = b.build_display_packet(55.0, 0.0, 0.0, 0.0, 0);
        assert_eq!(p.len(), 110);
        assert_eq!(&p[..4], &[0xda, 0xdb, 0xdc, 0xdd]);
        assert!(p[20..].iter().all(|&c| c == 0));
    }
}

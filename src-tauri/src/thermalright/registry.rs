use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceTransport {
    /// 7-segment LED display driven with the DA DB DC DD packet (TRCC FormLED).
    HidDigital,
    /// LCD screen over HID (TRCC FormLCD); not driven yet.
    HidLcd,
    /// LCD screen over USB bulk; not driven yet.
    BulkLcd,
    /// Enumerated by TRCC but never driven by it.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceProfile {
    FrozenMagicDigital,
    FrozenHorizonDigital,
    AirCoolerDigital,
    VisionLcd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDeviceSpec {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub transport: DeviceTransport,
    pub default_profile: DeviceProfile,
}

pub const SUPPORTED_DEVICES: &[KnownDeviceSpec] = &[
    KnownDeviceSpec {
        vid: 0x0416,
        pid: 0x8001,
        name: "Thermalright Digital LED (0416:8001)",
        transport: DeviceTransport::HidDigital,
        default_profile: DeviceProfile::FrozenMagicDigital,
    },
    KnownDeviceSpec {
        vid: 0x0418,
        pid: 0x5303,
        name: "Thermalright HID device (0418:5303, unsupported)",
        transport: DeviceTransport::Unsupported,
        default_profile: DeviceProfile::AirCoolerDigital,
    },
    KnownDeviceSpec {
        vid: 0x0418,
        pid: 0x5304,
        name: "Thermalright HID LCD (0418:5304)",
        transport: DeviceTransport::HidLcd,
        default_profile: DeviceProfile::AirCoolerDigital,
    },
    KnownDeviceSpec {
        vid: 0x87ad,
        pid: 0x70db,
        name: "Thermalright Vision LCD (87ad:70db)",
        transport: DeviceTransport::BulkLcd,
        default_profile: DeviceProfile::VisionLcd,
    },
    KnownDeviceSpec {
        vid: 0x0416,
        pid: 0x5302,
        name: "Thermalright Vision LCD (0416:5302)",
        transport: DeviceTransport::BulkLcd,
        default_profile: DeviceProfile::VisionLcd,
    },
    KnownDeviceSpec {
        vid: 0x0416,
        pid: 0x5406,
        name: "Thermalright Vision LCD (0416:5406)",
        transport: DeviceTransport::BulkLcd,
        default_profile: DeviceProfile::VisionLcd,
    },
    KnownDeviceSpec {
        vid: 0x0416,
        pid: 0x5408,
        name: "Thermalright Vision LCD (0416:5408)",
        transport: DeviceTransport::BulkLcd,
        default_profile: DeviceProfile::VisionLcd,
    },
    KnownDeviceSpec {
        vid: 0x0416,
        pid: 0x5409,
        name: "Thermalright Vision LCD (0416:5409)",
        transport: DeviceTransport::BulkLcd,
        default_profile: DeviceProfile::VisionLcd,
    },
];

pub fn lookup_device(vid: u16, pid: u16) -> Option<&'static KnownDeviceSpec> {
    SUPPORTED_DEVICES
        .iter()
        .find(|spec| spec.vid == vid && spec.pid == pid)
}

/// TRCC model numbers (init-handshake reply) that use the 30-LED layout in
/// protocol.rs: 1 Frozen Horizon Pro, 2 Frozen Magic Pro, 3 AX120 Digital.
/// Others (PA120 16-31, AK120 32, LF8 48/49, ...) have different layouts.
pub fn is_supported_led_style(style: u8) -> bool {
    matches!(style, 1..=3)
}

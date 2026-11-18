use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceTransport {
    HidDigital,
    BulkLcd,
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
        name: "Thermalright Digital HID (0418:5303)",
        transport: DeviceTransport::HidDigital,
        default_profile: DeviceProfile::AirCoolerDigital,
    },
    KnownDeviceSpec {
        vid: 0x0418,
        pid: 0x5304,
        name: "Thermalright Digital HID (0418:5304)",
        transport: DeviceTransport::HidDigital,
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

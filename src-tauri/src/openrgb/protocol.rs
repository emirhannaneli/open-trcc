//! OpenRGB SDK wire format: packet encoding and controller data parsing.

pub const PROTOCOL_VERSION: u32 = 3;

pub const REQUEST_CONTROLLER_COUNT: u32 = 0;
pub const REQUEST_CONTROLLER_DATA: u32 = 1;
pub const REQUEST_PROTOCOL_VERSION: u32 = 40;
pub const SET_CLIENT_NAME: u32 = 50;
pub const DEVICE_LIST_UPDATED: u32 = 100;
pub const RESIZEZONE: u32 = 1000;
pub const UPDATELEDS: u32 = 1050;
pub const UPDATEMODE: u32 = 1101;

/// Size of the packet header: magic, device index, packet id, payload length.
pub const HEADER_LEN: usize = 16;

pub fn encode(dev: u32, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(b"ORGB");
    out.extend_from_slice(&dev.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[derive(Debug, Clone)]
pub struct Controller {
    pub kind: i32,
    pub name: String,
    pub modes: Vec<Mode>,
    pub zones: Vec<Zone>,
    pub led_count: usize,
}

#[derive(Debug, Clone)]
pub struct Mode {
    pub name: String,
    /// The mode's exact serialized bytes, re-sent verbatim by UPDATEMODE.
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub leds_min: u32,
    pub leds_max: u32,
    pub leds_count: u32,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| format!("controller data truncated at byte {}", self.pos))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    /// `u16 len` (including the NUL) followed by that many bytes.
    fn string(&mut self) -> Result<String, String> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

pub fn parse_controller(data: &[u8], version: u32) -> Result<Controller, String> {
    if version < PROTOCOL_VERSION {
        return Err(format!("unsupported protocol version {version}"));
    }
    let mut r = Reader { data, pos: 0 };
    let _size = r.u32()?;
    let kind = r.i32()?;
    let name = r.string()?;
    for _ in 0..5 {
        // vendor, description, version, serial, location
        r.string()?;
    }
    let num_modes = r.u16()?;
    let _active_mode = r.i32()?;

    let mut modes = Vec::new();
    for _ in 0..num_modes {
        let start = r.pos;
        let mode_name = r.string()?;
        r.take(4 + 4 + 10 * 4)?; // value, flags, speed/brightness/colors/direction fields
        let num_colors = r.u16()? as usize;
        r.take(num_colors * 4)?;
        modes.push(Mode {
            name: mode_name,
            raw: data[start..r.pos].to_vec(),
        });
    }

    let num_zones = r.u16()?;
    let mut zones = Vec::new();
    for _ in 0..num_zones {
        r.string()?;
        r.i32()?; // zone type
        let leds_min = r.u32()?;
        let leds_max = r.u32()?;
        let leds_count = r.u32()?;
        let matrix_len = r.u16()? as usize;
        r.take(matrix_len)?;
        zones.push(Zone {
            leds_min,
            leds_max,
            leds_count,
        });
    }

    let led_count = r.u16()? as usize;
    for _ in 0..led_count {
        r.string()?;
        r.u32()?;
    }
    let num_colors = r.u16()? as usize;
    r.take(num_colors * 4)?;

    Ok(Controller {
        kind,
        name,
        modes,
        zones,
        led_count,
    })
}

pub fn update_mode_payload(mode_index: i32, mode: &Mode) -> Vec<u8> {
    let size = (4 + 4 + mode.raw.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&mode_index.to_le_bytes());
    out.extend_from_slice(&mode.raw);
    out
}

pub fn resize_zone_payload(zone: i32, size: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&zone.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out
}

pub fn update_leds_payload(count: usize, rgb: (u8, u8, u8)) -> Vec<u8> {
    let size = (4 + 2 + count * 4) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&(count as u16).to_le_bytes());
    for _ in 0..count {
        out.extend_from_slice(&[rgb.0, rgb.1, rgb.2, 0]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_captured_mystic_light() {
        let c = parse_controller(include_bytes!("fixtures/mystic_light_v3.bin"), 3).unwrap();
        assert_eq!(c.kind, 0);
        assert_eq!(c.name, "MSI B550M PRO-VDH (MS-7C95)");
        assert!(c.modes.iter().any(|m| m.name == "Direct"));
        let zones: Vec<_> = c.zones.iter().map(|z| z.leds_max).collect();
        assert_eq!(zones.len(), 5); // JRGB1, JRGB2, JRAINBOW1, JRAINBOW2, ONBOARD
    }

    #[test]
    fn truncated_controller_is_an_error() {
        let d = include_bytes!("fixtures/mystic_light_v3.bin");
        for cut in [0, 10, d.len() / 2, d.len() - 1] {
            assert!(parse_controller(&d[..cut], 3).is_err());
        }
    }

    #[test]
    fn mode_raw_round_trips_into_update_payload() {
        let c = parse_controller(include_bytes!("fixtures/mystic_light_v3.bin"), 3).unwrap();
        let m = c.modes.iter().find(|m| m.name == "Direct").unwrap();
        let p = update_mode_payload(0, m);
        assert_eq!(u32::from_le_bytes(p[..4].try_into().unwrap()) as usize, p.len());
        assert_eq!(&p[8..], &m.raw[..]);
    }

    #[test]
    fn update_leds_payload_layout() {
        assert_eq!(
            update_leds_payload(2, (1, 2, 3)),
            vec![14, 0, 0, 0, 2, 0, 1, 2, 3, 0, 1, 2, 3, 0]
        );
    }

    #[test]
    fn header_layout() {
        assert_eq!(
            encode(5, 1050, &[9]),
            b"ORGB\x05\0\0\0\x1a\x04\0\0\x01\0\0\0\x09".to_vec()
        );
    }
}

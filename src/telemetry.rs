//! Pure decoder for the Assetto Corsa built-in UDP telemetry protocol.
//!
//! Pure decoder and request encoder for the Assetto Corsa built-in UDP telemetry
//! protocol. No network code lives here.
//!
//! The wire structs are laid out like C structs with natural alignment (4-byte
//! `int`/`float`, 1-byte `char`/`bool`), and integers are little-endian.
//!
//! The packets carry no type tag. Packets are told apart by their exact byte length:
//!   * 408 bytes: handshake response
//!   * 328 bytes: car update
//!   * 212 bytes: lap completed
//!
//! Name fields are fixed `wchar_t[50]` arrays (100 bytes of UTF-16LE). The game
//! pads them with `%` and NUL characters, so a name ends at the first `%` or NUL.

use std::fmt;

/// Length of every client request: three little-endian `i32` values.
pub const REQUEST_LEN: usize = 12;
/// Handshake request length (same as [`REQUEST_LEN`]).
pub const HANDSHAKE_REQUEST_LEN: usize = REQUEST_LEN;
/// Handshake response length.
pub const HANDSHAKE_RESPONSE_LEN: usize = 408;
/// Car update length.
pub const CAR_UPDATE_LEN: usize = 328;
/// Lap completed length.
pub const LAP_LEN: usize = 212;

/// Number of UTF-16 code units in each fixed name field.
const NAME_CHARS: usize = 50;
/// Length in bytes of each fixed name field.
const NAME_LEN: usize = NAME_CHARS * 2;

/// Minimum lap time accepted by [`summarize_laps`], in milliseconds.
const MIN_PLAUSIBLE_LAP_MS: u32 = 10_000;

// Request header: identifier and protocol version are always 1.
const REQUEST_IDENTIFIER: i32 = 1;
const REQUEST_VERSION: i32 = 1;

/// Operation code: handshake.
pub const OP_HANDSHAKE: i32 = 0;
/// Operation code: subscribe to car updates pushed by the game.
pub const OP_SUBSCRIBE_UPDATE: i32 = 1;
/// Operation code: subscribe to lap-completed events only.
pub const OP_SUBSCRIBE_SPOT: i32 = 2;
/// Operation code: end the telemetry session.
pub const OP_DISMISS: i32 = 3;

// Handshake response offsets.
const HS_CAR_NAME: usize = 0; // wchar_t[50]
const HS_DRIVER_NAME: usize = 100; // wchar_t[50]
// int identifier at 200, int version at 204.
const HS_TRACK_NAME: usize = 208; // wchar_t[50]
const HS_TRACK_CONFIG: usize = 308; // wchar_t[50]

// Car update offsets (natural alignment).
const CAR_SPEED_KMH: usize = 8; // float speed_Kmh
const CAR_LAP_TIME: usize = 40; // int lapTime (milliseconds)
const CAR_ENGINE_RPM: usize = 68; // float engineRPM
const CAR_GEAR: usize = 76; // int gear
const CAR_COORDINATES: usize = 316; // float carCoordinates[3]
// Driver inputs and g-forces. The six 1-byte flags before them end at byte 26,
// so the first float after them lands on the next 4-byte boundary, 28.
const CAR_G_VERTICAL: usize = 28; // float: up/down acceleration in g
const CAR_G_LATERAL: usize = 32; // float: sideways acceleration in g
const CAR_G_LONGITUDINAL: usize = 36; // float: forward/back acceleration in g
const CAR_THROTTLE: usize = 56; // float: throttle pedal, 0.0 to 1.0
const CAR_BRAKE: usize = 60; // float: brake pedal, 0.0 to 1.0
const CAR_STEER_ANGLE: usize = 72; // float: steering wheel angle, unit not stated

// Lap completed offsets.
const LAP_CAR_ID: usize = 0; // int carIdentifierNumber
const LAP_LAP_COUNT: usize = 4; // int lap
const LAP_DRIVER_NAME: usize = 8; // wchar_t[50]
const LAP_CAR_NAME: usize = 108; // wchar_t[50]
const LAP_TIME: usize = 208; // int time (milliseconds)

/// A decoded telemetry packet.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    Handshake(HandshakeInfo),
    CarUpdate(CarUpdate),
    LapCompleted(LapInfo),
    /// Reserved for recognised packet codes the decoder does not model.
    /// [`decode`] does not currently produce this variant.
    Other(u8),
}

/// Session information returned by the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeInfo {
    pub car: String,
    pub driver: String,
    pub track: String,
    pub track_config: String,
}

/// Per-car telemetry sample from a car update packet.
#[derive(Debug, Clone, PartialEq)]
pub struct CarUpdate {
    pub speed_kmh: f32,
    /// Raw gear value as sent by the game (`int gear`), clamped to `i8`.
    /// It is not remapped, so the meaning of each value is up to the caller.
    pub gear: i8,
    /// Engine speed from `engineRPM`, rounded and clamped to `u16`.
    pub rpm: u16,
    /// World coordinates from `carCoordinates[3]`.
    pub position: (f32, f32, f32),
    /// Current lap time from `lapTime`, in milliseconds. Negative values map to 0.
    pub lap_time_ms: u32,
    /// Throttle pedal position, 0.0 released to 1.0 fully pressed, as sent.
    pub throttle: f32,
    /// Brake pedal position, 0.0 released to 1.0 fully pressed, as sent.
    pub brake: f32,
    /// Steering wheel angle as the game sends it. The protocol does not name a unit.
    pub steer_angle: f32,
    /// Sideways acceleration in g. The sign follows the game.
    pub g_lateral: f32,
    /// Acceleration along the car's length in g. The sign follows the game.
    pub g_longitudinal: f32,
    /// Vertical acceleration in g. Close to 1.0 when the car sits level.
    pub g_vertical: f32,
}

/// A completed lap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LapInfo {
    /// `carIdentifierNumber`, clamped to `u8`.
    pub car_id: u8,
    /// `time` in milliseconds. Negative values map to 0.
    pub lap_time_ms: u32,
    /// `lap` (lap count), clamped to `u16`.
    pub laps: u16,
    /// `driverName`, lossy UTF-16 up to the first `%` or NUL.
    pub driver: String,
    /// `carName`, lossy UTF-16 up to the first `%` or NUL.
    pub car: String,
}

/// Errors returned by [`decode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The input is too short for the packet it claims to be, or empty.
    Truncated,
    /// The input length matches no known packet, and the first byte is kept.
    UnknownType(u8),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "telemetry packet is truncated"),
            DecodeError::UnknownType(code) => {
                write!(f, "unknown telemetry packet type 0x{code:02x}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes one UDP datagram from Assetto Corsa.
///
/// Dispatch is by exact length (see the module docs). Every read is
/// bounds-checked, so malformed input returns an error and never panics.
pub fn decode(bytes: &[u8]) -> Result<Packet, DecodeError> {
    if bytes.is_empty() {
        return Err(DecodeError::Truncated);
    }
    match bytes.len() {
        HANDSHAKE_RESPONSE_LEN => decode_handshake(bytes).map(Packet::Handshake),
        CAR_UPDATE_LEN => decode_car_update(bytes).map(Packet::CarUpdate),
        LAP_LEN => decode_lap(bytes).map(Packet::LapCompleted),
        // Shorter than the largest known packet but not an exact match:
        // treat as truncated.
        n if n < HANDSHAKE_RESPONSE_LEN => Err(DecodeError::Truncated),
        _ => Err(DecodeError::UnknownType(bytes[0])),
    }
}

/// Builds a 12-byte client request: identifier 1, version 1, then `operation`,
/// all little-endian. See the `OP_*` constants for the operation codes.
pub fn encode_request(operation: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(REQUEST_LEN);
    out.extend_from_slice(&REQUEST_IDENTIFIER.to_le_bytes());
    out.extend_from_slice(&REQUEST_VERSION.to_le_bytes());
    out.extend_from_slice(&operation.to_le_bytes());
    out
}

/// Builds the 12-byte handshake request (operation [`OP_HANDSHAKE`]).
pub fn encode_handshake_request() -> Vec<u8> {
    encode_request(OP_HANDSHAKE)
}

/// Summary statistics over a set of completed laps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LapSummary {
    /// Fastest valid lap, in milliseconds.
    pub best_ms: Option<u32>,
    /// Mean of the valid laps, rounded to the nearest millisecond.
    pub average_ms: Option<u32>,
    /// Number of valid laps.
    pub count: usize,
}

/// Summarises laps, ignoring zero and implausible times (below 10 seconds).
pub fn summarize_laps(laps: &[LapInfo]) -> LapSummary {
    let valid: Vec<u32> = laps
        .iter()
        .map(|l| l.lap_time_ms)
        .filter(|&ms| ms >= MIN_PLAUSIBLE_LAP_MS)
        .collect();
    if valid.is_empty() {
        return LapSummary::default();
    }
    let count = valid.len();
    let best_ms = valid.iter().copied().min();
    let sum: u64 = valid.iter().map(|&ms| u64::from(ms)).sum();
    let count64 = count as u64;
    let average = (sum + count64 / 2) / count64;
    LapSummary {
        best_ms,
        average_ms: Some(average as u32),
        count,
    }
}

// ---- field readers (all bounds-checked) ----

fn bytes_at(b: &[u8], off: usize, len: usize) -> Result<&[u8], DecodeError> {
    off.checked_add(len)
        .and_then(|end| b.get(off..end))
        .ok_or(DecodeError::Truncated)
}

fn i32_at(b: &[u8], off: usize) -> Result<i32, DecodeError> {
    let s = bytes_at(b, off, 4)?;
    Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f32_at(b: &[u8], off: usize) -> Result<f32, DecodeError> {
    let s = bytes_at(b, off, 4)?;
    Ok(f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Reads a fixed `wchar_t[50]` name field as lossy UTF-16LE, stopping at the
/// first `%` or NUL, which the game uses as padding.
fn name_at(b: &[u8], off: usize) -> Result<String, DecodeError> {
    let field = bytes_at(b, off, NAME_LEN)?;
    let units: Vec<u16> = field
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&unit| unit != 0 && unit != u16::from(b'%'))
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

fn clamp_i32_to_u32(v: i32) -> u32 {
    v.max(0) as u32
}

fn f32_to_u16(v: f32) -> u16 {
    if !v.is_finite() || v <= 0.0 {
        0
    } else {
        v.round().min(f32::from(u16::MAX)) as u16
    }
}

fn decode_handshake(b: &[u8]) -> Result<HandshakeInfo, DecodeError> {
    Ok(HandshakeInfo {
        car: name_at(b, HS_CAR_NAME)?,
        driver: name_at(b, HS_DRIVER_NAME)?,
        track: name_at(b, HS_TRACK_NAME)?,
        track_config: name_at(b, HS_TRACK_CONFIG)?,
    })
}

fn decode_car_update(b: &[u8]) -> Result<CarUpdate, DecodeError> {
    let gear = i32_at(b, CAR_GEAR)?.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8;
    Ok(CarUpdate {
        speed_kmh: f32_at(b, CAR_SPEED_KMH)?,
        gear,
        rpm: f32_to_u16(f32_at(b, CAR_ENGINE_RPM)?),
        position: (
            f32_at(b, CAR_COORDINATES)?,
            f32_at(b, CAR_COORDINATES + 4)?,
            f32_at(b, CAR_COORDINATES + 8)?,
        ),
        lap_time_ms: clamp_i32_to_u32(i32_at(b, CAR_LAP_TIME)?),
        throttle: f32_at(b, CAR_THROTTLE)?,
        brake: f32_at(b, CAR_BRAKE)?,
        steer_angle: f32_at(b, CAR_STEER_ANGLE)?,
        g_lateral: f32_at(b, CAR_G_LATERAL)?,
        g_longitudinal: f32_at(b, CAR_G_LONGITUDINAL)?,
        g_vertical: f32_at(b, CAR_G_VERTICAL)?,
    })
}

fn decode_lap(b: &[u8]) -> Result<LapInfo, DecodeError> {
    let car_id = i32_at(b, LAP_CAR_ID)?.clamp(0, i32::from(u8::MAX)) as u8;
    let laps = i32_at(b, LAP_LAP_COUNT)?.clamp(0, i32::from(u16::MAX)) as u16;
    Ok(LapInfo {
        car_id,
        lap_time_ms: clamp_i32_to_u32(i32_at(b, LAP_TIME)?),
        laps,
        driver: name_at(b, LAP_DRIVER_NAME)?,
        car: name_at(b, LAP_CAR_NAME)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- synthetic packet builders (little-endian, documented offsets) ----

    fn put_i32(buf: &mut [u8], off: usize, v: i32) {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_f32(buf: &mut [u8], off: usize, v: f32) {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Writes `s` as UTF-16LE padded with `%`, the way the game fills name fields.
    fn put_name(buf: &mut [u8], off: usize, s: &str) {
        let mut units: Vec<u16> = s.encode_utf16().take(NAME_CHARS).collect();
        units.resize(NAME_CHARS, u16::from(b'%'));
        for (i, unit) in units.iter().enumerate() {
            buf[off + i * 2..off + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn handshake_bytes() -> Vec<u8> {
        let mut b = vec![0u8; HANDSHAKE_RESPONSE_LEN];
        put_name(&mut b, HS_CAR_NAME, "ks_ferrari_488_gt3");
        put_name(&mut b, HS_DRIVER_NAME, "Leon");
        put_i32(&mut b, 200, 1); // identifier
        put_i32(&mut b, 204, 1); // version
        put_name(&mut b, HS_TRACK_NAME, "monza");
        put_name(&mut b, HS_TRACK_CONFIG, "gp");
        b
    }

    fn car_bytes() -> Vec<u8> {
        let mut b = vec![0u8; CAR_UPDATE_LEN];
        b[0] = b'a';
        put_i32(&mut b, 4, CAR_UPDATE_LEN as i32); // size
        put_f32(&mut b, CAR_SPEED_KMH, 187.5);
        put_i32(&mut b, CAR_LAP_TIME, 61_234);
        put_f32(&mut b, CAR_ENGINE_RPM, 7412.6);
        put_i32(&mut b, CAR_GEAR, 4);
        put_f32(&mut b, CAR_COORDINATES, 10.5);
        put_f32(&mut b, CAR_COORDINATES + 4, -2.25);
        put_f32(&mut b, CAR_COORDINATES + 8, 300.0);
        b
    }

    fn lap_bytes() -> Vec<u8> {
        let mut b = vec![0u8; LAP_LEN];
        put_i32(&mut b, LAP_CAR_ID, 3);
        put_i32(&mut b, LAP_LAP_COUNT, 5);
        put_name(&mut b, LAP_DRIVER_NAME, "Max");
        put_name(&mut b, LAP_CAR_NAME, "ks_mazda_mx5");
        put_i32(&mut b, LAP_TIME, 92_500);
        b
    }

    fn lap(ms: u32) -> LapInfo {
        LapInfo {
            car_id: 0,
            lap_time_ms: ms,
            laps: 1,
            driver: String::new(),
            car: String::new(),
        }
    }

    // ---- decode tests ----

    #[test]
    fn decodes_handshake() {
        match decode(&handshake_bytes()) {
            Ok(Packet::Handshake(h)) => {
                assert_eq!(h.car, "ks_ferrari_488_gt3");
                assert_eq!(h.driver, "Leon");
                assert_eq!(h.track, "monza");
                assert_eq!(h.track_config, "gp");
            }
            other => panic!("expected handshake, got {other:?}"),
        }
    }

    #[test]
    fn decodes_car_update() {
        match decode(&car_bytes()) {
            Ok(Packet::CarUpdate(c)) => {
                assert_eq!(c.speed_kmh, 187.5);
                assert_eq!(c.gear, 4);
                assert_eq!(c.rpm, 7413);
                assert_eq!(c.position, (10.5, -2.25, 300.0));
                assert_eq!(c.lap_time_ms, 61_234);
            }
            other => panic!("expected car update, got {other:?}"),
        }
    }

    #[test]
    fn decodes_driver_inputs_and_g_forces() {
        let mut b = car_bytes();
        put_f32(&mut b, CAR_THROTTLE, 0.75);
        put_f32(&mut b, CAR_BRAKE, 0.25);
        put_f32(&mut b, CAR_STEER_ANGLE, -123.5);
        put_f32(&mut b, CAR_G_LATERAL, 1.4);
        put_f32(&mut b, CAR_G_LONGITUDINAL, -0.6);
        put_f32(&mut b, CAR_G_VERTICAL, 1.02);
        match decode(&b) {
            Ok(Packet::CarUpdate(c)) => {
                assert_eq!(c.throttle, 0.75);
                assert_eq!(c.brake, 0.25);
                assert_eq!(c.steer_angle, -123.5);
                assert_eq!(c.g_lateral, 1.4);
                assert_eq!(c.g_longitudinal, -0.6);
                assert_eq!(c.g_vertical, 1.02);
                // Existing fields still decode from the same packet.
                assert_eq!(c.speed_kmh, 187.5);
                assert_eq!(c.gear, 4);
                assert_eq!(c.lap_time_ms, 61_234);
            }
            other => panic!("expected car update, got {other:?}"),
        }
    }

    #[test]
    fn driver_inputs_default_to_zero_when_unset() {
        // The builder leaves the new bytes at zero, so a packet without them still decodes.
        match decode(&car_bytes()) {
            Ok(Packet::CarUpdate(c)) => {
                assert_eq!(c.throttle, 0.0);
                assert_eq!(c.brake, 0.0);
                assert_eq!(c.steer_angle, 0.0);
                assert_eq!(c.g_lateral, 0.0);
                assert_eq!(c.g_longitudinal, 0.0);
                assert_eq!(c.g_vertical, 0.0);
            }
            other => panic!("expected car update, got {other:?}"),
        }
    }

    #[test]
    fn car_update_fields_do_not_overlap_and_fit_the_packet() {
        // (offset, width) of every float or int read from a car update.
        let fields = [
            (CAR_SPEED_KMH, 4),
            (CAR_LAP_TIME, 4),
            (CAR_ENGINE_RPM, 4),
            (CAR_GEAR, 4),
            (CAR_G_VERTICAL, 4),
            (CAR_G_LATERAL, 4),
            (CAR_G_LONGITUDINAL, 4),
            (CAR_THROTTLE, 4),
            (CAR_BRAKE, 4),
            (CAR_STEER_ANGLE, 4),
            (CAR_COORDINATES, 12),
        ];
        for (i, &(off, width)) in fields.iter().enumerate() {
            assert!(off + width <= CAR_UPDATE_LEN, "field at {off} overruns the packet");
            assert_eq!(off % 4, 0, "field at {off} is not 4-byte aligned");
            for &(other, other_width) in &fields[i + 1..] {
                let disjoint = off + width <= other || other + other_width <= off;
                assert!(disjoint, "fields at {off} and {other} overlap");
            }
        }
    }

    #[test]
    fn decodes_lap() {
        match decode(&lap_bytes()) {
            Ok(Packet::LapCompleted(l)) => {
                assert_eq!(l.car_id, 3);
                assert_eq!(l.laps, 5);
                assert_eq!(l.lap_time_ms, 92_500);
                assert_eq!(l.driver, "Max");
                assert_eq!(l.car, "ks_mazda_mx5");
            }
            other => panic!("expected lap, got {other:?}"),
        }
    }

    #[test]
    fn gear_and_rpm_are_clamped() {
        let mut b = car_bytes();
        put_i32(&mut b, CAR_GEAR, 1000);
        put_f32(&mut b, CAR_ENGINE_RPM, -5.0);
        match decode(&b) {
            Ok(Packet::CarUpdate(c)) => {
                assert_eq!(c.gear, i8::MAX);
                assert_eq!(c.rpm, 0);
            }
            other => panic!("expected car update, got {other:?}"),
        }
        put_i32(&mut b, CAR_GEAR, -1000);
        put_f32(&mut b, CAR_ENGINE_RPM, 1.0e9);
        match decode(&b) {
            Ok(Packet::CarUpdate(c)) => {
                assert_eq!(c.gear, i8::MIN);
                assert_eq!(c.rpm, u16::MAX);
            }
            other => panic!("expected car update, got {other:?}"),
        }
    }

    #[test]
    fn name_is_lossy_and_capped_at_field_length() {
        // Lone surrogates and no terminator: the decoder must cap at the 50-unit field.
        let mut b = handshake_bytes();
        for pair in b[HS_DRIVER_NAME..HS_DRIVER_NAME + NAME_LEN].as_chunks_mut::<2>().0 {
            *pair = 0xD800u16.to_le_bytes();
        }
        match decode(&b) {
            Ok(Packet::Handshake(h)) => {
                assert!(h.driver.chars().all(|c| c == '\u{FFFD}'));
                assert_eq!(h.driver.chars().count(), NAME_CHARS);
            }
            other => panic!("expected handshake, got {other:?}"),
        }
    }

    #[test]
    fn names_decode_non_ascii_utf16() {
        let mut b = handshake_bytes();
        put_name(&mut b, HS_DRIVER_NAME, "Jérôme Müller");
        match decode(&b) {
            Ok(Packet::Handshake(h)) => assert_eq!(h.driver, "Jérôme Müller"),
            other => panic!("expected handshake, got {other:?}"),
        }
    }

    #[test]
    fn empty_input_is_truncated() {
        assert_eq!(decode(&[]), Err(DecodeError::Truncated));
    }

    #[test]
    fn unknown_length_reports_first_byte() {
        let mut b = vec![0u8; HANDSHAKE_RESPONSE_LEN + 92];
        b[0] = 0x42;
        assert_eq!(decode(&b), Err(DecodeError::UnknownType(0x42)));
    }

    #[test]
    fn truncated_prefixes_return_err() {
        // Prefixes whose length equals another packet's exact length are
        // ambiguous by length alone (the packets have no type tag), so they
        // decode as that packet. Every other prefix must be rejected.
        let exact_lengths = [HANDSHAKE_RESPONSE_LEN, CAR_UPDATE_LEN, LAP_LEN];
        for packet in [handshake_bytes(), car_bytes(), lap_bytes()] {
            for len in 0..packet.len() {
                let prefix = &packet[..len];
                let result = decode(prefix);
                if exact_lengths.contains(&len) {
                    assert!(result.is_ok(), "prefix of len {len} should decode");
                } else {
                    assert!(result.is_err(), "prefix of len {len} must be Err");
                }
            }
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        for len in 0..=(HANDSHAKE_RESPONSE_LEN + 8) {
            let buf = vec![0xFFu8; len];
            let _ = decode(&buf);
            let buf = vec![0x00u8; len];
            let _ = decode(&buf);
        }
    }

    #[test]
    fn handshake_request_bytes() {
        let req = encode_handshake_request();
        assert_eq!(req.len(), HANDSHAKE_REQUEST_LEN);
        assert_eq!(
            req,
            vec![1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
            "identifier=1, version=1, operationId=0, little-endian"
        );
    }

    #[test]
    fn request_encoding_for_each_operation() {
        assert_eq!(encode_request(OP_HANDSHAKE), vec![1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(encode_request(OP_SUBSCRIBE_UPDATE), vec![1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(encode_request(OP_SUBSCRIBE_SPOT), vec![1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(encode_request(OP_DISMISS), vec![1, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0]);
        for op in [OP_HANDSHAKE, OP_SUBSCRIBE_UPDATE, OP_SUBSCRIBE_SPOT, OP_DISMISS] {
            assert_eq!(encode_request(op).len(), REQUEST_LEN);
        }
    }

    #[test]
    fn error_display_is_nonempty() {
        assert!(!DecodeError::Truncated.to_string().is_empty());
        assert!(DecodeError::UnknownType(0x7f).to_string().contains("7f"));
    }

    // ---- summarize_laps tests ----

    #[test]
    fn summarize_empty_is_default() {
        let s = summarize_laps(&[]);
        assert_eq!(s, LapSummary { best_ms: None, average_ms: None, count: 0 });
    }

    #[test]
    fn summarize_ignores_zero_and_implausible() {
        let laps = [lap(0), lap(9_999), lap(10_000), lap(90_000), lap(95_000)];
        let s = summarize_laps(&laps);
        assert_eq!(s.count, 3);
        assert_eq!(s.best_ms, Some(10_000));
        assert_eq!(s.average_ms, Some(65_000));
    }

    #[test]
    fn summarize_all_invalid_is_empty() {
        let s = summarize_laps(&[lap(0), lap(5_000)]);
        assert_eq!(s, LapSummary::default());
    }

    #[test]
    fn summarize_average_rounds_to_nearest() {
        // (90_001 + 90_002) / 2 = 90_001.5 -> 90_002
        let s = summarize_laps(&[lap(90_001), lap(90_002)]);
        assert_eq!(s.average_ms, Some(90_002));
        assert_eq!(s.best_ms, Some(90_001));
    }
}

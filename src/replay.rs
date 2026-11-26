//! Parser for Assetto Corsa `.acreplay` files (format version 16).
//!
//! Layout: a header, then per-car blocks, each holding a frame array with a trailing
//! extension count. All multi-byte integers are little endian.
//!
//! [`parse_replay`] returns the header summary and lap data. [`parse_replay_samples`]
//! also returns one [`Sample`] per frame of the first car, stored up to [`MAX_SAMPLES`].
//! Every car block is walked in both cases so that a truncated file is detected.

use std::fmt;
use std::io;
use std::path::Path;

/// The only header version this parser accepts.
const SUPPORTED_VERSION: u32 = 16;
/// Versions above this are treated as "not a replay" rather than "unsupported".
const MAX_PLAUSIBLE_VERSION: u32 = 255;
/// Upper bound for any length-prefixed string.
const MAX_STRING_LEN: usize = 4096;

/// Maximum number of per-frame samples kept for the first car. Frames past this point
/// are still validated and counted, but not stored; [`ReplaySamples::is_capped`] reports it.
pub const MAX_SAMPLES: usize = 500_000;

/// Size in bytes of one per-frame car record.
const CAR_FRAME_SIZE: usize = 256;
// Byte offsets inside a car frame. Fields are naturally aligned (f32 and u32 on
// 4-byte boundaries), which accounts for the gaps between the offsets below.
/// Car body position: three f32 values (x, y, with y up, z).
const FRAME_POSITION: usize = 0;
/// Body velocity: three f16 values (x, y, z) in m/s.
const FRAME_VELOCITY: usize = 164;
/// Engine speed in RPM, one f16 value.
const FRAME_RPM: usize = 170;
/// Byte offset of `currentLapTime` (u32, ms) inside a car frame. Only the test builder writes it.
#[cfg(test)]
const FRAME_CURRENT_LAP_TIME: usize = 220;
/// Byte offset of `lastLapTime` (u32, ms) inside a car frame.
const FRAME_LAST_LAP_TIME: usize = 224;
/// Gear code (u8): 0 = reverse, 1 = neutral, 2 and up = forward gears.
const FRAME_GEAR: usize = 234;
/// Throttle pedal (u8, 0..=255).
const FRAME_GAS: usize = 244;
/// Brake pedal (u8, 0..=255).
const FRAME_BRAKE: usize = 245;
/// Byte offset of `currentLap` (u8, 0 = first lap) inside a car frame.
const FRAME_CURRENT_LAP: usize = 246;

/// Unknown bytes after each car's header block.
const CAR_HEADER_PAD: u64 = 20;
/// Unknown bytes between consecutive frames of one car (plus wing data).
const FRAME_GAP: u64 = 20;
/// Size of one extra trailing entry at the end of a car block.
const TRAILING_ENTRY_SIZE: u64 = 8;

/// Summary of a replay file.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayInfo {
    /// Header version (always 16 for a successfully parsed file).
    pub version: u32,
    /// Track folder name.
    pub track: String,
    /// Track configuration, `None` when the header string is empty.
    pub config_track: Option<String>,
    /// Car ID of the first car entry.
    pub car: String,
    /// Driver name of the first car entry.
    pub driver: String,
    /// Derived: number of laps completed by the first car (see `lap_times_ms`).
    pub lap_count: usize,
    /// Derived: lap times in ms, taken from `lastLapTime` each time `currentLap` advances.
    pub lap_times_ms: Vec<u32>,
    /// Recording length of the first car in seconds (frames * recording interval).
    pub duration_s: f64,
    /// Number of frames recorded for the first car.
    pub sample_count: usize,
    /// Weather string from the header.
    pub weather: String,
    /// Number of car entries in the file.
    pub num_cars: usize,
}

/// One recorded frame of the first car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Seconds from the start of the recording: frame index times the recording interval.
    pub time_s: f64,
    /// Speed in km/h, the magnitude of the body velocity vector.
    pub speed_kmh: f32,
    /// Engine speed in revolutions per minute, as stored in the frame.
    pub rpm: f32,
    /// Raw gear code: 0 = reverse, 1 = neutral, 2 and up = forward gears (`n - 1`).
    pub gear: u8,
    /// Throttle from 0.0 (released) to 1.0 (fully pressed).
    pub throttle: f32,
    /// Brake from 0.0 (released) to 1.0 (fully pressed).
    pub brake: f32,
    /// Car body position x in metres.
    pub x: f32,
    /// Car body position y (height, up) in metres.
    pub y: f32,
    /// Car body position z in metres.
    pub z: f32,
}

/// A parsed replay together with the first car's per-frame samples.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaySamples {
    info: ReplayInfo,
    samples: Vec<Sample>,
    capped: bool,
}

impl ReplaySamples {
    /// Header summary and lap data, identical to what [`parse_replay`] returns.
    pub fn info(&self) -> &ReplayInfo {
        &self.info
    }

    /// Stored samples in recording order. Sample `i` was recorded at
    /// `i * recording_interval` seconds. Holds at most [`MAX_SAMPLES`] entries.
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// True when the first car had more frames than were stored.
    pub fn is_capped(&self) -> bool {
        self.capped
    }

    /// Consume the recording and return its samples, e.g. to hand to a playback model.
    pub fn into_samples(self) -> Vec<Sample> {
        self.samples
    }
}

/// Errors returned by the replay parser.
#[derive(Debug)]
pub enum ReplayError {
    /// Underlying I/O failure while reading the file.
    Io(io::Error),
    /// The bytes do not look like a replay header, or a field is out of range.
    BadHeader,
    /// Header version is plausible but not supported.
    UnsupportedVersion(u32),
    /// The buffer ended before the structure was complete.
    Truncated,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Io(e) => write!(f, "replay I/O error: {e}"),
            ReplayError::BadHeader => write!(f, "not a valid replay header"),
            ReplayError::UnsupportedVersion(v) => {
                write!(f, "unsupported replay version {v} (expected {SUPPORTED_VERSION})")
            }
            ReplayError::Truncated => write!(f, "replay file is truncated"),
        }
    }
}

impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReplayError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ReplayError {
    fn from(e: io::Error) -> Self {
        ReplayError::Io(e)
    }
}

/// Bounds-checked little-endian cursor over a byte slice.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ReplayError> {
        let end = self.pos.checked_add(n).ok_or(ReplayError::Truncated)?;
        let slice = self.buf.get(self.pos..end).ok_or(ReplayError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    /// Skip `n` bytes, where `n` may come from a 64-bit count in the file.
    fn skip(&mut self, n: u64) -> Result<(), ReplayError> {
        let n = usize::try_from(n).map_err(|_| ReplayError::Truncated)?;
        self.take(n).map(|_| ())
    }

    fn u32(&mut self) -> Result<u32, ReplayError> {
        let b = self.take(4)?;
        let arr: [u8; 4] = b.try_into().map_err(|_| ReplayError::Truncated)?;
        Ok(u32::from_le_bytes(arr))
    }

    fn f64(&mut self) -> Result<f64, ReplayError> {
        let b = self.take(8)?;
        let arr: [u8; 8] = b.try_into().map_err(|_| ReplayError::Truncated)?;
        Ok(f64::from_le_bytes(arr))
    }

    /// Length-prefixed string (u32 length, then bytes). Capped at `MAX_STRING_LEN`.
    fn string(&mut self) -> Result<String, ReplayError> {
        let len = self.u32()? as usize;
        if len > MAX_STRING_LEN {
            return Err(ReplayError::BadHeader);
        }
        let b = self.take(len)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
}

/// Read `N` bytes at `offset` from a car frame, failing instead of panicking when out of range.
fn frame_bytes<const N: usize>(frame: &[u8], offset: usize) -> Result<[u8; N], ReplayError> {
    let end = offset.checked_add(N).ok_or(ReplayError::Truncated)?;
    let b = frame.get(offset..end).ok_or(ReplayError::Truncated)?;
    b.try_into().map_err(|_| ReplayError::Truncated)
}

fn frame_u8(frame: &[u8], offset: usize) -> Result<u8, ReplayError> {
    let [b] = frame_bytes::<1>(frame, offset)?;
    Ok(b)
}

fn frame_u32(frame: &[u8], offset: usize) -> Result<u32, ReplayError> {
    Ok(u32::from_le_bytes(frame_bytes::<4>(frame, offset)?))
}

fn frame_f32(frame: &[u8], offset: usize) -> Result<f32, ReplayError> {
    Ok(f32::from_le_bytes(frame_bytes::<4>(frame, offset)?))
}

/// Read an IEEE 754 half-precision value and widen it to f32.
fn frame_f16(frame: &[u8], offset: usize) -> Result<f32, ReplayError> {
    Ok(f16_to_f32(u16::from_le_bytes(frame_bytes::<2>(frame, offset)?)))
}

/// Convert IEEE 754 binary16 bits to f32. Handles subnormals, infinities and NaN.
fn f16_to_f32(bits: u16) -> f32 {
    let negative = bits & 0x8000 != 0;
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x3ff);
    let magnitude = match exponent {
        // Subnormal: mantissa / 1024 * 2^-14.
        0 => mantissa * 2f32.powi(-24),
        31 if mantissa == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        e => (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
    };
    if negative { -magnitude } else { magnitude }
}

/// Lap information gathered from the first car's frames.
#[derive(Default)]
struct LapTracker {
    prev_lap: Option<u8>,
    lap_times_ms: Vec<u32>,
}

impl LapTracker {
    /// A lap is completed when `currentLap` advances; the completed lap time is `lastLapTime`.
    /// Zero values are ignored, and the first frame only sets the baseline.
    fn observe(&mut self, frame: &[u8]) -> Result<(), ReplayError> {
        let current_lap = frame_u8(frame, FRAME_CURRENT_LAP)?;
        if let Some(prev) = self.prev_lap && prev != current_lap {
            let last = frame_u32(frame, FRAME_LAST_LAP_TIME)?;
            if last != 0 {
                self.lap_times_ms.push(last);
            }
        }
        self.prev_lap = Some(current_lap);
        Ok(())
    }
}

/// Decode the telemetry fields of one car frame. `index` is the frame number within the car.
fn decode_sample(frame: &[u8], index: usize, interval_ms: f64) -> Result<Sample, ReplayError> {
    let vx = frame_f16(frame, FRAME_VELOCITY)?;
    let vy = frame_f16(frame, FRAME_VELOCITY + 2)?;
    let vz = frame_f16(frame, FRAME_VELOCITY + 4)?;
    let speed_ms = (vx * vx + vy * vy + vz * vz).sqrt();

    Ok(Sample {
        time_s: index as f64 * interval_ms / 1000.0,
        speed_kmh: speed_ms * 3.6,
        rpm: frame_f16(frame, FRAME_RPM)?,
        gear: frame_u8(frame, FRAME_GEAR)?,
        throttle: f32::from(frame_u8(frame, FRAME_GAS)?) / 255.0,
        brake: f32::from(frame_u8(frame, FRAME_BRAKE)?) / 255.0,
        x: frame_f32(frame, FRAME_POSITION)?,
        y: frame_f32(frame, FRAME_POSITION + 4)?,
        z: frame_f32(frame, FRAME_POSITION + 8)?,
    })
}

/// Walk the whole buffer. Samples are decoded for the first `sample_cap` frames of the
/// first car. Returns the summary, the stored samples, and whether any frame was dropped.
fn walk(bytes: &[u8], sample_cap: usize) -> Result<(ReplayInfo, Vec<Sample>, bool), ReplayError> {
    let mut r = Reader::new(bytes);

    let version = r.u32()?;
    if version == 0 || version > MAX_PLAUSIBLE_VERSION {
        return Err(ReplayError::BadHeader);
    }
    if version != SUPPORTED_VERSION {
        return Err(ReplayError::UnsupportedVersion(version));
    }

    // Recording interval in milliseconds.
    let recording_interval_ms = r.f64()?;
    if !recording_interval_ms.is_finite() || recording_interval_ms <= 0.0 {
        return Err(ReplayError::BadHeader);
    }

    let weather = r.string()?;
    let track = r.string()?;
    let track_config = r.string()?;
    let num_cars = r.u32()?;
    let _current_recording_index = r.u32()?;
    let num_frames = r.u32()?;
    let num_track_objects = r.u32()?;

    if num_cars == 0 {
        return Err(ReplayError::BadHeader);
    }

    // Sun angles (2 + 2 bytes) and track objects (12 bytes each), once per frame.
    let per_frame = 4u64 + 12u64 * u64::from(num_track_objects);
    let skip = per_frame
        .checked_mul(u64::from(num_frames))
        .ok_or(ReplayError::BadHeader)?;
    r.skip(skip)?;

    let mut samples: Vec<Sample> = Vec::new();
    let mut capped = false;
    let mut first_car: Option<(String, String, u32, LapTracker)> = None;

    for car_index in 0..num_cars {
        let car_id = r.string()?;
        let driver_name = r.string()?;
        let _nation_code = r.string()?;
        let _driver_team = r.string()?;
        let _car_skin_id = r.string()?;
        let car_frames = r.u32()?;
        let num_wings = r.u32()?;
        r.skip(CAR_HEADER_PAD)?;

        let wing_bytes = u64::from(num_wings) * 4;
        let mut laps = LapTracker::default();

        for i in 0..car_frames {
            let frame = r.take(CAR_FRAME_SIZE)?;
            if car_index == 0 {
                laps.observe(frame)?;
                let index = i as usize;
                if index < sample_cap {
                    samples.push(decode_sample(frame, index, recording_interval_ms)?);
                } else {
                    capped = true;
                }
            }
            if i + 1 < car_frames {
                r.skip(FRAME_GAP + wing_bytes)?;
            } else {
                // A trailing entry list follows only the last frame of a car.
                r.skip(wing_bytes)?;
                let count = r.u32()?;
                r.skip(u64::from(count) * TRAILING_ENTRY_SIZE)?;
            }
        }

        if car_index == 0 {
            first_car = Some((car_id, driver_name, car_frames, laps));
        }
    }

    // num_cars >= 1 is checked above, so the first car is always present.
    let (car, driver, sample_count_u32, laps) = first_car.ok_or(ReplayError::BadHeader)?;
    let sample_count = sample_count_u32 as usize;
    let duration_s = sample_count as f64 * recording_interval_ms / 1000.0;

    let info = ReplayInfo {
        version,
        track,
        config_track: if track_config.is_empty() {
            None
        } else {
            Some(track_config)
        },
        car,
        driver,
        lap_count: laps.lap_times_ms.len(),
        lap_times_ms: laps.lap_times_ms,
        duration_s,
        sample_count,
        weather,
        num_cars: num_cars as usize,
    };
    Ok((info, samples, capped))
}

/// Parse a complete `.acreplay` buffer. Performs no file IO.
pub fn parse_replay(bytes: &[u8]) -> Result<ReplayInfo, ReplayError> {
    // A cap of zero skips sample decoding entirely.
    walk(bytes, 0).map(|(info, _, _)| info)
}

/// Parse a complete `.acreplay` buffer and decode the first car's per-frame samples,
/// stored up to [`MAX_SAMPLES`]. Performs no file IO.
pub fn parse_replay_samples(bytes: &[u8]) -> Result<ReplaySamples, ReplayError> {
    let (info, samples, capped) = walk(bytes, MAX_SAMPLES)?;
    Ok(ReplaySamples {
        info,
        samples,
        capped,
    })
}

/// Read and parse a `.acreplay` file from disk.
pub fn read_replay(path: &Path) -> Result<ReplayInfo, ReplayError> {
    let bytes = std::fs::read(path)?;
    parse_replay(&bytes)
}

/// Read and parse a `.acreplay` file from disk, including per-frame samples.
pub fn read_replay_samples(path: &Path) -> Result<ReplaySamples, ReplayError> {
    let bytes = std::fs::read(path)?;
    parse_replay_samples(&bytes)
}

/// Byte offset of body orientation Euler angles (3 x f32, radians) in a car frame.
const FRAME_ROTATION: usize = 12;
/// Byte offset of the 4 wheel local pose blocks (4 x 24 bytes: pos[3] + rot[3]).
const FRAME_WHEELS: usize = 24;
/// Byte offset of car status/lights flags byte in a car frame.
const FRAME_LIGHTS: usize = 236;

/// Local position and rotation of one wheel decoded from a replay frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WheelPose {
    pub pos: [f32; 3],
    pub rot: [f32; 3],
}

/// Full 3D pose + telemetry for one car at one replay frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarFrame3D {
    pub sample: Sample,
    /// Body Euler angles `[pitch, yaw, roll]` in radians (zeroed if non-finite).
    pub rotation: [f32; 3],
    /// Four wheels (`LF`, `RF`, `LR`, `RR`).
    pub wheels: [WheelPose; 4],
    /// Bitmask for headlights/brake lights/status flags.
    pub lights_flags: u8,
    pub current_lap: u8,
}

/// Per-car metadata and decoded 3D frames from a multi-car `.acreplay`.
#[derive(Debug, Clone, PartialEq)]
pub struct CarReplay {
    pub car_id: String,
    pub driver: String,
    pub nation: String,
    pub team: String,
    pub skin_id: String,
    pub lap_times_ms: Vec<u32>,
    pub frames: Vec<CarFrame3D>,
}

/// Complete multi-car `.acreplay` recording for 2D/3D playback.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiCarReplay {
    pub info: ReplayInfo,
    pub recording_interval_ms: f64,
    /// Per-frame sun angles `(sun_angle_1, sun_angle_2)` from the global replay header.
    pub sun_angles: Vec<(i16, i16)>,
    pub cars: Vec<CarReplay>,
    pub capped: bool,
}

fn decode_car_frame_3d(
    frame: &[u8],
    index: usize,
    interval_ms: f64,
) -> Result<CarFrame3D, ReplayError> {
    let sample = decode_sample(frame, index, interval_ms)?;
    let mut rotation = [0.0f32; 3];
    for (i, slot) in rotation.iter_mut().enumerate() {
        let v = frame_f32(frame, FRAME_ROTATION + i * 4)?;
        *slot = if v.is_finite() { v } else { 0.0 };
    }
    let mut wheels = [WheelPose::default(); 4];
    for (w_idx, wheel) in wheels.iter_mut().enumerate() {
        let base = FRAME_WHEELS + w_idx * 24;
        for i in 0..3 {
            let p = frame_f32(frame, base + i * 4)?;
            let r = frame_f32(frame, base + 12 + i * 4)?;
            wheel.pos[i] = if p.is_finite() { p } else { 0.0 };
            wheel.rot[i] = if r.is_finite() { r } else { 0.0 };
        }
    }
    Ok(CarFrame3D {
        sample,
        rotation,
        wheels,
        lights_flags: frame_u8(frame, FRAME_LIGHTS)?,
        current_lap: frame_u8(frame, FRAME_CURRENT_LAP)?,
    })
}

/// Parses a v16 `.acreplay` buffer and decodes 3D frames for ALL cars (up to `max_frames` per car).
pub fn parse_replay_multicar(
    bytes: &[u8],
    max_frames: usize,
) -> Result<MultiCarReplay, ReplayError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version == 0 || version > MAX_PLAUSIBLE_VERSION {
        return Err(ReplayError::BadHeader);
    }
    if version != SUPPORTED_VERSION {
        return Err(ReplayError::UnsupportedVersion(version));
    }
    let recording_interval_ms = r.f64()?;
    if !recording_interval_ms.is_finite() || recording_interval_ms <= 0.0 {
        return Err(ReplayError::BadHeader);
    }

    let weather = r.string()?;
    let track = r.string()?;
    let track_config = r.string()?;
    let num_cars = r.u32()?;
    let _current_recording_index = r.u32()?;
    let num_frames = r.u32()?;
    let num_track_objects = r.u32()?;
    if num_cars == 0 {
        return Err(ReplayError::BadHeader);
    }

    let obj_bytes = 12u64
        .checked_mul(u64::from(num_track_objects))
        .ok_or(ReplayError::BadHeader)?;
    let mut sun_angles = Vec::with_capacity((num_frames as usize).min(max_frames));
    for i in 0..num_frames {
        let s = r.take(4)?;
        if (i as usize) < max_frames {
            let a = i16::from_le_bytes([s[0], s[1]]);
            let b = i16::from_le_bytes([s[2], s[3]]);
            sun_angles.push((a, b));
        }
        r.skip(obj_bytes)?;
    }

    let mut cars = Vec::with_capacity(num_cars as usize);
    let mut capped = false;
    for _ in 0..num_cars {
        let car_id = r.string()?;
        let driver = r.string()?;
        let nation = r.string()?;
        let team = r.string()?;
        let skin_id = r.string()?;
        let car_frames = r.u32()?;
        let num_wings = r.u32()?;
        r.skip(CAR_HEADER_PAD)?;

        let wing_bytes = u64::from(num_wings) * 4;
        let mut laps = LapTracker::default();
        let mut frames = Vec::with_capacity((car_frames as usize).min(max_frames));
        for i in 0..car_frames {
            let frame = r.take(CAR_FRAME_SIZE)?;
            laps.observe(frame)?;
            if (i as usize) < max_frames {
                frames.push(decode_car_frame_3d(frame, i as usize, recording_interval_ms)?);
            } else {
                capped = true;
            }
            if i + 1 < car_frames {
                r.skip(FRAME_GAP + wing_bytes)?;
            } else {
                r.skip(wing_bytes)?;
                let count = r.u32()?;
                r.skip(u64::from(count) * TRAILING_ENTRY_SIZE)?;
            }
        }
        cars.push(CarReplay {
            car_id,
            driver,
            nation,
            team,
            skin_id,
            lap_times_ms: laps.lap_times_ms,
            frames,
        });
    }

    let first = cars.first().ok_or(ReplayError::BadHeader)?;
    let sample_count = first.frames.len();
    let info = ReplayInfo {
        version,
        track,
        config_track: (!track_config.is_empty()).then_some(track_config),
        car: first.car_id.clone(),
        driver: first.driver.clone(),
        lap_count: first.lap_times_ms.len(),
        lap_times_ms: first.lap_times_ms.clone(),
        duration_s: sample_count as f64 * recording_interval_ms / 1000.0,
        sample_count,
        weather,
        num_cars: cars.len(),
    };
    Ok(MultiCarReplay {
        info,
        recording_interval_ms,
        sun_angles,
        cars,
        capped,
    })
}

/// Reads and parses a multi-car `.acreplay` file from disk.
pub fn read_replay_multicar(path: &Path, max_frames: usize) -> Result<MultiCarReplay, ReplayError> {
    let bytes = std::fs::read(path)?;
    parse_replay_multicar(&bytes, max_frames)
}

/// Slices a v16 `.acreplay` binary buffer to the half-open frame range `[start_frame, end_frame)`,
/// producing a valid, self-contained `.acreplay` clip preserving all cars, track objects, and
/// trailing metadata blocks.
pub fn trim_replay(bytes: &[u8], start_frame: u32, end_frame: u32) -> Result<Vec<u8>, ReplayError> {
    if start_frame >= end_frame {
        return Err(ReplayError::BadHeader);
    }
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != SUPPORTED_VERSION {
        return Err(ReplayError::UnsupportedVersion(version));
    }
    let interval_ms = r.f64()?;
    if !interval_ms.is_finite() || interval_ms <= 0.0 {
        return Err(ReplayError::BadHeader);
    }
    let weather = r.string()?;
    let track = r.string()?;
    let track_config = r.string()?;
    let num_cars = r.u32()?;
    let _current_recording_index = r.u32()?;
    let num_frames = r.u32()?;
    let num_track_objects = r.u32()?;
    if num_cars == 0 || end_frame > num_frames {
        return Err(ReplayError::BadHeader);
    }

    let new_num_frames = end_frame - start_frame;
    let mut out = Vec::with_capacity(bytes.len() / 2 + 256);
    let put_str = |buf: &mut Vec<u8>, s: &str| {
        buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    };
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&interval_ms.to_le_bytes());
    put_str(&mut out, &weather);
    put_str(&mut out, &track);
    put_str(&mut out, &track_config);
    out.extend_from_slice(&num_cars.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // current_recording_index reset
    out.extend_from_slice(&new_num_frames.to_le_bytes());
    out.extend_from_slice(&num_track_objects.to_le_bytes());

    // Slice global sun + track-object block.
    let per_global = (4u64 + 12u64 * u64::from(num_track_objects)) as usize;
    let global_block = r.take(
        per_global
            .checked_mul(num_frames as usize)
            .ok_or(ReplayError::Truncated)?,
    )?;
    let g_start = (start_frame as usize) * per_global;
    let g_end = (end_frame as usize) * per_global;
    out.extend_from_slice(&global_block[g_start..g_end]);

    // Slice each car block.
    for _ in 0..num_cars {
        let car_id = r.string()?;
        let driver = r.string()?;
        let nation = r.string()?;
        let team = r.string()?;
        let skin = r.string()?;
        let car_frames = r.u32()?;
        let num_wings = r.u32()?;
        let pad = r.take(CAR_HEADER_PAD as usize)?;
        let wing_bytes = (num_wings as usize) * 4;
        let gap = FRAME_GAP as usize;

        let mut raw_frames: Vec<&[u8]> = Vec::with_capacity(car_frames as usize);
        let mut raw_wings: Vec<&[u8]> = Vec::with_capacity(car_frames as usize);
        let mut raw_gaps: Vec<&[u8]> = Vec::with_capacity(car_frames as usize);
        let mut trailing_count = 0u32;
        let mut trailing_bytes: &[u8] = &[];

        for i in 0..car_frames {
            raw_frames.push(r.take(CAR_FRAME_SIZE)?);
            if i + 1 < car_frames {
                let between = r.take(gap + wing_bytes)?;
                raw_wings.push(&between[..wing_bytes]);
                raw_gaps.push(&between[wing_bytes..]);
            } else {
                raw_wings.push(r.take(wing_bytes)?);
                trailing_count = r.u32()?;
                let tb_len = (trailing_count as usize)
                    .checked_mul(TRAILING_ENTRY_SIZE as usize)
                    .ok_or(ReplayError::Truncated)?;
                trailing_bytes = r.take(tb_len)?;
            }
        }

        let s = (start_frame as usize).min(raw_frames.len());
        let e = (end_frame as usize).min(raw_frames.len());
        let kept = e.saturating_sub(s);

        put_str(&mut out, &car_id);
        put_str(&mut out, &driver);
        put_str(&mut out, &nation);
        put_str(&mut out, &team);
        put_str(&mut out, &skin);
        out.extend_from_slice(&(kept as u32).to_le_bytes());
        out.extend_from_slice(&num_wings.to_le_bytes());
        out.extend_from_slice(pad);

        for k in 0..kept {
            let idx = s + k;
            out.extend_from_slice(raw_frames[idx]);
            if k + 1 < kept {
                out.extend_from_slice(raw_wings[idx]);
                out.extend_from_slice(raw_gaps[idx]);
            } else {
                out.extend_from_slice(raw_wings[idx]);
                out.extend_from_slice(&trailing_count.to_le_bytes());
                out.extend_from_slice(trailing_bytes);
            }
        }
    }

    Ok(out)
}

/// Reads `src`, trims `[start_frame, end_frame)`, and writes to a new file `dst`
/// (refusing to overwrite an existing file).
pub fn trim_replay_file(
    src: &Path,
    dst: &Path,
    start_frame: u32,
    end_frame: u32,
) -> Result<usize, ReplayError> {
    use std::fs::OpenOptions;
    use std::io::Write;
    let bytes = std::fs::read(src)?;
    let trimmed = trim_replay(&bytes, start_frame, end_frame)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(dst)?;
    file.write_all(&trimmed)?;
    Ok(trimmed.len())
}

/// Format a lap time as `M:SS.mmm`, e.g. `1:23.456`.
pub fn format_lap_time(ms: u32) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_str(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }

    /// A 256-byte car frame with the lap fields set; everything else zero.
    fn frame(current_lap: u8, current_lap_time: u32, last_lap_time: u32) -> Vec<u8> {
        let mut f = vec![0u8; CAR_FRAME_SIZE];
        f[FRAME_CURRENT_LAP_TIME..FRAME_CURRENT_LAP_TIME + 4]
            .copy_from_slice(&current_lap_time.to_le_bytes());
        f[FRAME_LAST_LAP_TIME..FRAME_LAST_LAP_TIME + 4]
            .copy_from_slice(&last_lap_time.to_le_bytes());
        f[FRAME_CURRENT_LAP] = current_lap;
        f
    }

    /// Writes telemetry for frame `i` so that every decoded field is distinguishable.
    /// Frame 2 has 3 m/s and 4 m/s lateral/longitudinal velocity (5 m/s = 18 km/h),
    /// 7000 rpm, gear code 4 and full throttle. Frame 4 has full brake.
    fn set_telemetry(f: &mut [u8], i: usize) {
        let x = i as f32 * 0.5;
        f[FRAME_POSITION..FRAME_POSITION + 4].copy_from_slice(&x.to_le_bytes());
        f[FRAME_POSITION + 4..FRAME_POSITION + 8].copy_from_slice(&(x + 1.0).to_le_bytes());
        f[FRAME_POSITION + 8..FRAME_POSITION + 12].copy_from_slice(&(-x).to_le_bytes());

        let (vx, vz): (u16, u16) = if i == 2 {
            (0x4200, 0x4400) // 3.0 and 4.0 as f16
        } else {
            (0, 0)
        };
        f[FRAME_VELOCITY..FRAME_VELOCITY + 2].copy_from_slice(&vx.to_le_bytes());
        f[FRAME_VELOCITY + 4..FRAME_VELOCITY + 6].copy_from_slice(&vz.to_le_bytes());

        let rpm: u16 = if i == 2 { 0x6ED6 } else { 0 }; // 7000.0 as f16
        f[FRAME_RPM..FRAME_RPM + 2].copy_from_slice(&rpm.to_le_bytes());
        f[FRAME_GEAR] = if i == 2 { 4 } else { 0 };
        f[FRAME_GAS] = if i == 2 { 255 } else { 0 };
        f[FRAME_BRAKE] = if i == 4 { 255 } else { 0 };
    }

    /// Synthetic replay: 2 cars, track objects present, car 0 has 6 frames with
    /// two completed laps (83456 ms and 81001 ms), car 1 has 2 frames and wing data.
    fn synthetic_replay(config: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&16u32.to_le_bytes()); // version
        b.extend_from_slice(&500.0f64.to_le_bytes()); // recording interval ms
        push_str(&mut b, "clear"); // weather
        push_str(&mut b, "ks_nordschleife"); // track
        push_str(&mut b, config); // track config
        b.extend_from_slice(&2u32.to_le_bytes()); // numCars
        b.extend_from_slice(&6u32.to_le_bytes()); // currentRecordingIndex
        b.extend_from_slice(&6u32.to_le_bytes()); // numFrames
        b.extend_from_slice(&1u32.to_le_bytes()); // numTrackObjects
        // Sun angles and one track object per frame: (2 + 2 + 12) * 6 bytes.
        b.extend_from_slice(&[0u8; 16 * 6]);

        // Car 0: no wings.
        push_str(&mut b, "ks_ferrari_488_gt3");
        push_str(&mut b, "Max Tester");
        push_str(&mut b, "NL");
        push_str(&mut b, "Team");
        push_str(&mut b, "0");
        b.extend_from_slice(&6u32.to_le_bytes()); // numFrames
        b.extend_from_slice(&0u32.to_le_bytes()); // numWings
        b.extend_from_slice(&[0u8; 20]);
        let car0: Vec<Vec<u8>> = [
            (0, 0, 0),
            (0, 10_000, 0),
            (1, 500, 83_456),
            (1, 20_000, 83_456),
            (2, 600, 81_001),
            (2, 30_000, 81_001),
        ]
        .iter()
        .enumerate()
        .map(|(i, &(lap, cur, last))| {
            let mut f = frame(lap, cur, last);
            set_telemetry(&mut f, i);
            f
        })
        .collect();
        for (i, f) in car0.iter().enumerate() {
            b.extend_from_slice(f);
            if i + 1 < car0.len() {
                b.extend_from_slice(&[0u8; 20]);
            } else {
                b.extend_from_slice(&0u32.to_le_bytes()); // trailing count
            }
        }

        // Car 1: two wings, two frames, one trailing entry on the last frame.
        push_str(&mut b, "bmw_m3_gt2");
        push_str(&mut b, "Other Driver");
        push_str(&mut b, "DE");
        push_str(&mut b, "");
        push_str(&mut b, "1");
        b.extend_from_slice(&2u32.to_le_bytes()); // numFrames
        b.extend_from_slice(&2u32.to_le_bytes()); // numWings
        b.extend_from_slice(&[0u8; 20]);
        b.extend_from_slice(&frame(0, 0, 0));
        b.extend_from_slice(&[0u8; 20 + 8]); // gap + wing data
        b.extend_from_slice(&frame(0, 1_000, 0));
        b.extend_from_slice(&[0u8; 8]); // wing data on last frame
        b.extend_from_slice(&1u32.to_le_bytes()); // trailing count
        b.extend_from_slice(&[0u8; 8]); // one trailing entry of 8 bytes
        b
    }

    #[test]
    fn parses_header_and_first_car() {
        let info = parse_replay(&synthetic_replay("GP")).expect("valid replay");
        assert_eq!(info.version, 16);
        assert_eq!(info.track, "ks_nordschleife");
        assert_eq!(info.config_track.as_deref(), Some("GP"));
        assert_eq!(info.weather, "clear");
        assert_eq!(info.car, "ks_ferrari_488_gt3");
        assert_eq!(info.driver, "Max Tester");
        assert_eq!(info.num_cars, 2);
        assert_eq!(info.sample_count, 6);
        assert!((info.duration_s - 3.0).abs() < 1e-9);
    }

    #[test]
    fn empty_track_config_is_none() {
        let info = parse_replay(&synthetic_replay("")).expect("valid replay");
        assert_eq!(info.config_track, None);
    }

    #[test]
    fn extracts_lap_times_from_lap_changes() {
        let info = parse_replay(&synthetic_replay("GP")).expect("valid replay");
        assert_eq!(info.lap_times_ms, vec![83_456, 81_001]);
        assert_eq!(info.lap_count, 2);
    }

    #[test]
    fn samples_decode_first_car_frames_only() {
        let bytes = synthetic_replay("GP");
        let rec = parse_replay_samples(&bytes).expect("valid replay");
        assert_eq!(rec.info(), &parse_replay(&bytes).expect("valid replay"));
        assert!(!rec.is_capped());

        // Six frames of car 0, not the two of car 1.
        let samples = rec.samples();
        assert_eq!(samples.len(), 6);
        assert!((samples[5].time_s - 2.5).abs() < 1e-9);

        let s = samples[2];
        assert!((s.time_s - 1.0).abs() < 1e-9);
        assert!((s.speed_kmh - 18.0).abs() < 1e-3);
        assert!((s.rpm - 7000.0).abs() < 1e-3);
        assert_eq!(s.gear, 4);
        assert!((s.throttle - 1.0).abs() < 1e-6);
        assert_eq!(s.brake, 0.0);
        assert!((s.x - 1.0).abs() < 1e-6);
        assert!((s.y - 2.0).abs() < 1e-6);
        assert!((s.z + 1.0).abs() < 1e-6);

        assert!((samples[4].brake - 1.0).abs() < 1e-6);
        assert_eq!(samples[0].speed_kmh, 0.0);
        assert_eq!(samples[0].throttle, 0.0);
    }

    #[test]
    fn sample_cap_truncates_and_flags() {
        let bytes = synthetic_replay("GP");
        let (info, samples, capped) = walk(&bytes, 3).expect("valid replay");
        assert_eq!(samples.len(), 3);
        assert!(capped);
        assert_eq!(info.sample_count, 6);

        let (_, all, capped_all) = walk(&bytes, 6).expect("valid replay");
        assert_eq!(all.len(), 6);
        assert!(!capped_all);
    }

    #[test]
    fn f16_conversion_special_values() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0xC000), -2.0);
        assert_eq!(f16_to_f32(0x4200), 3.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x7C00), f32::INFINITY);
        assert_eq!(f16_to_f32(0xFC00), f32::NEG_INFINITY);
        assert!(f16_to_f32(0x7E00).is_nan());
    }

    #[test]
    fn format_lap_time_cases() {
        assert_eq!(format_lap_time(0), "0:00.000");
        assert_eq!(format_lap_time(83_456), "1:23.456");
        assert_eq!(format_lap_time(81_001), "1:21.001");
        assert_eq!(format_lap_time(60_000), "1:00.000");
        assert_eq!(format_lap_time(3_599_999), "59:59.999");
    }

    #[test]
    fn every_prefix_is_truncated_not_panic() {
        let full = synthetic_replay("GP");
        for len in 0..full.len() {
            match parse_replay(&full[..len]) {
                Err(ReplayError::Truncated) => {}
                other => panic!("prefix of length {len} gave {other:?}"),
            }
        }
        assert!(parse_replay(&full).is_ok());
    }

    #[test]
    fn sample_prefixes_are_truncated_not_panic() {
        let full = synthetic_replay("GP");
        for len in 0..full.len() {
            assert!(
                matches!(parse_replay_samples(&full[..len]), Err(ReplayError::Truncated)),
                "prefix of length {len} did not report truncation"
            );
        }
        assert!(parse_replay_samples(&full).is_ok());
    }

    #[test]
    fn garbage_returns_bad_header() {
        assert!(matches!(
            parse_replay(b"this is not a replay file at all"),
            Err(ReplayError::BadHeader)
        ));
        assert!(matches!(parse_replay(&[0xFFu8; 64]), Err(ReplayError::BadHeader)));
        assert!(matches!(parse_replay(&[0u8; 64]), Err(ReplayError::BadHeader)));
        assert!(matches!(parse_replay(&[]), Err(ReplayError::Truncated)));
    }

    #[test]
    fn pseudo_random_buffers_never_panic() {
        // Simple LCG so the test is deterministic.
        let mut state: u64 = 0x1234_5678_9abc_def0;
        for _ in 0..500 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = (state >> 33) as usize % 300;
            let mut buf = Vec::with_capacity(len);
            for _ in 0..len {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                buf.push((state >> 33) as u8);
            }
            // Force a plausible version so deeper parsing paths are exercised too.
            if buf.len() >= 4 {
                buf[..4].copy_from_slice(&16u32.to_le_bytes());
            }
            let _ = parse_replay(&buf);
            let _ = parse_replay_samples(&buf);
        }
    }

    #[test]
    fn unsupported_version_is_reported() {
        let mut b = synthetic_replay("GP");
        b[..4].copy_from_slice(&15u32.to_le_bytes());
        assert!(matches!(
            parse_replay(&b),
            Err(ReplayError::UnsupportedVersion(15))
        ));
    }

    #[test]
    fn oversized_string_is_bad_header() {
        let mut b = Vec::new();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&500.0f64.to_le_bytes());
        b.extend_from_slice(&u32::MAX.to_le_bytes()); // weather length
        assert!(matches!(parse_replay(&b), Err(ReplayError::BadHeader)));
    }

    #[test]
    fn huge_counts_do_not_allocate_or_panic() {
        let mut b = Vec::new();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&500.0f64.to_le_bytes());
        push_str(&mut b, "");
        push_str(&mut b, "");
        push_str(&mut b, "");
        b.extend_from_slice(&1u32.to_le_bytes()); // numCars
        b.extend_from_slice(&0u32.to_le_bytes()); // currentRecordingIndex
        b.extend_from_slice(&u32::MAX.to_le_bytes()); // numFrames
        b.extend_from_slice(&u32::MAX.to_le_bytes()); // numTrackObjects
        assert!(parse_replay(&b).is_err());
        assert!(parse_replay_samples(&b).is_err());
    }

    #[test]
    fn zero_cars_is_bad_header() {
        let mut b = Vec::new();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&500.0f64.to_le_bytes());
        push_str(&mut b, "");
        push_str(&mut b, "");
        push_str(&mut b, "");
        b.extend_from_slice(&0u32.to_le_bytes()); // numCars
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(parse_replay(&b), Err(ReplayError::BadHeader)));
    }

    #[test]
    fn read_replay_missing_file_is_io_error() {
        let path = Path::new("/nonexistent/definitely/missing.acreplay");
        assert!(matches!(read_replay(path), Err(ReplayError::Io(_))));
        assert!(matches!(read_replay_samples(path), Err(ReplayError::Io(_))));
    }

    #[test]
    fn error_display_and_from_io() {
        assert!(!ReplayError::Truncated.to_string().is_empty());
        assert!(ReplayError::UnsupportedVersion(7).to_string().contains('7'));
        let e: ReplayError = io::Error::other("boom").into();
        assert!(matches!(e, ReplayError::Io(_)));
    }

    #[test]
    fn multicar_decode_and_trim_replay_round_trip() {
        // Build a synthetic 2-car, 60-frame, 3-track-object, 2-wing, 4-trailing-entry v16 replay.
        let num_frames = 60u32;
        let num_objects = 3u32;
        let num_wings = 2u32;
        let trailing_count = 4u32;
        let mut buf = Vec::new();
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&33.333f64.to_le_bytes());
        push_str(&mut buf, "3_clear");
        push_str(&mut buf, "monza");
        push_str(&mut buf, "");
        buf.extend_from_slice(&2u32.to_le_bytes()); // num_cars
        buf.extend_from_slice(&0u32.to_le_bytes()); // current_recording_index
        buf.extend_from_slice(&num_frames.to_le_bytes());
        buf.extend_from_slice(&num_objects.to_le_bytes());
        for f in 0..num_frames {
            buf.extend_from_slice(&(f as i16).to_le_bytes());
            buf.extend_from_slice(&(-(f as i16)).to_le_bytes());
            buf.extend_from_slice(&vec![0xABu8; (num_objects as usize) * 12]);
        }
        for car_i in 0..2u32 {
            push_str(&mut buf, &format!("car_{car_i}"));
            push_str(&mut buf, &format!("Driver {car_i}"));
            push_str(&mut buf, "DE");
            push_str(&mut buf, "Team");
            push_str(&mut buf, &format!("skin_{car_i}"));
            buf.extend_from_slice(&num_frames.to_le_bytes());
            buf.extend_from_slice(&num_wings.to_le_bytes());
            buf.extend_from_slice(&vec![0u8; CAR_HEADER_PAD as usize]);
            for f in 0..num_frames {
                let mut fr = frame(0, f * 33, 0);
                let x = (f as f32) + (car_i as f32) * 100.0;
                fr[FRAME_POSITION..FRAME_POSITION + 4].copy_from_slice(&x.to_le_bytes());
                fr[FRAME_ROTATION + 4..FRAME_ROTATION + 8].copy_from_slice(&0.5f32.to_le_bytes());
                fr[FRAME_LIGHTS] = 0b0000_0011;
                buf.extend_from_slice(&fr);
                if f + 1 < num_frames {
                    buf.extend_from_slice(&vec![0x11u8; (FRAME_GAP as usize) + (num_wings as usize) * 4]);
                } else {
                    buf.extend_from_slice(&vec![0x22u8; (num_wings as usize) * 4]);
                    buf.extend_from_slice(&trailing_count.to_le_bytes());
                    buf.extend_from_slice(&vec![0x33u8; (trailing_count as usize) * (TRAILING_ENTRY_SIZE as usize)]);
                }
            }
        }

        let mc = parse_replay_multicar(&buf, 1000).unwrap();
        assert_eq!(mc.cars.len(), 2);
        assert_eq!(mc.sun_angles.len(), 60);
        assert_eq!(mc.cars[1].skin_id, "skin_1");
        assert!((mc.cars[0].frames[15].sample.x - 15.0).abs() < 1e-5);
        assert!((mc.cars[0].frames[15].rotation[1] - 0.5).abs() < 1e-5);
        assert_eq!(mc.cars[0].frames[15].lights_flags, 0b0000_0011);

        // Trim [10, 40) -> 30 frames, re-parse and verify exact positions & metadata.
        let trimmed = trim_replay(&buf, 10, 40).unwrap();
        assert!(trimmed.len() < buf.len());
        let mc_trim = parse_replay_multicar(&trimmed, 1000).unwrap();
        assert_eq!(mc_trim.cars.len(), 2);
        assert_eq!(mc_trim.cars[0].frames.len(), 30);
        assert_eq!(mc_trim.cars[1].frames.len(), 30);
        assert_eq!(mc_trim.sun_angles[0], (10, -10));
        assert!((mc_trim.cars[0].frames[0].sample.x - 10.0).abs() < 1e-5);
        assert!((mc_trim.cars[1].frames[29].sample.x - 139.0).abs() < 1e-5);

        // Invalid ranges rejected.
        assert!(trim_replay(&buf, 40, 40).is_err());
        assert!(trim_replay(&buf, 10, 61).is_err());
    }
}

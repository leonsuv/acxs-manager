//! Pure parsers for the game's shared-memory pages.
//!
//! The game publishes three read-only pages: physics (updated every physics
//! step), graphics (HUD and timing state) and static (session and car limits
//! that stay fixed for a session). This module turns the bytes of one page
//! into a typed struct. The parsers make no OS calls. On Windows,
//! [`open_pages`] and [`read_snapshot`] open and map the pages, then hand the
//! bytes to the parsers.
//!
//! Layout rules, which match a record with a maximum field alignment of 4:
//! little-endian integers and floats are aligned to 4 bytes, UTF-16 string
//! fields are aligned to 2 bytes, and padding is inserted where needed. Strings
//! are fixed-length arrays of UTF-16 code units. They are decoded lossily and
//! end at the first NUL.
//!
//! Buffers longer than the page are accepted, because the mapping may be
//! larger than the page. Shorter buffers are rejected with
//! [`SharedMemError::TooShort`].

use std::fmt;

/// Byte length of a physics page: the offset of its last field plus that field.
pub const PHYSICS_PAGE_LEN: usize = 580;
/// Byte length of a graphics page.
pub const GRAPHICS_PAGE_LEN: usize = 288;
/// Byte length of a static page.
pub const STATIC_PAGE_LEN: usize = 684;

/// Errors from the page parsers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedMemError {
    /// The buffer ends before the page's last field.
    TooShort {
        /// Which page was being parsed: `"physics"`, `"graphics"` or `"static"`.
        page: &'static str,
        /// Bytes the page needs.
        needed: usize,
        /// Bytes actually supplied.
        got: usize,
    },
    /// The game's mapping for this page does not exist, usually because the
    /// game is not running.
    NotRunning {
        /// Which page was being opened.
        page: &'static str,
    },
    /// A Windows call failed.
    Os {
        /// The call that failed.
        call: &'static str,
        /// The value of `GetLastError` after the failure.
        code: u32,
    },
    /// Shared memory is only available on Windows.
    Unsupported,
}

impl fmt::Display for SharedMemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SharedMemError::TooShort { page, needed, got } => write!(
                f,
                "{page} page too short: need {needed} bytes, got {got}"
            ),
            SharedMemError::NotRunning { page } => write!(
                f,
                "{page} page is not mapped: is the game running?"
            ),
            SharedMemError::Os { call, code } => {
                write!(f, "{call} failed with Windows error {code}")
            }
            SharedMemError::Unsupported => {
                write!(f, "shared memory pages are only available on Windows")
            }
        }
    }
}

impl std::error::Error for SharedMemError {}

/// Session state reported by the graphics page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcStatus {
    Off,
    Replay,
    Live,
    Pause,
    /// A value this module does not know yet.
    Unknown(i32),
}

impl AcStatus {
    fn from_raw(v: i32) -> Self {
        match v {
            0 => AcStatus::Off,
            1 => AcStatus::Replay,
            2 => AcStatus::Live,
            3 => AcStatus::Pause,
            other => AcStatus::Unknown(other),
        }
    }
}

/// Session kind reported by the graphics page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Practice,
    Qualify,
    Race,
    Hotlap,
    TimeAttack,
    Drift,
    Drag,
    /// Any other value, including -1 for unknown.
    Unknown(i32),
}

impl SessionType {
    fn from_raw(v: i32) -> Self {
        match v {
            0 => SessionType::Practice,
            1 => SessionType::Qualify,
            2 => SessionType::Race,
            3 => SessionType::Hotlap,
            4 => SessionType::TimeAttack,
            5 => SessionType::Drift,
            6 => SessionType::Drag,
            other => SessionType::Unknown(other),
        }
    }
}

/// Flag shown to the driver, reported by the graphics page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagType {
    None,
    Blue,
    Yellow,
    Black,
    White,
    Checkered,
    Penalty,
    /// A value this module does not know yet.
    Unknown(i32),
}

impl FlagType {
    fn from_raw(v: i32) -> Self {
        match v {
            0 => FlagType::None,
            1 => FlagType::Blue,
            2 => FlagType::Yellow,
            3 => FlagType::Black,
            4 => FlagType::White,
            5 => FlagType::Checkered,
            6 => FlagType::Penalty,
            other => FlagType::Unknown(other),
        }
    }
}

/// The physics page, updated every physics step.
///
/// Four-element arrays are ordered front-left, front-right, rear-left,
/// rear-right. Integer fields are raw values as the game writes them, and a
/// nonzero flag means true.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsPage {
    pub packet_id: i32,
    /// Throttle position, 0.0 to 1.0.
    pub throttle: f32,
    /// Brake position, 0.0 to 1.0.
    pub brake: f32,
    pub fuel: f32,
    /// Raw gear index as the game writes it.
    pub gear: i32,
    pub rpm: i32,
    pub steer_angle: f32,
    pub speed_kmh: f32,
    /// Velocity in world axes.
    pub velocity: [f32; 3],
    /// Acceleration in g, as x, y and z.
    pub g_forces: [f32; 3],
    /// Wheel slip per tyre.
    pub wheel_slip: [f32; 4],
    pub wheel_load: [f32; 4],
    pub wheels_pressure: [f32; 4],
    pub wheel_angular_speed: [f32; 4],
    pub tyre_wear: [f32; 4],
    pub tyre_dirty_level: [f32; 4],
    pub tyre_core_temperature: [f32; 4],
    pub camber_rad: [f32; 4],
    pub suspension_travel: [f32; 4],
    pub drs: f32,
    pub tc: f32,
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub cg_height: f32,
    pub car_damage: [f32; 5],
    pub number_of_tyres_out: i32,
    pub pit_limiter_on: i32,
    pub abs: f32,
    pub kers_charge: f32,
    pub kers_input: f32,
    pub auto_shifter_on: i32,
    pub ride_height: [f32; 2],
    pub turbo_boost: f32,
    pub ballast: f32,
    pub air_density: f32,
    pub air_temp: f32,
    pub road_temp: f32,
    pub local_angular_velocity: [f32; 3],
    pub final_ff: f32,
    pub performance_meter: f32,
    pub engine_brake: i32,
    pub ers_recovery_level: i32,
    pub ers_power_level: i32,
    pub ers_heat_charging: i32,
    pub ers_is_charging: i32,
    pub kers_current_kj: f32,
    pub drs_available: i32,
    pub drs_enabled: i32,
    pub brake_temp: [f32; 4],
    pub clutch: f32,
    pub tyre_temp_inner: [f32; 4],
    pub tyre_temp_middle: [f32; 4],
    pub tyre_temp_outer: [f32; 4],
    pub is_ai_controlled: i32,
    /// Contact point per tyre, as x, y and z.
    pub tyre_contact_point: [[f32; 3]; 4],
    /// Contact normal per tyre, as x, y and z.
    pub tyre_contact_normal: [[f32; 3]; 4],
    /// Contact heading per tyre, as x, y and z.
    pub tyre_contact_heading: [[f32; 3]; 4],
    pub brake_bias: f32,
    /// Velocity in the car's local axes.
    pub local_velocity: [f32; 3],
}

/// The graphics page: timing, position and session state.
///
/// Lap and sector times are text as the game formats them. The `*_ms` fields
/// are the integer counterparts; their unit is assumed to be milliseconds, and
/// the layout itself does not state it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphicsPage {
    pub packet_id: i32,
    pub status: AcStatus,
    pub session: SessionType,
    pub current_lap_time: String,
    pub last_lap_time: String,
    pub best_lap_time: String,
    pub split: String,
    pub completed_laps: i32,
    pub position: i32,
    pub current_lap_ms: i32,
    pub last_lap_ms: i32,
    pub best_lap_ms: i32,
    pub session_time_left: f32,
    pub distance_traveled: f32,
    pub is_in_pit: i32,
    pub current_sector_index: i32,
    pub last_sector_time: i32,
    pub number_of_laps: i32,
    pub tyre_compound: String,
    pub replay_time_multiplier: f32,
    pub normalized_car_position: f32,
    /// Car position in world coordinates, as x, y and z.
    pub car_coordinates: [f32; 3],
    pub penalty_time: f32,
    pub flag: FlagType,
    pub ideal_line_on: i32,
    pub is_in_pit_lane: i32,
    pub surface_grip: f32,
    pub mandatory_pit_done: i32,
}

/// The static page: session and car limits that stay fixed for a session.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticPage {
    pub sm_version: String,
    pub ac_version: String,
    pub number_of_sessions: i32,
    pub num_cars: i32,
    pub car_model: String,
    pub track: String,
    pub player_name: String,
    pub player_surname: String,
    pub player_nick: String,
    pub sector_count: i32,
    pub max_torque: f32,
    pub max_power: f32,
    pub max_rpm: i32,
    pub fuel_max: f32,
    pub suspension_max_travel: [f32; 4],
    pub tyre_radius: [f32; 4],
    pub max_turbo_boost: f32,
    /// Retired field, kept so later offsets line up.
    pub deprecated_1: f32,
    /// Retired field, kept so later offsets line up.
    pub deprecated_2: f32,
    pub penalties_enabled: i32,
    pub aid_fuel_rate: f32,
    pub aid_tyre_rate: f32,
    pub aid_mechanical_damage: f32,
    pub aid_allow_tyre_blankets: i32,
    pub aid_stability: f32,
    pub aid_auto_clutch: i32,
    pub aid_auto_blip: i32,
    pub has_drs: i32,
    pub has_ers: i32,
    pub has_kers: i32,
    pub kers_max_joules: f32,
    pub engine_brake_settings_count: i32,
    pub ers_power_controller_count: i32,
    pub track_spline_length: f32,
    pub track_configuration: String,
    pub ers_max_j: f32,
    pub is_timed_race: i32,
    pub has_extra_lap: i32,
    pub car_skin: String,
    pub reversed_grid_positions: i32,
    pub pit_window_start: i32,
    pub pit_window_end: i32,
}

/// Parses a physics page. `bytes` must hold at least [`PHYSICS_PAGE_LEN`] bytes.
pub fn parse_physics(bytes: &[u8]) -> Result<PhysicsPage, SharedMemError> {
    check_len(bytes, PHYSICS_PAGE_LEN, "physics")?;
    let mut r = Reader::new(bytes, "physics");
    Ok(PhysicsPage {
        packet_id: r.read_i32()?,
        throttle: r.read_f32()?,
        brake: r.read_f32()?,
        fuel: r.read_f32()?,
        gear: r.read_i32()?,
        rpm: r.read_i32()?,
        steer_angle: r.read_f32()?,
        speed_kmh: r.read_f32()?,
        velocity: r.read_f32s()?,
        g_forces: r.read_f32s()?,
        wheel_slip: r.read_f32s()?,
        wheel_load: r.read_f32s()?,
        wheels_pressure: r.read_f32s()?,
        wheel_angular_speed: r.read_f32s()?,
        tyre_wear: r.read_f32s()?,
        tyre_dirty_level: r.read_f32s()?,
        tyre_core_temperature: r.read_f32s()?,
        camber_rad: r.read_f32s()?,
        suspension_travel: r.read_f32s()?,
        drs: r.read_f32()?,
        tc: r.read_f32()?,
        heading: r.read_f32()?,
        pitch: r.read_f32()?,
        roll: r.read_f32()?,
        cg_height: r.read_f32()?,
        car_damage: r.read_f32s()?,
        number_of_tyres_out: r.read_i32()?,
        pit_limiter_on: r.read_i32()?,
        abs: r.read_f32()?,
        kers_charge: r.read_f32()?,
        kers_input: r.read_f32()?,
        auto_shifter_on: r.read_i32()?,
        ride_height: r.read_f32s()?,
        turbo_boost: r.read_f32()?,
        ballast: r.read_f32()?,
        air_density: r.read_f32()?,
        air_temp: r.read_f32()?,
        road_temp: r.read_f32()?,
        local_angular_velocity: r.read_f32s()?,
        final_ff: r.read_f32()?,
        performance_meter: r.read_f32()?,
        engine_brake: r.read_i32()?,
        ers_recovery_level: r.read_i32()?,
        ers_power_level: r.read_i32()?,
        ers_heat_charging: r.read_i32()?,
        ers_is_charging: r.read_i32()?,
        kers_current_kj: r.read_f32()?,
        drs_available: r.read_i32()?,
        drs_enabled: r.read_i32()?,
        brake_temp: r.read_f32s()?,
        clutch: r.read_f32()?,
        tyre_temp_inner: r.read_f32s()?,
        tyre_temp_middle: r.read_f32s()?,
        tyre_temp_outer: r.read_f32s()?,
        is_ai_controlled: r.read_i32()?,
        tyre_contact_point: r.read_vec3s()?,
        tyre_contact_normal: r.read_vec3s()?,
        tyre_contact_heading: r.read_vec3s()?,
        brake_bias: r.read_f32()?,
        local_velocity: r.read_f32s()?,
    })
}

/// Parses a graphics page. `bytes` must hold at least [`GRAPHICS_PAGE_LEN`] bytes.
pub fn parse_graphics(bytes: &[u8]) -> Result<GraphicsPage, SharedMemError> {
    check_len(bytes, GRAPHICS_PAGE_LEN, "graphics")?;
    let mut r = Reader::new(bytes, "graphics");
    Ok(GraphicsPage {
        packet_id: r.read_i32()?,
        status: AcStatus::from_raw(r.read_i32()?),
        session: SessionType::from_raw(r.read_i32()?),
        current_lap_time: r.read_wstr::<15>()?,
        last_lap_time: r.read_wstr::<15>()?,
        best_lap_time: r.read_wstr::<15>()?,
        split: r.read_wstr::<15>()?,
        completed_laps: r.read_i32()?,
        position: r.read_i32()?,
        current_lap_ms: r.read_i32()?,
        last_lap_ms: r.read_i32()?,
        best_lap_ms: r.read_i32()?,
        session_time_left: r.read_f32()?,
        distance_traveled: r.read_f32()?,
        is_in_pit: r.read_i32()?,
        current_sector_index: r.read_i32()?,
        last_sector_time: r.read_i32()?,
        number_of_laps: r.read_i32()?,
        tyre_compound: r.read_wstr::<33>()?,
        replay_time_multiplier: r.read_f32()?,
        normalized_car_position: r.read_f32()?,
        car_coordinates: r.read_f32s()?,
        penalty_time: r.read_f32()?,
        flag: FlagType::from_raw(r.read_i32()?),
        ideal_line_on: r.read_i32()?,
        is_in_pit_lane: r.read_i32()?,
        surface_grip: r.read_f32()?,
        mandatory_pit_done: r.read_i32()?,
    })
}

/// Parses a static page. `bytes` must hold at least [`STATIC_PAGE_LEN`] bytes.
pub fn parse_static(bytes: &[u8]) -> Result<StaticPage, SharedMemError> {
    check_len(bytes, STATIC_PAGE_LEN, "static")?;
    let mut r = Reader::new(bytes, "static");
    Ok(StaticPage {
        sm_version: r.read_wstr::<15>()?,
        ac_version: r.read_wstr::<15>()?,
        number_of_sessions: r.read_i32()?,
        num_cars: r.read_i32()?,
        car_model: r.read_wstr::<33>()?,
        track: r.read_wstr::<33>()?,
        player_name: r.read_wstr::<33>()?,
        player_surname: r.read_wstr::<33>()?,
        player_nick: r.read_wstr::<33>()?,
        sector_count: r.read_i32()?,
        max_torque: r.read_f32()?,
        max_power: r.read_f32()?,
        max_rpm: r.read_i32()?,
        fuel_max: r.read_f32()?,
        suspension_max_travel: r.read_f32s()?,
        tyre_radius: r.read_f32s()?,
        max_turbo_boost: r.read_f32()?,
        deprecated_1: r.read_f32()?,
        deprecated_2: r.read_f32()?,
        penalties_enabled: r.read_i32()?,
        aid_fuel_rate: r.read_f32()?,
        aid_tyre_rate: r.read_f32()?,
        aid_mechanical_damage: r.read_f32()?,
        aid_allow_tyre_blankets: r.read_i32()?,
        aid_stability: r.read_f32()?,
        aid_auto_clutch: r.read_i32()?,
        aid_auto_blip: r.read_i32()?,
        has_drs: r.read_i32()?,
        has_ers: r.read_i32()?,
        has_kers: r.read_i32()?,
        kers_max_joules: r.read_f32()?,
        engine_brake_settings_count: r.read_i32()?,
        ers_power_controller_count: r.read_i32()?,
        track_spline_length: r.read_f32()?,
        track_configuration: r.read_wstr::<33>()?,
        ers_max_j: r.read_f32()?,
        is_timed_race: r.read_i32()?,
        has_extra_lap: r.read_i32()?,
        car_skin: r.read_wstr::<33>()?,
        reversed_grid_positions: r.read_i32()?,
        pit_window_start: r.read_i32()?,
        pit_window_end: r.read_i32()?,
    })
}

/// All three pages parsed from one read.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub physics: PhysicsPage,
    pub graphics: GraphicsPage,
    pub static_page: StaticPage,
}

#[cfg(windows)]
const PHYSICS_MAPPING: &str = "acpmf_physics";
#[cfg(windows)]
const GRAPHICS_MAPPING: &str = "acpmf_graphics";
#[cfg(windows)]
const STATIC_MAPPING: &str = "acpmf_static";

/// Owned copies of the raw bytes of each page, read from the game's mappings.
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedPages {
    pub physics: Vec<u8>,
    pub graphics: Vec<u8>,
    pub static_page: Vec<u8>,
}

/// Opens the game's physics, graphics and static mappings read-only and copies
/// the bytes of each page. The mappings and handles are released before this
/// returns, so the result owns its data.
///
/// Fails with [`SharedMemError::NotRunning`] when a mapping is missing.
#[cfg(windows)]
pub fn open_pages() -> Result<SharedPages, SharedMemError> {
    Ok(SharedPages {
        physics: read_mapping(PHYSICS_MAPPING, PHYSICS_PAGE_LEN, "physics")?,
        graphics: read_mapping(GRAPHICS_MAPPING, GRAPHICS_PAGE_LEN, "graphics")?,
        static_page: read_mapping(STATIC_MAPPING, STATIC_PAGE_LEN, "static")?,
    })
}

/// Opens and parses all three pages in one call.
#[cfg(windows)]
pub fn read_snapshot() -> Result<Snapshot, SharedMemError> {
    let pages = open_pages()?;
    Ok(Snapshot {
        physics: parse_physics(&pages.physics)?,
        graphics: parse_graphics(&pages.graphics)?,
        static_page: parse_static(&pages.static_page)?,
    })
}

/// Off Windows there is no shared memory, so this always returns
/// [`SharedMemError::Unsupported`].
#[cfg(not(windows))]
pub fn read_snapshot() -> Result<Snapshot, SharedMemError> {
    Err(SharedMemError::Unsupported)
}

/// Owns a file mapping handle and closes it on drop.
#[cfg(windows)]
struct MappingHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for MappingHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenFileMappingW and is
        // closed only here.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Owns a mapped view and unmaps it on drop.
#[cfg(windows)]
struct MappedView(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for MappedView {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        // SAFETY: the address came from a successful MapViewOfFile and is
        // unmapped only here.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.0 });
        }
    }
}

/// Opens the named mapping, maps `len` bytes read-only and copies them out.
/// The view is dropped before the handle, so both are released on every path.
#[cfg(windows)]
fn read_mapping(name: &str, len: usize, page: &'static str) -> Result<Vec<u8>, SharedMemError> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError};
    use windows_sys::Win32::System::Memory::{FILE_MAP_READ, MapViewOfFile, OpenFileMappingW};

    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: `wide` is NUL-terminated and lives across the call.
    let raw_handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr()) };
    if raw_handle.is_null() {
        // SAFETY: plain query of the calling thread's last error.
        let code = unsafe { GetLastError() };
        return Err(if code == ERROR_FILE_NOT_FOUND {
            SharedMemError::NotRunning { page }
        } else {
            SharedMemError::Os {
                call: "OpenFileMappingW",
                code,
            }
        });
    }
    let handle = MappingHandle(raw_handle);

    // SAFETY: `handle` is an open file mapping object.
    let addr = unsafe { MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, len) };
    if addr.Value.is_null() {
        // SAFETY: plain query of the calling thread's last error.
        let code = unsafe { GetLastError() };
        return Err(SharedMemError::Os {
            call: "MapViewOfFile",
            code,
        });
    }
    let view = MappedView(addr.Value);

    // SAFETY: the view maps at least `len` readable bytes and stays mapped
    // until `view` is dropped at the end of this function.
    let bytes = unsafe { std::slice::from_raw_parts(view.0 as *const u8, len) }.to_vec();
    Ok(bytes)
}

fn check_len(bytes: &[u8], needed: usize, page: &'static str) -> Result<(), SharedMemError> {
    if bytes.len() < needed {
        Err(SharedMemError::TooShort {
            page,
            needed,
            got: bytes.len(),
        })
    } else {
        Ok(())
    }
}

/// Sequential, bounds-checked reader over one page.
///
/// Every read checks the remaining length, so a short buffer returns an error
/// and never panics.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    page: &'static str,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], page: &'static str) -> Self {
        Reader { bytes, pos: 0, page }
    }

    fn too_short(&self, needed: usize) -> SharedMemError {
        SharedMemError::TooShort {
            page: self.page,
            needed,
            got: self.bytes.len(),
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SharedMemError> {
        let bytes: &'a [u8] = self.bytes;
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| self.too_short(usize::MAX))?;
        let slice = bytes.get(self.pos..end).ok_or_else(|| self.too_short(end))?;
        self.pos = end;
        Ok(slice)
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], SharedMemError> {
        let slice = self.take(N)?;
        let mut out = [0u8; N];
        for (dst, src) in out.iter_mut().zip(slice) {
            *dst = *src;
        }
        Ok(out)
    }

    /// Skips padding so the next field starts on a multiple of `to`.
    fn align(&mut self, to: usize) -> Result<(), SharedMemError> {
        let pad = (to - self.pos % to) % to;
        self.take(pad).map(|_| ())
    }

    fn read_i32(&mut self) -> Result<i32, SharedMemError> {
        self.align(4)?;
        Ok(i32::from_le_bytes(self.take_array::<4>()?))
    }

    fn read_f32(&mut self) -> Result<f32, SharedMemError> {
        self.align(4)?;
        Ok(f32::from_le_bytes(self.take_array::<4>()?))
    }

    fn read_f32s<const N: usize>(&mut self) -> Result<[f32; N], SharedMemError> {
        let mut out = [0.0f32; N];
        for v in out.iter_mut() {
            *v = self.read_f32()?;
        }
        Ok(out)
    }

    fn read_vec3s<const N: usize>(&mut self) -> Result<[[f32; 3]; N], SharedMemError> {
        let mut out = [[0.0f32; 3]; N];
        for v in out.iter_mut() {
            *v = self.read_f32s::<3>()?;
        }
        Ok(out)
    }

    /// Reads a fixed array of `N` UTF-16 code units and decodes it up to the
    /// first NUL. Invalid sequences become U+FFFD.
    fn read_wstr<const N: usize>(&mut self) -> Result<String, SharedMemError> {
        let raw = self.take(2 * N)?;
        let units: Vec<u16> = raw
            .as_chunks::<2>().0.iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes little-endian values at explicit byte offsets. The tests use
    /// this rather than the parser's cursor, so the offsets are checked
    /// independently.
    struct Buf(Vec<u8>);

    impl Buf {
        fn new(len: usize) -> Self {
            Buf(vec![0; len])
        }

        fn i32(&mut self, at: usize, v: i32) -> &mut Self {
            self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
            self
        }

        fn f32(&mut self, at: usize, v: f32) -> &mut Self {
            self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
            self
        }

        /// Writes up to `units` UTF-16 code units. A string shorter than the
        /// field is left NUL-padded.
        fn wstr(&mut self, at: usize, s: &str, units: usize) -> &mut Self {
            for (i, u) in s.encode_utf16().take(units).enumerate() {
                let p = at + 2 * i;
                self.0[p..p + 2].copy_from_slice(&u.to_le_bytes());
            }
            self
        }
    }

    fn assert_short<T: std::fmt::Debug>(r: Result<T, SharedMemError>, page: &'static str, needed: usize, got: usize) {
        assert_eq!(
            r.unwrap_err(),
            SharedMemError::TooShort { page, needed, got }
        );
    }

    #[test]
    fn physics_fields_at_layout_offsets() {
        let mut b = Buf::new(PHYSICS_PAGE_LEN);
        b.i32(0, 42)
            .f32(4, 0.9)
            .f32(8, 0.1)
            .f32(12, 55.5)
            .i32(16, 4)
            .i32(20, 7200)
            .f32(24, -0.25)
            .f32(28, 201.5)
            .f32(44, 1.2) // g_forces[0]
            .f32(64, 0.33) // wheel_slip[2]
            .i32(244, 1) // number_of_tyres_out
            .i32(416, 1) // is_ai_controlled
            .f32(464, 9.5) // tyre_contact_point[3].z (420 + 3*12 + 8)
            .f32(564, 0.56) // brake_bias
            .f32(576, 3.0); // local_velocity[2]
        let p = parse_physics(&b.0).unwrap();
        assert_eq!(p.packet_id, 42);
        assert_eq!(p.throttle, 0.9);
        assert_eq!(p.brake, 0.1);
        assert_eq!(p.fuel, 55.5);
        assert_eq!(p.gear, 4);
        assert_eq!(p.rpm, 7200);
        assert_eq!(p.steer_angle, -0.25);
        assert_eq!(p.speed_kmh, 201.5);
        assert_eq!(p.g_forces[0], 1.2);
        assert_eq!(p.wheel_slip[2], 0.33);
        assert_eq!(p.number_of_tyres_out, 1);
        assert_eq!(p.is_ai_controlled, 1);
        assert_eq!(p.tyre_contact_point[3][2], 9.5);
        assert_eq!(p.brake_bias, 0.56);
        assert_eq!(p.local_velocity[2], 3.0);
    }

    #[test]
    fn physics_truncated_at_every_length_is_rejected() {
        let full = vec![0u8; PHYSICS_PAGE_LEN];
        for len in 0..PHYSICS_PAGE_LEN {
            assert_short(parse_physics(&full[..len]), "physics", PHYSICS_PAGE_LEN, len);
        }
        assert!(parse_physics(&full).is_ok());
    }

    #[test]
    fn physics_accepts_longer_buffer() {
        let mut b = Buf::new(PHYSICS_PAGE_LEN + 64);
        b.f32(28, 150.0);
        let p = parse_physics(&b.0).unwrap();
        assert_eq!(p.speed_kmh, 150.0);
    }

    #[test]
    fn graphics_fields_at_layout_offsets() {
        let mut b = Buf::new(GRAPHICS_PAGE_LEN);
        b.i32(4, 2) // status: Live
            .i32(8, 2) // session: Race
            .wstr(12, "1:23.456", 15)
            .wstr(42, "1:24.001", 15)
            .wstr(72, "1:22.900", 15)
            .wstr(102, "+0.5", 15)
            .i32(132, 3) // completed_laps
            .i32(136, 5) // position
            .i32(140, 83456)
            .i32(144, 84001)
            .i32(148, 82900)
            .f32(152, 1200.0)
            .i32(160, 1) // is_in_pit
            .i32(172, 10) // number_of_laps
            .wstr(176, "SM", 33)
            .f32(244, 1.0) // replay_time_multiplier, after 2 bytes of padding
            .f32(248, 0.42) // normalized_car_position
            .f32(252, 100.0) // car_coordinates[0]
            .i32(268, 4) // flag: White
            .i32(284, 1); // mandatory_pit_done
        let g = parse_graphics(&b.0).unwrap();
        assert_eq!(g.status, AcStatus::Live);
        assert_eq!(g.session, SessionType::Race);
        assert_eq!(g.current_lap_time, "1:23.456");
        assert_eq!(g.last_lap_time, "1:24.001");
        assert_eq!(g.best_lap_time, "1:22.900");
        assert_eq!(g.split, "+0.5");
        assert_eq!(g.completed_laps, 3);
        assert_eq!(g.position, 5);
        assert_eq!(g.current_lap_ms, 83456);
        assert_eq!(g.last_lap_ms, 84001);
        assert_eq!(g.best_lap_ms, 82900);
        assert_eq!(g.session_time_left, 1200.0);
        assert_eq!(g.is_in_pit, 1);
        assert_eq!(g.number_of_laps, 10);
        assert_eq!(g.tyre_compound, "SM");
        assert_eq!(g.replay_time_multiplier, 1.0);
        assert_eq!(g.normalized_car_position, 0.42);
        assert_eq!(g.car_coordinates[0], 100.0);
        assert_eq!(g.flag, FlagType::White);
        assert_eq!(g.mandatory_pit_done, 1);
    }

    #[test]
    fn graphics_padding_bytes_are_ignored() {
        let mut b = Buf::new(GRAPHICS_PAGE_LEN);
        b.0[242] = 0xFF;
        b.0[243] = 0xFF;
        b.f32(244, 2.5);
        let g = parse_graphics(&b.0).unwrap();
        assert_eq!(g.replay_time_multiplier, 2.5);
    }

    #[test]
    fn graphics_truncated_at_every_length_is_rejected() {
        let full = vec![0u8; GRAPHICS_PAGE_LEN];
        for len in 0..GRAPHICS_PAGE_LEN {
            assert_short(parse_graphics(&full[..len]), "graphics", GRAPHICS_PAGE_LEN, len);
        }
        assert!(parse_graphics(&full).is_ok());
    }

    #[test]
    fn static_fields_at_layout_offsets() {
        let mut b = Buf::new(STATIC_PAGE_LEN);
        b.wstr(0, "1.7", 15)
            .wstr(30, "1.16.3", 15)
            .i32(60, 3) // number_of_sessions
            .i32(64, 20) // num_cars
            .wstr(68, "ferrari_488_gt3", 33)
            .wstr(134, "monza", 33)
            .wstr(200, "Max", 33)
            .wstr(266, "Verstappen", 33)
            .wstr(332, "vvv", 33)
            .i32(400, 3) // sector_count, after 2 bytes of padding
            .f32(404, 550.0) // max_torque
            .i32(412, 8500) // max_rpm
            .f32(416, 100.0) // fuel_max
            .f32(436, 0.33) // tyre_radius[0]
            .wstr(524, "gp", 33)
            .f32(592, 4.5) // ers_max_j, after 2 bytes of padding
            .wstr(604, "red_fox", 33)
            .i32(672, 1) // reversed_grid_positions, after 2 bytes of padding
            .i32(676, 5)
            .i32(680, 15);
        let s = parse_static(&b.0).unwrap();
        assert_eq!(s.sm_version, "1.7");
        assert_eq!(s.ac_version, "1.16.3");
        assert_eq!(s.number_of_sessions, 3);
        assert_eq!(s.num_cars, 20);
        assert_eq!(s.car_model, "ferrari_488_gt3");
        assert_eq!(s.track, "monza");
        assert_eq!(s.player_name, "Max");
        assert_eq!(s.player_surname, "Verstappen");
        assert_eq!(s.player_nick, "vvv");
        assert_eq!(s.sector_count, 3);
        assert_eq!(s.max_torque, 550.0);
        assert_eq!(s.max_rpm, 8500);
        assert_eq!(s.fuel_max, 100.0);
        assert_eq!(s.tyre_radius[0], 0.33);
        assert_eq!(s.track_configuration, "gp");
        assert_eq!(s.ers_max_j, 4.5);
        assert_eq!(s.car_skin, "red_fox");
        assert_eq!(s.reversed_grid_positions, 1);
        assert_eq!(s.pit_window_start, 5);
        assert_eq!(s.pit_window_end, 15);
    }

    #[test]
    fn static_truncated_at_every_length_is_rejected() {
        let full = vec![0u8; STATIC_PAGE_LEN];
        for len in 0..STATIC_PAGE_LEN {
            assert_short(parse_static(&full[..len]), "static", STATIC_PAGE_LEN, len);
        }
        assert!(parse_static(&full).is_ok());
    }

    #[test]
    fn wstr_stops_at_first_nul() {
        let mut b = Buf::new(16);
        // "abc", then a NUL at unit 3, then "def" that must not be read.
        b.wstr(0, "abc", 8).wstr(8, "def", 4);
        let mut r = Reader::new(&b.0, "test");
        assert_eq!(r.read_wstr::<8>().unwrap(), "abc");
        // The read still consumes the whole field, so the next one starts after it.
        assert_eq!(r.pos, 16);
    }

    #[test]
    fn wstr_full_length_without_nul() {
        let mut b = Buf::new(30);
        b.wstr(0, "xxxxxxxxxxxxxxx", 15);
        let mut r = Reader::new(&b.0, "test");
        assert_eq!(r.read_wstr::<15>().unwrap(), "xxxxxxxxxxxxxxx");
    }

    #[test]
    fn wstr_empty_and_all_nul() {
        let b = Buf::new(66);
        let mut r = Reader::new(&b.0, "test");
        assert_eq!(r.read_wstr::<33>().unwrap(), "");
    }

    #[test]
    fn wstr_decodes_non_ascii() {
        let mut b = Buf::new(66);
        b.wstr(0, "Pålsson 日本", 33);
        let mut r = Reader::new(&b.0, "test");
        assert_eq!(r.read_wstr::<33>().unwrap(), "Pålsson 日本");
    }

    #[test]
    fn wstr_replaces_unpaired_surrogate() {
        let mut b = Buf::new(8);
        // 'A', an unpaired high surrogate, 'B', then NUL.
        let units: [u16; 4] = [0x0041, 0xD800, 0x0042, 0x0000];
        for (i, u) in units.iter().enumerate() {
            b.0[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        let mut r = Reader::new(&b.0, "test");
        assert_eq!(r.read_wstr::<4>().unwrap(), "A\u{FFFD}B");
    }

    #[test]
    fn short_reader_returns_error_without_panicking() {
        let bytes = [0u8; 3];
        let mut r = Reader::new(&bytes, "test");
        assert_short(r.read_i32(), "test", 4, 3);
        let mut r = Reader::new(&bytes, "test");
        assert_short(r.read_wstr::<2>(), "test", 4, 3);
    }

    #[test]
    fn unknown_enum_values_are_kept_raw() {
        let mut b = Buf::new(GRAPHICS_PAGE_LEN);
        b.i32(4, 9).i32(8, -1).i32(268, 2);
        let g = parse_graphics(&b.0).unwrap();
        assert_eq!(g.status, AcStatus::Unknown(9));
        assert_eq!(g.session, SessionType::Unknown(-1));
        assert_eq!(g.flag, FlagType::Yellow);
    }

    #[test]
    fn error_message_names_page_and_sizes() {
        let msg = SharedMemError::TooShort {
            page: "graphics",
            needed: 288,
            got: 10,
        }
        .to_string();
        assert!(msg.contains("graphics"));
        assert!(msg.contains("288"));
        assert!(msg.contains("10"));
    }

    #[cfg(not(windows))]
    #[test]
    fn read_snapshot_returns_error_off_windows() {
        let result = read_snapshot();
        assert!(matches!(result, Err(SharedMemError::Unsupported)));
    }
}

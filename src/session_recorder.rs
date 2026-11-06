//! Records the player's own driving from the game's shared memory.
//!
//! [`SessionAggregator`] is pure: it is fed snapshots with the elapsed time and keeps running
//! sums, maxima and event counts, no per-sample storage. [`spawn`] runs it on a background
//! thread and writes one JSON file per session to `%APPDATA%\ACXS\sessions\`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::driving_stats::DrivenSession;
use crate::sharedmem::{self, AcStatus, Snapshot};

/// Sessions shorter than this many seconds of live driving are not saved.
const MIN_SESSION_SECONDS: f64 = 30.0;
/// Sample period while the game is live (about 30 Hz).
const SAMPLE_PERIOD: Duration = Duration::from_millis(33);
/// Poll period while no game is running.
const IDLE_PERIOD: Duration = Duration::from_secs(2);
/// Seconds without readable shared memory before a session is closed.
const GONE_AFTER: f64 = 5.0;
/// Largest step one sample may add, so a stalled thread cannot invent drive time.
const MAX_STEP: f64 = 0.2;
/// Wheel load in newtons below which a wheel counts as off the ground.
const AIRBORNE_LOAD: f32 = 1.0;
/// All four wheels must stay light this long before a jump counts.
const MIN_AIRBORNE: f64 = 0.25;
/// Peak g and speed loss that together make a crash.
const CRASH_G: f32 = 2.5;
/// Spike needed when the brake is pressed at least [`CRASH_BRAKE`].
const CRASH_G_BRAKING: f32 = 4.5;
const CRASH_BRAKE: f32 = 0.5;
const CRASH_SPEED_LOSS: f32 = 15.0;
const CRASH_WINDOW: usize = 10;
/// Seconds between two crashes of the same kind that count separately.
const EVENT_GAP: f64 = 3.0;

/// One lap as the game timed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedLap {
    pub number: i32,
    pub time_ms: i32,
}

/// Kinds counted by occurrence, in the order of `SessionAggregator::event_counts`.
const COUNTED: [EventKind; 6] = [
    EventKind::Crash,
    EventKind::OffTrack,
    EventKind::FrontLockup,
    EventKind::RearLockup,
    EventKind::Wheelspin,
    EventKind::Jump,
];

/// Number of equal slices of the lap used to place events.
pub const SEGMENTS: usize = 100;

/// What happened at a spot on the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Crash,
    OffTrack,
    FrontLockup,
    RearLockup,
    Wheelspin,
    Jump,
    Understeer,
    Oversteer,
    WallHit,
    CarHit,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            EventKind::Crash => "crashes",
            EventKind::OffTrack => "off track",
            EventKind::FrontLockup => "front lock-ups",
            EventKind::RearLockup => "rear lock-ups",
            EventKind::Wheelspin => "wheelspins",
            EventKind::Jump => "jumps",
            EventKind::Understeer => "understeer",
            EventKind::Oversteer => "oversteer",
            EventKind::WallHit => "wall hits",
            EventKind::CarHit => "car hits",
        }
    }
}

/// Events of one kind in one slice of the lap. Understeer and oversteer count samples-worth
/// of time in tenths of a second, the rest count occurrences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotspot {
    /// Slice 0..[`SEGMENTS`] of the normalized track position.
    pub segment: u8,
    pub kind: EventKind,
    pub count: u32,
}

/// Metres between points of the recorded trace.
const TRACE_STEP_M: f64 = 8.0;
/// Most trace points kept, so a very long session stays small.
const TRACE_MAX: usize = 3000;
/// Events are merged per square of this many metres on the trace.
const EVENT_CELL_M: f32 = 25.0;

/// An event with its world position, for tracks the game gives no lap position for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EventPoint {
    pub kind: EventKind,
    pub x: f32,
    pub z: f32,
    /// Occurrences, or tenths of a second for understeer and oversteer.
    pub count: u32,
}

/// What was recorded for one session. This is the file format.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RecordedSession {
    pub car_id: String,
    pub skin: String,
    pub track_id: String,
    pub track_layout: String,
    /// Practice, Qualify, Race and so on.
    #[serde(default)]
    pub session_type: String,
    /// Local start, `YYYY-MM-DDTHH:MM:SS`.
    pub started_at: String,
    pub seconds: f64,
    pub distance_m: f64,
    pub max_speed: f64,
    pub crashes: u32,
    pub offroad: u32,
    pub jumps: u32,
    pub longest_airborne: f64,
    pub total_airborne: f64,
    pub front_lockups: u32,
    pub rear_lockups: u32,
    pub wheelspins: u32,
    /// Seconds of hard cornering that were judged, and how long of it was under- or oversteer.
    #[serde(default)]
    pub cornering_seconds: f64,
    #[serde(default)]
    pub understeer_seconds: f64,
    #[serde(default)]
    pub oversteer_seconds: f64,
    pub laps: Vec<RecordedLap>,
    pub best_lap_ms: Option<i32>,
    #[serde(default)]
    pub hotspots: Vec<Hotspot>,
    /// Unix seconds, to match the collision log.
    #[serde(default)]
    pub started_unix: i64,
    #[serde(default)]
    pub ended_unix: i64,
    /// Hits logged by the ACXS Collisions app, zero without it.
    #[serde(default)]
    pub wall_hits: u32,
    #[serde(default)]
    pub car_hits: u32,
    /// Average world position per slice of the lap as `(slice, x, z)`, enough to draw the track.
    #[serde(default)]
    pub path: Vec<(u8, f32, f32)>,
    /// Positions every few metres as `(x, z)`: the drawn line on tracks without lap position.
    #[serde(default)]
    pub trace: Vec<(f32, f32)>,
    #[serde(default)]
    pub event_points: Vec<EventPoint>,
    /// What the balance check saw, to explain a missing result.
    #[serde(default)]
    pub balance_samples: u32,
    #[serde(default)]
    pub max_steer: f32,
    #[serde(default)]
    pub max_yaw: f32,
    #[serde(default)]
    pub max_lateral_g: f32,
}

impl RecordedSession {
    /// Track id in the Content Manager style, `track/layout`.
    pub fn full_track_id(&self) -> String {
        if self.track_layout.is_empty() { self.track_id.clone() } else { format!("{}/{}", self.track_id, self.track_layout) }
    }

    /// The shape the Home statistics use.
    pub fn to_driven(&self) -> DrivenSession {
        DrivenSession {
            car_id: self.car_id.clone(),
            track_id: self.full_track_id(),
            date: self.started_at.get(..10).unwrap_or_default().to_string(),
            seconds: self.seconds,
            crashes: self.crashes,
            offroad: self.offroad,
            max_speed: self.max_speed,
            distance_m: self.distance_m,
            longest_airborne: self.longest_airborne,
            best_lap_ms: self.best_lap_ms.filter(|ms| *ms > 0).map(|ms| ms as u32),
            lockups: self.front_lockups + self.rear_lockups,
            wheelspins: self.wheelspins,
            cornering_seconds: self.cornering_seconds,
            understeer_seconds: self.understeer_seconds,
            oversteer_seconds: self.oversteer_seconds,
        }
    }
}

/// Lateral g above which a corner is judged for balance.
const BALANCE_MIN_G: f32 = 0.7;
/// Beyond this no tyre is cornering: it is an impact or a physics spike.
const BALANCE_MAX_G: f32 = 5.0;
/// Speed band, km/h, for learning the steering-to-yaw gain at low load.
const CAL_SPEED: (f32, f32) = (15.0, 120.0);
/// Lateral g below which the car is assumed to steer neutrally while calibrating.
const CAL_MAX_G: f32 = 0.4;
/// Samples needed before the gain is trusted.
const CAL_SAMPLES: u32 = 100;
/// Yaw ratio below this is understeer, above [`OVERSTEER_RATIO`] is oversteer.
const UNDERSTEER_RATIO: f64 = 0.8;
const OVERSTEER_RATIO: f64 = 1.25;

/// Judges understeer and oversteer without any car data.
///
/// At low speed and low lateral load a car follows the steering geometry, so
/// `yaw_rate / speed = gain * steer_angle` holds whatever the wheelbase and steering ratio are.
/// The gain is fitted by least squares from such samples. In a hard corner the measured yaw
/// rate is compared with what that gain predicts: well below means the front gave up
/// (understeer), well above means the rear rotated (oversteer).
#[derive(Debug, Default)]
pub struct Balance {
    sum_xy: f64,
    sum_xx: f64,
    samples: u32,
}

/// Verdict for one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handling {
    Neutral,
    Understeer,
    Oversteer,
}

impl Balance {
    /// Calibration samples gathered so far.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Feeds one sample. Returns a verdict only for hard cornering once the gain is known.
    pub fn sample(&mut self, speed_kmh: f32, steer: f32, yaw_rate: f32, lateral_g: f32) -> Option<Handling> {
        let v = f64::from(speed_kmh) / 3.6;
        if v < 1.0 || steer.abs() < 1e-3 {
            return None;
        }
        let (x, y) = (f64::from(steer), f64::from(yaw_rate) / v);
        if (CAL_SPEED.0..=CAL_SPEED.1).contains(&speed_kmh) && lateral_g.abs() < CAL_MAX_G && steer.abs() > 0.01 {
            self.sum_xy += x * y;
            self.sum_xx += x * x;
            self.samples = self.samples.saturating_add(1);
        }
        if self.samples < CAL_SAMPLES || self.sum_xx <= 0.0 || speed_kmh < 60.0 || !(BALANCE_MIN_G..=BALANCE_MAX_G).contains(&lateral_g.abs()) {
            return None;
        }
        let gain = self.sum_xy / self.sum_xx;
        if gain.abs() < 1e-6 {
            return None;
        }
        let ratio = y / (gain * x);
        // Yaw against the steering is counter-steering: the rear has already stepped out.
        Some(if ratio < 0.0 || ratio > OVERSTEER_RATIO {
            Handling::Oversteer
        } else if ratio < UNDERSTEER_RATIO {
            Handling::Understeer
        } else {
            Handling::Neutral
        })
    }
}

/// A rising-edge detector that ignores repeats within [`EVENT_GAP`] seconds.
#[derive(Debug, Default)]
struct Debounce {
    last: Option<f64>,
}

impl Debounce {
    fn fire(&mut self, now: f64) -> bool {
        if self.last.is_some_and(|t| now - t < EVENT_GAP) {
            self.last = Some(now);
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// Folds snapshots into a [`RecordedSession`].
#[derive(Debug, Default)]
pub struct SessionAggregator {
    session: RecordedSession,
    clock: f64,
    /// Recent `(speed, horizontal g, brake)` samples.
    speeds: VecDeque<(f32, f32, f32)>,
    crash: Debounce,
    front_lock: Debounce,
    rear_lock: Debounce,
    spin: Debounce,
    air_for: f64,
    air_counted: bool,
    tyres_out: bool,
    last_completed: Option<i32>,
    balance: Balance,
    hot: std::collections::BTreeMap<(u8, EventKind), f64>,
    path_sum: Vec<[f64; 3]>,
    trace_gap: f64,
    last_xz: Option<(f32, f32)>,
    points: std::collections::BTreeMap<(EventKind, i32, i32), (f32, f32, f64)>,
    started: bool,
    kind: Option<sharedmem::SessionType>,
}

impl SessionAggregator {
    pub fn new(started_at: String) -> Self {
        let started_unix = chrono::Utc::now().timestamp();
        Self { session: RecordedSession { started_at, started_unix, ..RecordedSession::default() }, ..Self::default() }
    }

    /// True when `snap` belongs to another session than the one being recorded.
    pub fn other_session(&self, snap: &Snapshot) -> bool {
        self.started
            && (self.session.car_id != snap.static_page.car_model
                || self.session.track_id != snap.static_page.track
                || self.kind != Some(snap.graphics.session))
    }

    pub fn seconds(&self) -> f64 {
        self.session.seconds
    }

    /// Adds one sample. `dt` is the time since the previous sample in seconds.
    pub fn feed(&mut self, snap: &Snapshot, dt: f64) {
        if snap.graphics.status != AcStatus::Live || snap.physics.is_ai_controlled != 0 {
            return;
        }
        let dt = dt.clamp(0.0, MAX_STEP);
        let (p, g, s) = (&snap.physics, &snap.graphics, &snap.static_page);
        if !self.started {
            self.started = true;
            self.session.car_id = s.car_model.clone();
            self.session.skin = s.car_skin.clone();
            self.session.track_id = s.track.clone();
            self.session.track_layout = s.track_configuration.clone();
            self.session.session_type = session_label(g.session);
            self.kind = Some(g.session);
        }
        self.clock += dt;
        let speed = f64::from(p.speed_kmh.max(0.0));
        self.session.seconds += dt;
        self.session.distance_m += speed / 3.6 * dt;
        self.session.max_speed = self.session.max_speed.max(speed);

        let before = self.event_counts();
        self.track_laps(g.completed_laps, g.last_lap_ms, g.best_lap_ms);
        self.track_tyres_out(p.number_of_tyres_out);
        self.track_airborne(&p.wheel_load, dt);
        self.track_crash(p.speed_kmh, &p.g_forces, p.brake);
        self.track_slip(p, s.tyre_radius, speed);
        let handling = self.balance.sample(p.speed_kmh, p.steer_angle, p.local_angular_velocity[1], p.g_forces[0]);
        let segment = (g.normalized_car_position.clamp(0.0, 0.9999) * SEGMENTS as f32) as u8;
        if self.path_sum.is_empty() {
            self.path_sum = vec![[0.0; 3]; SEGMENTS];
        }
        let cell = &mut self.path_sum[usize::from(segment)];
        cell[0] += f64::from(g.car_coordinates[0]);
        cell[1] += f64::from(g.car_coordinates[2]);
        cell[2] += 1.0;
        let xz = (g.car_coordinates[0], g.car_coordinates[2]);
        let here = (xz.0.is_finite() && xz.1.is_finite()).then_some(xz);
        self.add_trace(here, speed / 3.6 * dt);
        let after = self.event_counts();
        for (i, kind) in COUNTED.iter().enumerate() {
            if after[i] > before[i] {
                let n = after[i] - before[i];
                *self.hot.entry((segment, *kind)).or_default() += f64::from(n);
                self.add_point(*kind, here, f64::from(n));
            }
        }
        self.session.max_steer = self.session.max_steer.max(p.steer_angle.abs());
        self.session.max_yaw = self.session.max_yaw.max(p.local_angular_velocity[1].abs());
        if p.g_forces[0].abs() <= BALANCE_MAX_G {
            self.session.max_lateral_g = self.session.max_lateral_g.max(p.g_forces[0].abs());
        }
        self.session.balance_samples = self.balance.samples();
        if let Some(h) = handling {
            self.session.cornering_seconds += dt;
            match h {
                Handling::Understeer => {
                    self.session.understeer_seconds += dt;
                    *self.hot.entry((segment, EventKind::Understeer)).or_default() += dt * 10.0;
                    self.add_point(EventKind::Understeer, here, dt * 10.0);
                }
                Handling::Oversteer => {
                    self.session.oversteer_seconds += dt;
                    *self.hot.entry((segment, EventKind::Oversteer)).or_default() += dt * 10.0;
                    self.add_point(EventKind::Oversteer, here, dt * 10.0);
                }
                Handling::Neutral => {}
            }
        }
    }

    /// Adds a point to the trace once the car has moved [`TRACE_STEP_M`] since the last one.
    fn add_trace(&mut self, here: Option<(f32, f32)>, metres: f64) {
        let Some(here) = here else { return };
        self.trace_gap += metres;
        if (self.last_xz.is_none() || self.trace_gap >= TRACE_STEP_M) && self.session.trace.len() < TRACE_MAX {
            self.session.trace.push(here);
            self.trace_gap = 0.0;
        }
        self.last_xz = Some(here);
    }

    fn add_point(&mut self, kind: EventKind, here: Option<(f32, f32)>, amount: f64) {
        let Some((x, z)) = here else { return };
        let key = (kind, (x / EVENT_CELL_M).floor() as i32, (z / EVENT_CELL_M).floor() as i32);
        self.points.entry(key).or_insert((x, z, 0.0)).2 += amount;
    }

    fn event_counts(&self) -> [u32; 6] {
        let s = &self.session;
        [s.crashes, s.offroad, s.front_lockups, s.rear_lockups, s.wheelspins, s.jumps]
    }

    fn track_laps(&mut self, completed: i32, last_ms: i32, best_ms: i32) {
        if self.last_completed.is_some_and(|prev| completed > prev) && last_ms > 0 {
            self.session.laps.push(RecordedLap { number: completed, time_ms: last_ms });
        }
        self.last_completed = Some(completed);
        if best_ms > 0 {
            self.session.best_lap_ms = Some(self.session.best_lap_ms.map_or(best_ms, |b| b.min(best_ms)));
        }
    }

    fn track_tyres_out(&mut self, out: i32) {
        let off = out >= 3;
        if off && !self.tyres_out {
            self.session.offroad += 1;
        }
        self.tyres_out = off;
    }

    fn track_airborne(&mut self, load: &[f32; 4], dt: f64) {
        if load.iter().all(|l| *l < AIRBORNE_LOAD) {
            self.air_for += dt;
            if self.air_for >= MIN_AIRBORNE {
                if !self.air_counted {
                    self.air_counted = true;
                    self.session.jumps += 1;
                    // Credit the time spent before the threshold was reached.
                    self.session.total_airborne += self.air_for - dt;
                }
                self.session.total_airborne += dt;
                self.session.longest_airborne = self.session.longest_airborne.max(self.air_for);
            }
        } else {
            self.air_for = 0.0;
            self.air_counted = false;
        }
    }

    /// An impact shows as a horizontal g spike first and the lost speed just after, so both are
    /// looked for across the window. Kerbs and landings are vertical and do not count. With the
    /// brake pressed a far higher spike is needed, because hard braking alone reaches about 3 g.
    fn track_crash(&mut self, speed: f32, g_forces: &[f32; 3], brake: f32) {
        if self.speeds.len() == CRASH_WINDOW {
            self.speeds.pop_front();
        }
        let horizontal = g_forces[0].hypot(g_forces[2]);
        self.speeds.push_back((speed, if horizontal.is_finite() { horizontal } else { 0.0 }, brake));
        let top = self.speeds.iter().fold(0.0_f32, |m, s| m.max(s.0));
        let peak_g = self.speeds.iter().fold(0.0_f32, |m, s| m.max(s.1));
        let braked = self.speeds.iter().any(|s| s.2 >= CRASH_BRAKE);
        let needed_g = if braked { CRASH_G_BRAKING } else { CRASH_G };
        if peak_g >= needed_g && top - speed >= CRASH_SPEED_LOSS && self.crash.fire(self.clock) {
            self.session.crashes += 1;
        }
    }

    fn track_slip(&mut self, p: &sharedmem::PhysicsPage, radius: [f32; 4], speed_kmh: f64) {
        let v = (speed_kmh / 3.6) as f32;
        if v * 3.6 < 40.0 {
            return;
        }
        // Rolling speed of each wheel in m/s from its angular speed.
        let roll: Vec<f32> = (0..4).map(|i| (p.wheel_angular_speed[i] * radius[i]).abs()).collect();
        if p.brake > 0.6 {
            if roll[..2].iter().any(|r| *r < 0.7 * v) && self.front_lock.fire(self.clock) {
                self.session.front_lockups += 1;
            }
            if roll[2..].iter().any(|r| *r < 0.7 * v) && self.rear_lock.fire(self.clock) {
                self.session.rear_lockups += 1;
            }
        } else if p.throttle > 0.6 && roll[2..].iter().any(|r| *r > 1.3 * v) && self.spin.fire(self.clock) {
            self.session.wheelspins += 1;
        }
    }

    /// The session so far, or `None` when it was too short to keep.
    pub fn finish(mut self) -> Option<RecordedSession> {
        if !self.started || self.session.seconds < MIN_SESSION_SECONDS {
            return None;
        }
        self.session.path = self
            .path_sum
            .iter()
            .enumerate()
            .filter(|(_, c)| c[2] > 0.0)
            .map(|(i, c)| (i as u8, (c[0] / c[2]) as f32, (c[1] / c[2]) as f32))
            .collect();
        self.session.hotspots = self
            .hot
            .iter()
            .map(|(&(segment, kind), &value)| Hotspot { segment, kind, count: value.round() as u32 })
            .filter(|h| h.count > 0)
            .collect();
        self.session.event_points = self
            .points
            .iter()
            .map(|(&(kind, _, _), &(x, z, amount))| EventPoint { kind, x, z, count: amount.round() as u32 })
            .filter(|p| p.count > 0)
            .collect();
        Some(self.session)
    }
}

/// `%APPDATA%\ACXS\sessions`.
pub fn sessions_dir() -> PathBuf {
    crate::settings::settings_path().parent().map_or_else(|| PathBuf::from("sessions"), |dir| dir.join("sessions"))
}

/// Writes the session as `<started_at>-<car>.json` and returns the path.
pub fn save(dir: &Path, session: &RecordedSession) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp: String = session.started_at.chars().filter(char::is_ascii_digit).collect();
    let car: String = session.car_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    let path = dir.join(format!("{stamp}-{car}.json"));
    let json = serde_json::to_string(session).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Loads every readable session file in `dir`, oldest first. Unreadable files are skipped.
pub fn load_all(dir: &Path) -> Vec<RecordedSession> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut sessions: Vec<RecordedSession> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect();
    sessions.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    sessions
}

/// Shared switches for the recorder thread.
#[derive(Debug, Default)]
pub struct RecorderControl {
    /// True while a session is being recorded.
    pub recording: AtomicBool,
    /// Set to stop recording new sessions.
    pub paused: AtomicBool,
    /// Sessions saved since the app started.
    pub saved: std::sync::atomic::AtomicUsize,
}

/// Starts the recorder thread. It lives for the rest of the process.
pub fn spawn(control: Arc<RecorderControl>, on_saved: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("session-recorder".into())
        .spawn(move || {
            let counter = Arc::clone(&control);
            run(&control, &|| {
                counter.saved.fetch_add(1, Ordering::Relaxed);
                on_saved();
            });
        })
        .ok();
}

fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Decides from consecutive snapshots whether the game is really running a session.
///
/// The game's shared memory can outlive the game (the launcher holds it) with its last values
/// frozen, even `Live`. Only a physics packet counter that keeps moving counts as driving.
#[derive(Debug, Default)]
pub struct Watch {
    last_packet: Option<i32>,
    idle: f64,
}

impl Watch {
    /// Feeds one reading and returns whether the car is being driven right now.
    pub fn observe(&mut self, snap: Option<&Snapshot>, dt: f64) -> bool {
        let Some(snap) = snap else {
            self.last_packet = None;
            self.idle += dt;
            return false;
        };
        let packet = snap.physics.packet_id;
        let moved = self.last_packet.is_some_and(|last| last != packet);
        self.last_packet = Some(packet);
        let driving = snap.graphics.status == AcStatus::Live && moved;
        // A paused game stops the counter; that is not the end of the session.
        if driving || snap.graphics.status == AcStatus::Pause {
            self.idle = 0.0;
        } else {
            self.idle += dt;
        }
        driving
    }

    /// True once nothing has been driven for [`GONE_AFTER`] seconds.
    pub fn gone(&self) -> bool {
        self.idle >= GONE_AFTER
    }
}

fn run(control: &RecorderControl, on_saved: &dyn Fn()) {
    let dir = sessions_dir();
    let mut agg: Option<SessionAggregator> = None;
    let mut last = Instant::now();
    let mut watch = Watch::default();
    let mut last_error: Option<sharedmem::SharedMemError> = None;
    loop {
        let reading = if control.paused.load(Ordering::Relaxed) { Err(sharedmem::SharedMemError::Unsupported) } else { sharedmem::read_snapshot() };
        if let Err(err) = &reading
            && !matches!(err, sharedmem::SharedMemError::NotRunning { .. } | sharedmem::SharedMemError::Unsupported)
            && last_error.as_ref() != Some(err)
        {
            log_line(&format!("shared memory: {err}"));
            last_error = Some(err.clone());
        }
        let snap = reading.ok();
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64();
        last = now;
        let driving = watch.observe(snap.as_ref(), dt);
        if let Some(snap) = &snap {
            let changed = agg.as_ref().is_some_and(|a| a.other_session(snap));
            if changed {
                close(agg.take(), &dir, on_saved);
            }
            if driving && agg.is_none() {
                log_line(&format!("session started: {} on {}", snap.static_page.car_model, snap.static_page.track));
                agg = Some(SessionAggregator::new(now_stamp()));
            }
            if driving && let Some(a) = agg.as_mut() {
                a.feed(snap, dt);
            }
        }
        if watch.gone() {
            close(agg.take(), &dir, on_saved);
        }
        control.recording.store(agg.is_some(), Ordering::Relaxed);
        std::thread::sleep(if agg.is_some() { SAMPLE_PERIOD } else { IDLE_PERIOD });
    }
}

/// Archived replays kept; older ones are deleted from the ACXS archive (never from the game's folder).
const KEEP_REPLAYS: usize = 20;

/// `%APPDATA%\ACXS\replays`.
pub fn replay_archive_dir() -> PathBuf {
    sessions_dir().parent().map_or_else(|| PathBuf::from("replays"), |dir| dir.join("replays"))
}

/// Copies replays from `source` that were written at or after `since` into `archive`, skipping
/// ones already there, then trims the archive to [`KEEP_REPLAYS`]. Returns the number copied.
pub fn archive_replays(source: &Path, archive: &Path, since: std::time::SystemTime) -> std::io::Result<usize> {
    let mut copied = 0;
    for entry in std::fs::read_dir(source)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|x| !x.eq_ignore_ascii_case("acreplay")) {
            continue;
        }
        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        let Some(name) = path.file_name() else { continue };
        if modified < since || archive.join(name).exists() {
            continue;
        }
        std::fs::create_dir_all(archive)?;
        std::fs::copy(&path, archive.join(name))?;
        copied += 1;
    }
    if let Ok(read) = std::fs::read_dir(archive) {
        let mut files: Vec<_> = read
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("acreplay")))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort();
        let excess = files.len().saturating_sub(KEEP_REPLAYS);
        for (_, path) in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(copied)
}

fn close(agg: Option<SessionAggregator>, dir: &Path, on_saved: &dyn Fn()) {
    if let Some(mut session) = agg.and_then(SessionAggregator::finish) {
        session.ended_unix = chrono::Utc::now().timestamp();
        log_line(&format!("session ended: {} on {}, {:.0} s", session.car_id, session.track_id, session.seconds));
        let hits = crate::settings::Settings::load(&crate::settings::settings_path())
            .ac_root_path()
            .map(|root| crate::collision_app::collisions_between(&root, session.started_unix - 1, session.ended_unix + 5))
            .unwrap_or_default();
        attach_collisions(&mut session, &hits);
        if save(dir, &session).is_ok() {
            if let Some(docs) = crate::settings::ac_documents_dir() {
                // The game finishes writing the replay when the session ends; a start window of
                // the session length plus a minute catches it and nothing older.
                let since = std::time::SystemTime::now() - Duration::from_secs_f64(session.seconds + 60.0);
                let _ = archive_replays(&docs.join("replay").join("temp"), &replay_archive_dir(), since);
            }
            on_saved();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    fn snap() -> Snapshot {
        let mut physics = sharedmem::parse_physics(&[0u8; sharedmem::PHYSICS_PAGE_LEN]).unwrap();
        physics.wheel_load = [3000.0; 4];
        physics.wheel_angular_speed = [100.0; 4];
        let mut graphics = sharedmem::parse_graphics(&[0u8; sharedmem::GRAPHICS_PAGE_LEN]).unwrap();
        graphics.status = AcStatus::Live;
        let mut static_page = sharedmem::parse_static(&[0u8; sharedmem::STATIC_PAGE_LEN]).unwrap();
        static_page.car_model = "bmw_m3_e92".into();
        static_page.track = "ks_nordschleife".into();
        static_page.tyre_radius = [0.3; 4];
        Snapshot { physics, graphics, static_page }
    }


    #[test]
    fn accumulates_time_distance_and_top_speed() {
        let mut agg = SessionAggregator::new("2026-11-01T10:00:00".into());
        let mut s = snap();
        s.physics.speed_kmh = 100.0;
        s.physics.wheel_angular_speed = [92.6; 4];
        for _ in 0..600 {
            agg.feed(&s, 0.05);
        }
        s.physics.speed_kmh = 180.0;
        s.physics.wheel_angular_speed = [166.6; 4];
        agg.feed(&s, 0.05);
        let done = agg.finish().expect("long enough");
        assert!((done.seconds - 30.05).abs() < 1e-6);
        assert_eq!(done.max_speed, 180.0);
        assert!((done.distance_m - (100.0 / 3.6 * 30.0 + 180.0 / 3.6 * 0.05)).abs() < 1e-3);
        assert_eq!(done.car_id, "bmw_m3_e92");
    }

    #[test]
    fn short_or_paused_sessions_are_dropped() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.physics.speed_kmh = 50.0;
        agg.feed(&s, 0.1);
        s.graphics.status = AcStatus::Pause;
        for _ in 0..1000 {
            agg.feed(&s, 0.1);
        }
        assert!(agg.finish().is_none());
    }

    #[test]
    fn counts_a_jump_once_and_measures_it() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.physics.speed_kmh = 200.0;
        s.physics.wheel_angular_speed = [185.0; 4];
        for _ in 0..400 {
            agg.feed(&s, 0.1);
        }
        s.physics.wheel_load = [0.0; 4];
        for _ in 0..20 {
            agg.feed(&s, 0.1);
        }
        s.physics.wheel_load = [3000.0; 4];
        agg.feed(&s, 0.1);
        s.physics.wheel_load = [0.0, 3000.0, 0.0, 0.0];
        for _ in 0..20 {
            agg.feed(&s, 0.1);
        }
        let done = agg.finish().unwrap();
        assert_eq!(done.jumps, 1);
        assert!((done.longest_airborne - 2.0).abs() < 1e-6);
        assert!((done.total_airborne - 2.0).abs() < 1e-6);
    }

    #[test]
    fn crash_needs_both_g_and_speed_loss_and_is_debounced() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.physics.speed_kmh = 150.0;
        s.physics.wheel_angular_speed = [138.0; 4];
        for _ in 0..400 {
            agg.feed(&s, 0.1);
        }
        // Hard braking: speed loss with a spike below the braking limit.
        s.physics.speed_kmh = 100.0;
        s.physics.brake = 1.0;
        s.physics.g_forces = [0.0, 0.0, 3.0];
        agg.feed(&s, 0.1);
        assert_eq!(agg.session.crashes, 0);
        // Wall: a g spike with the speed collapsing, twice within the gap.
        s.physics.speed_kmh = 20.0;
        s.physics.g_forces = [6.0, 0.0, 0.0];
        agg.feed(&s, 0.1);
        agg.feed(&s, 0.1);
        assert_eq!(agg.session.crashes, 1);
    }

    #[test]
    fn detects_front_lockup_and_offroad_edges() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.physics.speed_kmh = 120.0;
        s.physics.brake = 1.0;
        s.physics.wheel_angular_speed = [30.0, 30.0, 110.0, 110.0];
        agg.feed(&s, 0.1);
        assert_eq!(agg.session.front_lockups, 1);
        assert_eq!(agg.session.rear_lockups, 0);
        s.physics.number_of_tyres_out = 4;
        agg.feed(&s, 0.1);
        agg.feed(&s, 0.1);
        s.physics.number_of_tyres_out = 0;
        agg.feed(&s, 0.1);
        s.physics.number_of_tyres_out = 3;
        agg.feed(&s, 0.1);
        assert_eq!(agg.session.offroad, 2);
    }

    #[test]
    fn laps_are_recorded_when_the_counter_advances() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.graphics.completed_laps = 0;
        agg.feed(&s, 0.1);
        s.graphics.completed_laps = 1;
        s.graphics.last_lap_ms = 95_000;
        s.graphics.best_lap_ms = 95_000;
        agg.feed(&s, 0.1);
        assert_eq!(agg.session.laps, vec![RecordedLap { number: 1, time_ms: 95_000 }]);
        assert_eq!(agg.session.best_lap_ms, Some(95_000));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("acxs-rec-test-{}", std::process::id()));
        let session = RecordedSession {
            car_id: "a_car".into(),
            track_id: "a_track".into(),
            track_layout: "gp".into(),
            started_at: "2026-11-01T10:00:00".into(),
            seconds: 61.0,
            ..RecordedSession::default()
        };
        save(&dir, &session).unwrap();
        let loaded = load_all(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(loaded, vec![session.clone()]);
        assert_eq!(session.to_driven().track_id, "a_track/gp");
        assert_eq!(session.to_driven().date, "2026-11-01");
    }

    #[test]
    fn balance_learns_gain_then_flags_understeer_and_oversteer() {
        let mut b = Balance::default();
        // Low speed, gain 0.1 (yaw/v per unit steer).
        for i in 0..300 {
            let steer = 0.1 + (i % 5) as f32 * 0.05;
            let v_kmh = 50.0;
            let yaw = 0.1 * steer * (v_kmh / 3.6);
            assert_eq!(b.sample(v_kmh, steer, yaw, 0.1), None);
        }
        let v = 100.0_f32;
        let ideal = 0.1 * 0.3 * (v / 3.6);
        assert_eq!(b.sample(v, 0.3, ideal, 1.2), Some(Handling::Neutral));
        assert_eq!(b.sample(v, 0.3, ideal * 0.6, 1.2), Some(Handling::Understeer));
        assert_eq!(b.sample(v, 0.3, ideal * 1.5, 1.2), Some(Handling::Oversteer));
        // Too gentle a corner is not judged.
        assert_eq!(b.sample(v, 0.3, ideal * 0.6, 0.3), None);
    }
}

/// A stretch of the lap where something kept happening.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// Lap position 0..1 where it starts and ends.
    pub start: f32,
    pub end: f32,
    pub kind: EventKind,
    /// Occurrences, or tenths of a second for understeer and oversteer.
    pub count: u32,
}

impl EventKind {
    /// True for the kinds measured in tenths of a second.
    pub fn is_timed(self) -> bool {
        matches!(self, EventKind::Understeer | EventKind::Oversteer)
    }
}

impl RecordedSession {
    /// The `limit` worst spots. Neighbouring slices of the same kind are merged into one stretch.
    /// Ranking counts one crash or lock-up like two seconds of understeer, so single events are
    /// not buried under long balance stretches.
    pub fn worst_spots(&self, limit: usize) -> Vec<Spot> {
        let mut cells: Vec<&Hotspot> = self.hotspots.iter().collect();
        cells.sort_by_key(|h| (h.kind, h.segment));
        let mut spots: Vec<(u32, Spot)> = Vec::new();
        let width = 1.0 / SEGMENTS as f32;
        let mut last_segment = 0u8;
        for h in cells {
            let start = f32::from(h.segment) * width;
            match spots.last_mut() {
                Some((_, spot)) if spot.kind == h.kind && h.segment <= last_segment + 1 => {
                    spot.end = start + width;
                    spot.count += h.count;
                }
                _ => spots.push((0, Spot { start, end: start + width, kind: h.kind, count: h.count })),
            }
            last_segment = h.segment;
        }
        for (score, spot) in &mut spots {
            *score = if spot.kind.is_timed() { spot.count } else { spot.count * 20 };
        }
        spots.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.start.total_cmp(&b.1.start)));
        spots.into_iter().take(limit).map(|(_, spot)| spot).collect()
    }
}

#[cfg(test)]
mod hotspot_tests {
    use super::*;

    #[test]
    fn events_are_placed_by_track_position() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = {
            let mut physics = sharedmem::parse_physics(&[0u8; sharedmem::PHYSICS_PAGE_LEN]).unwrap();
            physics.speed_kmh = 120.0;
            physics.brake = 1.0;
            physics.wheel_angular_speed = [30.0, 30.0, 110.0, 110.0];
            physics.wheel_load = [3000.0; 4];
            let mut graphics = sharedmem::parse_graphics(&[0u8; sharedmem::GRAPHICS_PAGE_LEN]).unwrap();
            graphics.status = AcStatus::Live;
            let mut static_page = sharedmem::parse_static(&[0u8; sharedmem::STATIC_PAGE_LEN]).unwrap();
            static_page.tyre_radius = [0.3; 4];
            Snapshot { physics, graphics, static_page }
        };
        s.graphics.car_coordinates = [10.0, 0.0, -4.0];
        s.graphics.normalized_car_position = 0.425;
        agg.feed(&s, 0.1);
        for _ in 0..400 {
            s.physics.brake = 0.0;
            s.physics.wheel_angular_speed = [100.0; 4];
            s.graphics.normalized_car_position = 0.9;
            agg.feed(&s, 0.1);
        }
        let done = agg.finish().unwrap();
        assert_eq!(done.hotspots, vec![Hotspot { segment: 42, kind: EventKind::FrontLockup, count: 1 }]);
        assert!(done.path.contains(&(42, 10.0, -4.0)));
        let worst = done.worst_spots(3);
        assert_eq!(worst.len(), 1);
        assert_eq!(worst[0].kind, EventKind::FrontLockup);
        assert!((worst[0].start - 0.42).abs() < 1e-6 && (worst[0].end - 0.43).abs() < 1e-6);
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;

    #[test]
    fn copies_new_replays_once_and_trims_the_archive() {
        let base = std::env::temp_dir().join(format!("acxs-arch-test-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.acreplay"), b"1").unwrap();
        std::fs::write(src.join("notes.txt"), b"x").unwrap();
        let long_ago = std::time::UNIX_EPOCH;
        assert_eq!(archive_replays(&src, &dst, long_ago).unwrap(), 1);
        assert_eq!(archive_replays(&src, &dst, long_ago).unwrap(), 0);
        assert!(dst.join("a.acreplay").exists() && !dst.join("notes.txt").exists());
        // Nothing older than `since` is taken.
        let future = std::time::SystemTime::now() + Duration::from_secs(3600);
        std::fs::write(src.join("b.acreplay"), b"2").unwrap();
        assert_eq!(archive_replays(&src, &dst, future).unwrap(), 0);
        for i in 0..25 {
            std::fs::write(dst.join(format!("old{i}.acreplay")), b"z").unwrap();
        }
        archive_replays(&src, &dst, future).unwrap();
        let left = std::fs::read_dir(&dst).unwrap().count();
        std::fs::remove_dir_all(&base).ok();
        assert_eq!(left, KEEP_REPLAYS);
    }
}

/// Replays the game keeps per session type after [`keep_more_replays`].
pub const RACE_KEEP: i64 = 10;
pub const QUALIFY_KEEP: i64 = 5;
pub const OTHERS_KEEP: i64 = 10;

/// Raises the game's autosave limits in `cfg\replay.ini` so replays are not thrown away so soon.
/// The old file is kept as a backup. Returns the file written.
pub fn keep_more_replays() -> std::io::Result<PathBuf> {
    let cfg = crate::settings::ac_documents_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Documents folder not found"))?
        .join("cfg")
        .join("replay.ini");
    let mut ini = crate::ini::Ini::load(&cfg)?;
    raise_replay_limits(&mut ini);
    ini.save_with_backup(&cfg)?;
    Ok(cfg)
}

/// Sets the autosave counts to at least the `*_KEEP` values; larger values and `-1` stay.
pub fn raise_replay_limits(ini: &mut crate::ini::Ini) {
    ini.set("AUTOSAVE", "ENABLED", 1);
    for (key, keep) in [("RACE", RACE_KEEP), ("QUALIFY", QUALIFY_KEEP), ("OTHERS", OTHERS_KEEP)] {
        let current = ini.get_i64("AUTOSAVE", key).unwrap_or(0);
        if current != -1 && current < keep {
            ini.set("AUTOSAVE", key, keep);
        }
    }
}

#[cfg(test)]
mod replay_cfg_tests {
    use super::*;

    #[test]
    fn limits_are_raised_but_never_lowered() {
        let mut ini = crate::ini::Ini::parse("[AUTOSAVE]\nENABLED=1 ;x\nRACE=2 ;max\nQUALIFY=-1\nOTHERS=40\n");
        raise_replay_limits(&mut ini);
        assert_eq!(ini.get_i64("AUTOSAVE", "RACE"), Some(RACE_KEEP));
        assert_eq!(ini.get_i64("AUTOSAVE", "QUALIFY"), Some(-1));
        assert_eq!(ini.get_i64("AUTOSAVE", "OTHERS"), Some(40));
    }
}

/// Adds logged collisions to a finished session: the two counters and one hotspot per slice.
pub fn attach_collisions(session: &mut RecordedSession, hits: &[crate::collision_app::Collision]) {
    use crate::collision_app::HitKind;
    for hit in hits {
        let kind = match hit.kind {
            HitKind::Wall => {
                session.wall_hits += 1;
                EventKind::WallHit
            }
            HitKind::Car => {
                session.car_hits += 1;
                EventKind::CarHit
            }
        };
        let segment = (hit.position.clamp(0.0, 0.9999) * SEGMENTS as f32) as u8;
        match session.hotspots.iter_mut().find(|h| h.segment == segment && h.kind == kind) {
            Some(spot) => spot.count += 1,
            None => session.hotspots.push(Hotspot { segment, kind, count: 1 }),
        }
    }
}

#[cfg(test)]
mod collision_tests {
    use super::*;
    use crate::collision_app::{Collision, HitKind};

    #[test]
    fn collisions_are_counted_and_placed() {
        let mut s = RecordedSession::default();
        let hit = |kind, position| Collision { unix: 0, kind, speed_kmh: 90.0, position };
        attach_collisions(&mut s, &[hit(HitKind::Wall, 0.5), hit(HitKind::Wall, 0.505), hit(HitKind::Car, 0.1)]);
        assert_eq!((s.wall_hits, s.car_hits), (2, 1));
        assert!(s.hotspots.contains(&Hotspot { segment: 50, kind: EventKind::WallHit, count: 2 }));
        assert!(s.hotspots.contains(&Hotspot { segment: 10, kind: EventKind::CarHit, count: 1 }));
    }
}

#[cfg(test)]
mod spot_tests {
    use super::*;

    #[test]
    fn neighbours_merge_and_single_events_outrank_balance_cells() {
        let cell = |segment, kind, count| Hotspot { segment, kind, count };
        let s = RecordedSession {
            hotspots: vec![
                cell(10, EventKind::Understeer, 3),
                cell(11, EventKind::Understeer, 3),
                cell(12, EventKind::Understeer, 3),
                cell(40, EventKind::Understeer, 2),
                cell(60, EventKind::Crash, 1),
            ],
            ..RecordedSession::default()
        };
        let spots = s.worst_spots(5);
        assert_eq!(spots.len(), 3);
        assert_eq!(spots[0].kind, EventKind::Crash);
        assert_eq!((spots[1].kind, spots[1].count), (EventKind::Understeer, 9));
        assert!((spots[1].start - 0.10).abs() < 1e-6 && (spots[1].end - 0.13).abs() < 1e-6);
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    fn snap(status: i32, packet: i32) -> Snapshot {
        let mut physics = sharedmem::parse_physics(&[0u8; sharedmem::PHYSICS_PAGE_LEN]).unwrap();
        physics.packet_id = packet;
        let mut graphics = sharedmem::parse_graphics(&[0u8; sharedmem::GRAPHICS_PAGE_LEN]).unwrap();
        graphics.status = match status {
            2 => AcStatus::Live,
            3 => AcStatus::Pause,
            _ => AcStatus::Off,
        };
        let static_page = sharedmem::parse_static(&[0u8; sharedmem::STATIC_PAGE_LEN]).unwrap();
        Snapshot { physics, graphics, static_page }
    }

    #[test]
    fn a_moving_counter_is_driving_and_a_frozen_one_ends_the_session() {
        let mut w = Watch::default();
        // First reading has nothing to compare with.
        assert!(!w.observe(Some(&snap(2, 10)), 0.03));
        assert!(w.observe(Some(&snap(2, 11)), 0.03));
        assert!(!w.gone());
        // The game exited but its memory stayed, frozen on Live.
        for _ in 0..200 {
            w.observe(Some(&snap(2, 11)), 0.03);
        }
        assert!(w.gone());
        // Memory that never moves does not start a session either.
        let mut fresh = Watch::default();
        assert!(!fresh.observe(Some(&snap(2, 5)), 2.0));
        assert!(!fresh.observe(Some(&snap(2, 5)), 2.0));
    }

    #[test]
    fn pause_and_missing_memory_are_told_apart() {
        let mut w = Watch::default();
        w.observe(Some(&snap(2, 1)), 0.03);
        for _ in 0..1000 {
            w.observe(Some(&snap(3, 1)), 0.03);
        }
        assert!(!w.gone(), "a paused game is still a session");
        for _ in 0..200 {
            w.observe(None, 0.03);
        }
        assert!(w.gone());
    }
}

/// Appends a line to `%APPDATA%\ACXS\recorder.log` so a missing session can be explained. The file
/// is cut back to its newest half when it grows past 64 KB.
pub fn log_line(text: &str) {
    let Some(dir) = sessions_dir().parent().map(Path::to_path_buf) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("recorder.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 64 * 1024)
        && let Ok(old) = std::fs::read_to_string(&path)
    {
        let _ = std::fs::write(&path, &old[old.len() / 2..]);
    }
    let line = format!("{}  {text}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

fn session_label(kind: sharedmem::SessionType) -> String {
    use sharedmem::SessionType as S;
    match kind {
        S::Practice => "Practice",
        S::Qualify => "Qualifying",
        S::Race => "Race",
        S::Hotlap => "Hotlap",
        S::TimeAttack => "Time attack",
        S::Drift => "Drift",
        S::Drag => "Drag",
        S::Unknown(_) => "",
    }
    .to_string()
}

#[cfg(test)]
mod real_drive_tests {
    use super::*;

    fn snap() -> Snapshot {
        let mut physics = sharedmem::parse_physics(&[0u8; sharedmem::PHYSICS_PAGE_LEN]).unwrap();
        physics.wheel_load = [3000.0; 4];
        let mut graphics = sharedmem::parse_graphics(&[0u8; sharedmem::GRAPHICS_PAGE_LEN]).unwrap();
        graphics.status = AcStatus::Live;
        let mut static_page = sharedmem::parse_static(&[0u8; sharedmem::STATIC_PAGE_LEN]).unwrap();
        static_page.tyre_radius = [0.3; 4];
        static_page.car_model = "car".into();
        static_page.track = "track".into();
        Snapshot { physics, graphics, static_page }
    }

    /// The slow crash from the Ferrari replay: 29 to 8 km/h, coasting. The spike comes first and
    /// the speed follows a few samples later.
    #[test]
    fn a_slow_impact_counts_when_the_spike_comes_before_the_speed_loss() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.physics.speed_kmh = 29.0;
        for _ in 0..20 {
            agg.feed(&s, 0.033);
        }
        s.physics.g_forces = [3.5, 0.0, 0.0];
        agg.feed(&s, 0.033);
        s.physics.g_forces = [0.2, 0.0, 0.0];
        for speed in [24.0, 18.0, 12.0, 8.0] {
            s.physics.speed_kmh = speed;
            agg.feed(&s, 0.033);
        }
        assert_eq!(agg.session.crashes, 1);
    }

    #[test]
    fn hard_braking_kerbs_and_landings_are_not_crashes() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        s.physics.speed_kmh = 250.0;
        for _ in 0..20 {
            agg.feed(&s, 0.033);
        }
        // Three g on the brakes loses speed fast but is only a stop.
        s.physics.brake = 1.0;
        s.physics.g_forces = [0.0, 0.0, -3.0];
        for i in 0..30 {
            s.physics.speed_kmh = 250.0 - i as f32 * 3.5;
            agg.feed(&s, 0.033);
        }
        // A vertical spike from a landing, while slowing down off the throttle.
        s.physics.brake = 0.0;
        s.physics.g_forces = [0.0, 9.0, 0.0];
        for i in 0..10 {
            s.physics.speed_kmh = 140.0 - i as f32 * 2.0;
            agg.feed(&s, 0.033);
        }
        assert_eq!(agg.session.crashes, 0);
    }

    #[test]
    fn counter_steering_is_oversteer_and_spikes_are_not_judged() {
        let mut b = Balance::default();
        for _ in 0..200 {
            let yaw = 0.1 * 0.2 * (50.0 / 3.6);
            b.sample(50.0, 0.2, yaw, 0.1);
        }
        let v = 100.0_f32;
        let ideal = 0.1 * 0.3 * (v / 3.6);
        assert_eq!(b.sample(v, 0.3, -ideal, 1.2), Some(Handling::Oversteer));
        assert_eq!(b.sample(v, 0.3, ideal, 40.0), None);
    }

    #[test]
    fn a_new_session_type_or_car_is_another_session() {
        let mut agg = SessionAggregator::new(String::new());
        let mut s = snap();
        assert!(!agg.other_session(&s));
        agg.feed(&s, 0.1);
        assert!(!agg.other_session(&s));
        let mut race = snap();
        race.graphics.session = sharedmem::SessionType::Race;
        assert!(agg.other_session(&race));
        s.static_page.car_model = "other".into();
        assert!(agg.other_session(&s));
    }
}

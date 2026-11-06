//! The options of a quick race beyond track and car: sessions, conditions, rules and driving
//! assists.
//!
//! [`RaceExtras`] rides inside [`crate::race::QuickRace`] and is written into `race.ini`.
//! [`Assists`] is written to its own file, `cfg/assists.ini`, which is where the game reads
//! the driver aids from. Every value is an integer or a flag so the structs stay comparable and
//! serialisable; the meaning of each field is on the field.

use serde::{Deserialize, Serialize};

/// What kind of session Quick Race starts. The values are the game's own `[SESSION_0]` settings,
/// as its Special Events write them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RaceMode {
    /// Practice, qualifying and race, with opponents.
    #[default]
    Weekend,
    Hotlap,
    TimeAttack,
    Drift,
}

impl RaceMode {
    pub const ALL: [RaceMode; 4] = [RaceMode::Weekend, RaceMode::Hotlap, RaceMode::TimeAttack, RaceMode::Drift];

    pub fn label(self) -> &'static str {
        match self {
            RaceMode::Weekend => "Race weekend",
            RaceMode::Hotlap => "Hotlap",
            RaceMode::TimeAttack => "Time attack",
            RaceMode::Drift => "Drift",
        }
    }

    /// Time attack runs on the track's line: every layout the game uses it on has one, and drift
    /// pads and freeroam maps do not.
    pub fn needs_racing_line(self) -> bool {
        self == RaceMode::TimeAttack
    }

    /// `(session name, session type, spawn set)`, or `None` for a weekend.
    pub fn session(self) -> Option<(&'static str, u8, &'static str)> {
        match self {
            RaceMode::Weekend => None,
            RaceMode::Hotlap => Some(("Hotlap", 4, "HOTLAP_START")),
            RaceMode::TimeAttack => Some(("Time Attack", 5, "START")),
            RaceMode::Drift => Some(("Drift Session", 6, "PIT")),
        }
    }
}

/// Sessions, conditions and rules of a quick race.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RaceExtras {
    /// The kind of session; only a weekend has opponents and the session switches.
    pub mode: RaceMode,
    /// Minutes of free practice before the rest. 0 leaves practice out.
    pub practice_minutes: u32,
    /// Minutes of qualifying before the race. 0 leaves qualifying out.
    pub qualify_minutes: u32,
    /// Whether the weekend has a race at all.
    pub race_enabled: bool,
    /// A race of this many minutes instead of a lap count. 0 means use the lap count.
    pub race_minutes: u32,
    /// Start the race from the pit lane instead of the grid.
    pub pit_start: bool,

    /// Ambient temperature in degrees Celsius.
    pub ambient_temp: i32,
    /// Road temperature in degrees Celsius.
    pub road_temp: i32,
    /// Wind speed range in km/h.
    pub wind_min_kmh: u32,
    pub wind_max_kmh: u32,
    /// Direction the wind blows from, 0..=359 degrees.
    pub wind_direction: u32,
    /// How many times faster than real time the day passes. 1 is real time.
    pub time_multiplier: u32,
    /// Grip of the track at the start, in percent (100 is the game's "optimum" rubbered-in track).
    pub track_grip: u32,

    /// Penalties for cutting corners and the like.
    pub penalties: bool,
    /// Jump start handling: 0 off, 1 drive-through, 2 pit lane start.
    pub jump_start_penalty: u32,
    /// Everyone drives the same, fixed setup.
    pub fixed_setup: bool,
    /// Wheels allowed off track before a lap is invalid. -1 never invalidates.
    pub allowed_tyres_out: i32,

    /// Also write the driver aids below to `assists.ini` when the race starts.
    pub write_assists: bool,
    pub assists: Assists,
}

impl Default for RaceExtras {
    fn default() -> Self {
        RaceExtras {
            mode: RaceMode::Weekend,
            practice_minutes: 0,
            qualify_minutes: 0,
            race_enabled: true,
            race_minutes: 0,
            pit_start: false,
            ambient_temp: 26,
            road_temp: 32,
            wind_min_kmh: 0,
            wind_max_kmh: 0,
            wind_direction: 0,
            time_multiplier: 1,
            track_grip: 100,
            penalties: true,
            jump_start_penalty: 0,
            fixed_setup: false,
            allowed_tyres_out: -1,
            write_assists: false,
            assists: Assists::default(),
        }
    }
}

impl RaceExtras {
    /// Problems with these options, one sentence each.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.mode == RaceMode::Weekend && self.practice_minutes == 0 && self.qualify_minutes == 0 && !self.race_enabled {
            problems.push("Turn on at least one session: practice, qualifying or the race.".to_string());
        }
        if self.practice_minutes > 600 || self.qualify_minutes > 600 || self.race_minutes > 1440 {
            problems.push("A session cannot be longer than 10 hours (a timed race, 24).".to_string());
        }
        if !(-30..=60).contains(&self.ambient_temp) || !(-30..=90).contains(&self.road_temp) {
            problems.push("Temperatures must be realistic (ambient -30 to 60, road -30 to 90 degrees).".to_string());
        }
        if self.wind_min_kmh > self.wind_max_kmh {
            problems.push("The minimum wind speed is above the maximum.".to_string());
        }
        if self.wind_max_kmh > 100 {
            problems.push("Wind speed is limited to 100 km/h.".to_string());
        }
        if self.wind_direction > 359 {
            problems.push("Wind direction is 0 to 359 degrees.".to_string());
        }
        if !(1..=60).contains(&self.time_multiplier) {
            problems.push("The time multiplier is 1 to 60.".to_string());
        }
        if self.track_grip > 100 {
            problems.push("Track grip is at most 100 percent.".to_string());
        }
        if self.jump_start_penalty > 2 {
            problems.push("Jump start penalty is 0, 1 or 2.".to_string());
        }
        problems.extend(self.assists.validate());
        problems
    }
}

/// Driver aids and damage settings, as `assists.ini` holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Assists {
    /// The racing line painted on the road.
    pub ideal_line: bool,
    /// Automatic gear changes.
    pub auto_shifter: bool,
    /// Stability control, 0 (off) to 100 (full).
    pub stability_control: u32,
    /// Traction control: 0 off, 1 as the car has it, 2 forced on.
    pub traction_control: u32,
    /// ABS: 0 off, 1 as the car has it, 2 forced on.
    pub abs: u32,
    /// Throttle blip on downshifts.
    pub auto_blip: bool,
    /// Automatic clutch.
    pub auto_clutch: bool,
    /// Damage shown on the car body.
    pub visual_damage: bool,
    /// How much damage affects the car, 0 to 100 percent.
    pub damage: u32,
    /// Fuel use: 0 none, 1 normal, 2 double and so on.
    pub fuel_rate: u32,
    /// Tyre wear: 0 none, 1 normal, 2 double and so on.
    pub tyre_wear: u32,
    /// Tyres start warm.
    pub tyre_blankets: bool,
    /// Slipstream effect: 0 none, 1 normal, 2 double and so on.
    pub slipstream: u32,
}

impl Default for Assists {
    fn default() -> Self {
        Assists {
            ideal_line: false,
            auto_shifter: false,
            stability_control: 0,
            traction_control: 1,
            abs: 1,
            auto_blip: true,
            auto_clutch: false,
            visual_damage: true,
            damage: 100,
            fuel_rate: 1,
            tyre_wear: 1,
            tyre_blankets: false,
            slipstream: 1,
        }
    }
}

impl Assists {
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.stability_control > 100 {
            problems.push("Stability control is 0 to 100.".to_string());
        }
        if self.traction_control > 2 || self.abs > 2 {
            problems.push("Traction control and ABS are 0, 1 or 2.".to_string());
        }
        if self.damage > 100 {
            problems.push("Damage is 0 to 100 percent.".to_string());
        }
        if self.fuel_rate > 10 || self.tyre_wear > 10 || self.slipstream > 10 {
            problems.push("Fuel use, tyre wear and slipstream go up to 10 times normal.".to_string());
        }
        problems
    }

    /// The `assists.ini` text, with CRLF line ends like the game writes.
    pub fn to_ini(&self) -> String {
        let flag = |b: bool| if b { "1" } else { "0" };
        let mut out = String::from("[ASSISTS]\r\n");
        for (key, value) in [
            ("IDEAL_LINE", flag(self.ideal_line).to_string()),
            ("AUTO_SHIFTER", flag(self.auto_shifter).to_string()),
            ("STABILITY_CONTROL", self.stability_control.to_string()),
            ("TRACTION_CONTROL", self.traction_control.to_string()),
            ("ABS", self.abs.to_string()),
            ("AUTO_BLIP", flag(self.auto_blip).to_string()),
            ("AUTO_CLUTCH", flag(self.auto_clutch).to_string()),
            ("VISUAL_DAMAGE", flag(self.visual_damage).to_string()),
            ("DAMAGE", self.damage.to_string()),
            ("FUEL_RATE", self.fuel_rate.to_string()),
            ("TYRE_WEAR", self.tyre_wear.to_string()),
            ("TYRE_BLANKETS", flag(self.tyre_blankets).to_string()),
            ("SLIPSTREAM", self.slipstream.to_string()),
        ] {
            out.push_str(&format!("{key}={value}\r\n"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(RaceExtras::default().validate().is_empty());
    }

    #[test]
    fn needs_a_session() {
        let extras = RaceExtras { race_enabled: false, ..RaceExtras::default() };
        assert!(!extras.validate().is_empty());
        let extras = RaceExtras { practice_minutes: 10, race_enabled: false, ..RaceExtras::default() };
        assert!(extras.validate().is_empty());
    }

    #[test]
    fn catches_nonsense_conditions() {
        let extras = RaceExtras { wind_min_kmh: 30, wind_max_kmh: 10, ..RaceExtras::default() };
        assert!(extras.validate().iter().any(|p| p.contains("wind")));
        let extras = RaceExtras { time_multiplier: 0, ..RaceExtras::default() };
        assert!(!extras.validate().is_empty());
    }

    #[test]
    fn assists_ini_lists_every_key() {
        let ini = Assists { auto_shifter: true, traction_control: 2, ..Assists::default() }.to_ini();
        assert!(ini.starts_with("[ASSISTS]"));
        assert!(ini.contains("AUTO_SHIFTER=1"));
        assert!(ini.contains("TRACTION_CONTROL=2"));
        assert!(ini.contains("ABS=1"));
        assert!(ini.contains("AUTO_BLIP=1"));
        assert!(ini.contains("SLIPSTREAM=1"));
    }

    #[test]
    fn old_saved_races_without_extras_still_load() {
        let extras: RaceExtras = serde_json::from_str("{}").unwrap();
        assert_eq!(extras, RaceExtras::default());
    }
}

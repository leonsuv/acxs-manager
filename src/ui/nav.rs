#![allow(dead_code)]
//! 6-Pillar AAA navigation model (`Pillar`, `Route`, per-pillar sub-tabs/sub-views,
//! remembered navigation state, and keyboard/gamepad focus routing).

use eframe::egui::{self, Key};
use serde::{Deserialize, Serialize};

/// The 6 top-level motorsport pillars in the AAA Top Bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Pillar {
    #[default]
    Paddock,
    Drive,
    Garage,
    Online,
    Telemetry,
    Setup,
}

impl Pillar {
    pub const ALL: [Pillar; 6] = [
        Pillar::Paddock,
        Pillar::Drive,
        Pillar::Garage,
        Pillar::Online,
        Pillar::Telemetry,
        Pillar::Setup,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Paddock => "Paddock",
            Self::Drive => "Drive",
            Self::Garage => "Garage",
            Self::Online => "Online",
            Self::Telemetry => "Telemetry",
            Self::Setup => "Setup",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PaddockTab {
    #[default]
    Hub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DriveTab {
    #[default]
    QuickRace,
    Events,
    Championships,
    Launcher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CarSubView {
    #[default]
    Browser,
    Specs,
    Tuning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TrackSubView {
    #[default]
    Browser,
    Inspector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SetupsView {
    #[default]
    Editor,
    Advice,
    AbTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StudioView {
    #[default]
    Skins,
    LiveryPainter,
    PlateDesigner,
    ModelTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LibraryView {
    #[default]
    Mods,
    Installer,
    Collections,
    Metadata,
    Exports,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GarageTab {
    Showroom,
    Cars(CarSubView),
    Tracks(TrackSubView),
    Setups(SetupsView),
    Studio(StudioView),
    Library(LibraryView),
}

impl Default for GarageTab {
    fn default() -> Self {
        Self::Showroom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OnlineTab {
    #[default]
    Browser,
    HostServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TelemetryTab {
    #[default]
    Sessions,
    Live,
    Replay,
    Trajectory,
    LapTimes,
    Timing,
    Strategy,
    Engineer,
    Media,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VideoView {
    #[default]
    Video,
    VrTriple,
    Audio,
    CameraReplay,
    Profiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ControlsView {
    #[default]
    Controls,
    Ffb,
    RigProfiles,
    HudLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CspView {
    #[default]
    Config,
    Installer,
    Lighting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SystemView {
    #[default]
    InstallFinder,
    Logs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetupTab {
    VideoAudio(VideoView),
    ControlsHud(ControlsView),
    Csp(CspView),
    WeatherFx,
    System(SystemView),
    App,
}

impl Default for SetupTab {
    fn default() -> Self {
        Self::VideoAudio(VideoView::Video)
    }
}

/// Complete, strongly typed destination across all 6 pillars, 7 legacy pages and 35 tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Paddock(PaddockTab),
    Drive(DriveTab),
    Garage(GarageTab),
    Online(OnlineTab),
    Telemetry(TelemetryTab),
    Setup(SetupTab),
}

impl Default for Route {
    fn default() -> Self {
        Self::Paddock(PaddockTab::Hub)
    }
}

impl Route {
    pub const fn pillar(self) -> Pillar {
        match self {
            Self::Paddock(_) => Pillar::Paddock,
            Self::Drive(_) => Pillar::Drive,
            Self::Garage(_) => Pillar::Garage,
            Self::Online(_) => Pillar::Online,
            Self::Telemetry(_) => Pillar::Telemetry,
            Self::Setup(_) => Pillar::Setup,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Paddock(PaddockTab::Hub) => "Paddock",
            Self::Drive(DriveTab::QuickRace) => "Quick Race",
            Self::Drive(DriveTab::Events) => "Events",
            Self::Drive(DriveTab::Championships) => "Championships",
            Self::Drive(DriveTab::Launcher) => "External Launcher",
            Self::Garage(GarageTab::Showroom) => "Showroom",
            Self::Garage(GarageTab::Cars(CarSubView::Browser)) => "Cars",
            Self::Garage(GarageTab::Cars(CarSubView::Specs)) => "Car Specs",
            Self::Garage(GarageTab::Cars(CarSubView::Tuning)) => "Car Tuning",
            Self::Garage(GarageTab::Tracks(TrackSubView::Browser)) => "Tracks",
            Self::Garage(GarageTab::Tracks(TrackSubView::Inspector)) => "Track Info",
            Self::Garage(GarageTab::Setups(SetupsView::Editor)) => "Setups",
            Self::Garage(GarageTab::Setups(SetupsView::Advice)) => "Setup Advice",
            Self::Garage(GarageTab::Setups(SetupsView::AbTest)) => "Setup A/B",
            Self::Garage(GarageTab::Studio(StudioView::Skins)) => "Skins",
            Self::Garage(GarageTab::Studio(StudioView::LiveryPainter)) => "Livery Painter",
            Self::Garage(GarageTab::Studio(StudioView::PlateDesigner)) => "Plate Designer",
            Self::Garage(GarageTab::Studio(StudioView::ModelTools)) => "Model Tools",
            Self::Garage(GarageTab::Library(LibraryView::Mods)) => "Mods",
            Self::Garage(GarageTab::Library(LibraryView::Installer)) => "Install Packages",
            Self::Garage(GarageTab::Library(LibraryView::Collections)) => "Collections",
            Self::Garage(GarageTab::Library(LibraryView::Metadata)) => "Metadata & Checks",
            Self::Garage(GarageTab::Library(LibraryView::Exports)) => "Exports",
            Self::Online(OnlineTab::Browser) => "Servers",
            Self::Online(OnlineTab::HostServer) => "Dedicated Server",
            Self::Telemetry(TelemetryTab::Sessions) => "Sessions",
            Self::Telemetry(TelemetryTab::Live) => "Live Telemetry",
            Self::Telemetry(TelemetryTab::Replay) => "Replay Viewer",
            Self::Telemetry(TelemetryTab::Trajectory) => "Trajectory Studio",
            Self::Telemetry(TelemetryTab::LapTimes) => "Lap Times",
            Self::Telemetry(TelemetryTab::Timing) => "Timing Tower",
            Self::Telemetry(TelemetryTab::Strategy) => "Strategy",
            Self::Telemetry(TelemetryTab::Engineer) => "Race Engineer",
            Self::Telemetry(TelemetryTab::Media) => "Screenshots",
            Self::Setup(SetupTab::VideoAudio(VideoView::Profiles)) => "Graphics Profile",
            Self::Setup(SetupTab::VideoAudio(_)) => "Game Settings",
            Self::Setup(SetupTab::ControlsHud(ControlsView::HudLayout)) => "HUD Layout",
            Self::Setup(SetupTab::ControlsHud(_)) => "Controls & FFB",
            Self::Setup(SetupTab::Csp(CspView::Config)) => "CSP Config",
            Self::Setup(SetupTab::Csp(CspView::Installer)) => "CSP Lights Patch",
            Self::Setup(SetupTab::Csp(CspView::Lighting)) => "Lighting Profiles",
            Self::Setup(SetupTab::WeatherFx) => "WeatherFX",
            Self::Setup(SetupTab::System(SystemView::InstallFinder)) => "Install Finder",
            Self::Setup(SetupTab::System(SystemView::Logs)) => "Logs",
            Self::Setup(SetupTab::App) => "Settings",
        }
    }

    /// Maps a [`Route`] backed by a registered `Tool` to its exact `Tool::name()`.
    pub const fn tool_name(self) -> Option<&'static str> {
        match self {
            Self::Drive(DriveTab::QuickRace) => Some("Quick Race"),
            Self::Drive(DriveTab::Events) => Some("Events"),
            Self::Drive(DriveTab::Championships) => Some("Championships"),
            Self::Drive(DriveTab::Launcher) => Some("External Launcher"),
            Self::Garage(GarageTab::Cars(CarSubView::Specs)) => Some("Car Specs"),
            Self::Garage(GarageTab::Cars(CarSubView::Tuning)) => Some("Car Tuning"),
            Self::Garage(GarageTab::Tracks(TrackSubView::Inspector)) => Some("Track Info"),
            Self::Garage(GarageTab::Setups(SetupsView::Editor | SetupsView::AbTest)) => Some("Setups"),
            Self::Garage(GarageTab::Setups(SetupsView::Advice)) => Some("Setup Advice"),
            Self::Garage(GarageTab::Studio(StudioView::Skins)) => Some("Skins"),
            Self::Garage(GarageTab::Studio(StudioView::LiveryPainter)) => Some("Livery Painter"),
            Self::Garage(GarageTab::Studio(StudioView::PlateDesigner)) => Some("Plate Designer"),
            Self::Garage(GarageTab::Studio(StudioView::ModelTools)) => Some("Model Tools"),
            Self::Garage(GarageTab::Library(LibraryView::Mods)) => Some("Mods"),
            Self::Garage(GarageTab::Library(LibraryView::Installer)) => Some("Install Packages"),
            Self::Garage(GarageTab::Library(LibraryView::Collections)) => Some("Collections"),
            Self::Garage(GarageTab::Library(LibraryView::Metadata)) => Some("Metadata & Checks"),
            Self::Garage(GarageTab::Library(LibraryView::Exports)) => Some("Exports"),
            Self::Online(OnlineTab::HostServer) => Some("Dedicated Server"),
            Self::Telemetry(TelemetryTab::Sessions) => Some("Sessions"),
            Self::Telemetry(TelemetryTab::Live) => Some("Live Telemetry"),
            Self::Telemetry(TelemetryTab::Replay) => Some("Replay Viewer"),
            Self::Telemetry(TelemetryTab::Trajectory) => Some("Trajectory Studio"),
            Self::Telemetry(TelemetryTab::LapTimes) => Some("Lap Times"),
            Self::Telemetry(TelemetryTab::Timing) => Some("Timing Tower"),
            Self::Telemetry(TelemetryTab::Strategy) => Some("Strategy"),
            Self::Telemetry(TelemetryTab::Engineer) => Some("Race Engineer"),
            Self::Telemetry(TelemetryTab::Media) => Some("Screenshots"),
            Self::Setup(SetupTab::VideoAudio(VideoView::Profiles)) => Some("Graphics Profile"),
            Self::Setup(SetupTab::VideoAudio(_)) => Some("Game Settings"),
            Self::Setup(SetupTab::ControlsHud(ControlsView::HudLayout)) => Some("HUD Layout"),
            Self::Setup(SetupTab::ControlsHud(_)) => Some("Game Settings"),
            Self::Setup(SetupTab::Csp(CspView::Config)) => Some("CSP Config"),
            Self::Setup(SetupTab::Csp(CspView::Lighting)) => Some("Lighting Profiles"),
            Self::Setup(SetupTab::WeatherFx) => Some("CSP Config"),
            Self::Setup(SetupTab::System(SystemView::InstallFinder)) => Some("Install Finder"),
            Self::Setup(SetupTab::System(SystemView::Logs)) => Some("Logs"),
            _ => None,
        }
    }

    /// Maps any legacy `Tool::name()` (or sidebar pin name) to its canonical [`Route`].
    pub fn from_tool_name(name: &str) -> Option<Self> {
        Some(match name {
            "Home" | "Paddock" => Self::Paddock(PaddockTab::Hub),
            "Showroom" => Self::Garage(GarageTab::Showroom),
            "Cars" => Self::Garage(GarageTab::Cars(CarSubView::Browser)),
            "Tracks" => Self::Garage(GarageTab::Tracks(TrackSubView::Browser)),
            "Servers" => Self::Online(OnlineTab::Browser),
            "Lights Patch" | "CSP Lights Patch" => Self::Setup(SetupTab::Csp(CspView::Installer)),
            "Settings" => Self::Setup(SetupTab::App),
            "Quick Race" => Self::Drive(DriveTab::QuickRace),
            "Events" => Self::Drive(DriveTab::Events),
            "Championships" => Self::Drive(DriveTab::Championships),
            "External Launcher" => Self::Drive(DriveTab::Launcher),
            "Car Specs" => Self::Garage(GarageTab::Cars(CarSubView::Specs)),
            "Car Tuning" => Self::Garage(GarageTab::Cars(CarSubView::Tuning)),
            "Track Info" => Self::Garage(GarageTab::Tracks(TrackSubView::Inspector)),
            "Setups" => Self::Garage(GarageTab::Setups(SetupsView::Editor)),
            "Setup Advice" => Self::Garage(GarageTab::Setups(SetupsView::Advice)),
            "Skins" => Self::Garage(GarageTab::Studio(StudioView::Skins)),
            "Livery Painter" => Self::Garage(GarageTab::Studio(StudioView::LiveryPainter)),
            "Plate Designer" => Self::Garage(GarageTab::Studio(StudioView::PlateDesigner)),
            "Model Tools" => Self::Garage(GarageTab::Studio(StudioView::ModelTools)),
            "Mods" => Self::Garage(GarageTab::Library(LibraryView::Mods)),
            "Install Packages" => Self::Garage(GarageTab::Library(LibraryView::Installer)),
            "Collections" => Self::Garage(GarageTab::Library(LibraryView::Collections)),
            "Metadata & Checks" => Self::Garage(GarageTab::Library(LibraryView::Metadata)),
            "Exports" => Self::Garage(GarageTab::Library(LibraryView::Exports)),
            "Dedicated Server" => Self::Online(OnlineTab::HostServer),
            "Sessions" => Self::Telemetry(TelemetryTab::Sessions),
            "Live Telemetry" => Self::Telemetry(TelemetryTab::Live),
            "Replay Viewer" => Self::Telemetry(TelemetryTab::Replay),
            "Trajectory Studio" => Self::Telemetry(TelemetryTab::Trajectory),
            "Lap Times" => Self::Telemetry(TelemetryTab::LapTimes),
            "Timing Tower" => Self::Telemetry(TelemetryTab::Timing),
            "Strategy" => Self::Telemetry(TelemetryTab::Strategy),
            "Race Engineer" => Self::Telemetry(TelemetryTab::Engineer),
            "Screenshots" => Self::Telemetry(TelemetryTab::Media),
            "Game Settings" => Self::Setup(SetupTab::VideoAudio(VideoView::Video)),
            "Graphics Profile" => Self::Setup(SetupTab::VideoAudio(VideoView::Profiles)),
            "HUD Layout" => Self::Setup(SetupTab::ControlsHud(ControlsView::HudLayout)),
            "CSP Config" => Self::Setup(SetupTab::Csp(CspView::Config)),
            "Lighting Profiles" => Self::Setup(SetupTab::Csp(CspView::Lighting)),
            "Install Finder" => Self::Setup(SetupTab::System(SystemView::InstallFinder)),
            "Logs" => Self::Setup(SetupTab::System(SystemView::Logs)),
            _ => return None,
        })
    }

    /// Exhaustive list of distinct routes for headless route-walk testing.
    pub fn all_routes() -> Vec<Route> {
        vec![
            Self::Paddock(PaddockTab::Hub),
            Self::Drive(DriveTab::QuickRace),
            Self::Drive(DriveTab::Events),
            Self::Drive(DriveTab::Championships),
            Self::Drive(DriveTab::Launcher),
            Self::Garage(GarageTab::Showroom),
            Self::Garage(GarageTab::Cars(CarSubView::Browser)),
            Self::Garage(GarageTab::Cars(CarSubView::Specs)),
            Self::Garage(GarageTab::Cars(CarSubView::Tuning)),
            Self::Garage(GarageTab::Tracks(TrackSubView::Browser)),
            Self::Garage(GarageTab::Tracks(TrackSubView::Inspector)),
            Self::Garage(GarageTab::Setups(SetupsView::Editor)),
            Self::Garage(GarageTab::Setups(SetupsView::Advice)),
            Self::Garage(GarageTab::Setups(SetupsView::AbTest)),
            Self::Garage(GarageTab::Studio(StudioView::Skins)),
            Self::Garage(GarageTab::Studio(StudioView::LiveryPainter)),
            Self::Garage(GarageTab::Studio(StudioView::PlateDesigner)),
            Self::Garage(GarageTab::Studio(StudioView::ModelTools)),
            Self::Garage(GarageTab::Library(LibraryView::Mods)),
            Self::Garage(GarageTab::Library(LibraryView::Installer)),
            Self::Garage(GarageTab::Library(LibraryView::Collections)),
            Self::Garage(GarageTab::Library(LibraryView::Metadata)),
            Self::Garage(GarageTab::Library(LibraryView::Exports)),
            Self::Online(OnlineTab::Browser),
            Self::Online(OnlineTab::HostServer),
            Self::Telemetry(TelemetryTab::Sessions),
            Self::Telemetry(TelemetryTab::Live),
            Self::Telemetry(TelemetryTab::Replay),
            Self::Telemetry(TelemetryTab::Trajectory),
            Self::Telemetry(TelemetryTab::LapTimes),
            Self::Telemetry(TelemetryTab::Timing),
            Self::Telemetry(TelemetryTab::Strategy),
            Self::Telemetry(TelemetryTab::Engineer),
            Self::Telemetry(TelemetryTab::Media),
            Self::Setup(SetupTab::VideoAudio(VideoView::Video)),
            Self::Setup(SetupTab::VideoAudio(VideoView::VrTriple)),
            Self::Setup(SetupTab::VideoAudio(VideoView::Audio)),
            Self::Setup(SetupTab::VideoAudio(VideoView::CameraReplay)),
            Self::Setup(SetupTab::VideoAudio(VideoView::Profiles)),
            Self::Setup(SetupTab::ControlsHud(ControlsView::Controls)),
            Self::Setup(SetupTab::ControlsHud(ControlsView::Ffb)),
            Self::Setup(SetupTab::ControlsHud(ControlsView::RigProfiles)),
            Self::Setup(SetupTab::ControlsHud(ControlsView::HudLayout)),
            Self::Setup(SetupTab::Csp(CspView::Config)),
            Self::Setup(SetupTab::Csp(CspView::Installer)),
            Self::Setup(SetupTab::Csp(CspView::Lighting)),
            Self::Setup(SetupTab::WeatherFx),
            Self::Setup(SetupTab::System(SystemView::InstallFinder)),
            Self::Setup(SetupTab::System(SystemView::Logs)),
            Self::Setup(SetupTab::App),
        ]
    }
}

/// Remembers the last active sub-tab per pillar so switching pillars feels seamless.
#[derive(Debug, Clone)]
pub struct NavState {
    pub drive: DriveTab,
    pub garage: GarageTab,
    pub online: OnlineTab,
    pub telemetry: TelemetryTab,
    pub setup: SetupTab,
}

impl Default for NavState {
    fn default() -> Self {
        Self {
            drive: DriveTab::QuickRace,
            garage: GarageTab::Showroom,
            online: OnlineTab::Browser,
            telemetry: TelemetryTab::Sessions,
            setup: SetupTab::VideoAudio(VideoView::Video),
        }
    }
}

impl NavState {
    pub fn remember(&mut self, route: Route) {
        match route {
            Route::Paddock(_) => {}
            Route::Drive(t) => self.drive = t,
            Route::Garage(t) => self.garage = t,
            Route::Online(t) => self.online = t,
            Route::Telemetry(t) => self.telemetry = t,
            Route::Setup(t) => self.setup = t,
        }
    }

    pub fn route_for(&self, pillar: Pillar) -> Route {
        match pillar {
            Pillar::Paddock => Route::Paddock(PaddockTab::Hub),
            Pillar::Drive => Route::Drive(self.drive),
            Pillar::Garage => Route::Garage(self.garage),
            Pillar::Online => Route::Online(self.online),
            Pillar::Telemetry => Route::Telemetry(self.telemetry),
            Pillar::Setup => Route::Setup(self.setup),
        }
    }
}

/// Focus region for keyboard / controller navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusRegion {
    #[default]
    Content,
    TopBar,
    SubNav,
    PaddockDock,
    Modal,
}

/// Lightweight keyboard/gamepad navigation state.
#[derive(Debug, Default)]
pub struct FocusNav {
    pub region: FocusRegion,
    pub keyboard_mode: bool,
}

impl FocusNav {
    /// Process global pillar shortcuts (`Alt+1..6`, `Ctrl+Tab`, `Ctrl+Shift+Tab`) when no
    /// text input is focused. Returns a new [`Pillar`] if switched.
    pub fn handle_global_keys(&mut self, ctx: &egui::Context, current: Pillar) -> Option<Pillar> {
        if ctx.egui_wants_keyboard_input() {
            return None;
        }
        let mut next = None;
        ctx.input(|i| {
            if i.pointer.delta() != egui::Vec2::ZERO {
                self.keyboard_mode = false;
            }
            if i.modifiers.alt {
                let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
                for (idx, key) in keys.into_iter().enumerate() {
                    if i.key_pressed(key) {
                        self.keyboard_mode = true;
                        next = Some(Pillar::ALL[idx]);
                    }
                }
            }
            if i.modifiers.ctrl && i.key_pressed(Key::Tab) {
                self.keyboard_mode = true;
                next = Some(if i.modifiers.shift { current.prev() } else { current.next() });
            }
        });
        next
    }
}

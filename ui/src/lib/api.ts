import { convertFileSrc, invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  AdaptiveAiSuggestion,
  AppSettings,
  CarEntry,
  CarPhysicsData,
  CarSfxInfo,
  CarTuningValues,
  Championship,
  ConfigSnapshot,
  ContentHealthIssue,
  ContentScanResult,
  CspPatchInfo,
  DedicatedServerConfig,
  DedicatedServerStatus,
  FfbConfig,
  JoinOnlineRequest,
  JsgmeModEntry,
  LiveTelemetryFrame,
  OnlineServerDetail,
  OnlineServerSummary,
  PackageEntry,
  QuickRacePreset,
  QuickRaceRequest,
  RconResponse,
  ReplaySummary,
  RigProfile,
  Scene3DMeshData,
  SetupDiffEntry,
  SetupDocument,
  SetupFileSummary,
  SkinEntry,
  SpecialEvent,
  StrategyPlan,
  StrategyRequest,
  TelemetrySessionAnalysis,
  TelemetrySessionSummary,
  TimingRow,
  Track3DSceneData,
  TrackEntry,
  VideoSettings,
  VrTripleSettings,
  WatchlistEntry,
  WeatherController,
} from "./types";

// ============================================================================
// Environment Detection & Asset Protocol
// ============================================================================

export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  );
}

export function assetUrl(filePath?: string | null): string | null {
  if (!filePath) return null;
  if (
    filePath.startsWith("http://") ||
    filePath.startsWith("https://") ||
    filePath.startsWith("data:") ||
    filePath.startsWith("asset:")
  ) {
    return filePath;
  }
  if (isTauri()) {
    try {
      return convertFileSrc(filePath);
    } catch {
      return null;
    }
  }
  return null;
}

// ============================================================================
// Browser-Mode Preview Fallbacks (so `npm run dev` runs cleanly outside Tauri)
// ============================================================================

const DEMO_CARS: CarEntry[] = [
  {
    id: "ks_porsche_911_gt3_r_2016",
    name: "Porsche 911 GT3 R",
    brand: "Porsche",
    class: "race",
    country: "Germany",
    year: 2016,
    bhp: "500 bhp",
    torque: "460 Nm",
    weight: "1220 kg",
    topSpeed: "292 km/h",
    pwRatio: "2.44 kg/hp",
    tags: ["#GT3", "race", "rear-engine", "germany"],
    path: "/ac/content/cars/ks_porsche_911_gt3_r_2016",
    skinsCount: 14,
    enabled: true,
  },
  {
    id: "ks_ferrari_488_gt3",
    name: "Ferrari 488 GT3",
    brand: "Ferrari",
    class: "race",
    country: "Italy",
    year: 2016,
    bhp: "550 bhp",
    torque: "650 Nm",
    weight: "1260 kg",
    topSpeed: "296 km/h",
    pwRatio: "2.29 kg/hp",
    tags: ["#GT3", "race", "twin-turbo", "italy"],
    path: "/ac/content/cars/ks_ferrari_488_gt3",
    skinsCount: 18,
    enabled: true,
  },
  {
    id: "ks_mazda_787b",
    name: "Mazda 787B",
    brand: "Mazda",
    class: "race",
    country: "Japan",
    year: 1991,
    bhp: "700 bhp",
    torque: "607 Nm",
    weight: "830 kg",
    topSpeed: "344 km/h",
    pwRatio: "1.18 kg/hp",
    tags: ["#GroupC", "rotary", "le-mans", "japan"],
    path: "/ac/content/cars/ks_mazda_787b",
    skinsCount: 6,
    enabled: true,
  },
  {
    id: "ks_mclaren_p1_gtr",
    name: "McLaren P1 GTR",
    brand: "McLaren",
    class: "race",
    country: "UK",
    year: 2015,
    bhp: "986 bhp",
    torque: "900 Nm",
    weight: "1440 kg",
    topSpeed: "349 km/h",
    pwRatio: "1.46 kg/hp",
    tags: ["#Hypercar", "hybrid", "track-day", "uk"],
    path: "/ac/content/cars/ks_mclaren_p1_gtr",
    skinsCount: 9,
    enabled: true,
  },
  {
    id: "ks_nissan_gtr_nismo",
    name: "Nissan GT-R NISMO",
    brand: "Nissan",
    class: "street",
    country: "Japan",
    year: 2015,
    bhp: "600 bhp",
    torque: "652 Nm",
    weight: "1720 kg",
    topSpeed: "315 km/h",
    pwRatio: "2.86 kg/hp",
    tags: ["#A8", "awd", "street", "japan"],
    path: "/ac/content/cars/ks_nissan_gtr_nismo",
    skinsCount: 7,
    enabled: true,
  },
  {
    id: "bmw_m3_e30_dtm",
    name: "BMW M3 E30 Group A 92",
    brand: "BMW",
    class: "race",
    country: "Germany",
    year: 1992,
    bhp: "355 bhp",
    torque: "300 Nm",
    weight: "980 kg",
    topSpeed: "281 km/h",
    pwRatio: "2.76 kg/hp",
    tags: ["#DTM", "vintage", "touring", "germany"],
    path: "/ac/content/cars/bmw_m3_e30_dtm",
    skinsCount: 11,
    enabled: true,
  },
];

const DEMO_TRACKS: TrackEntry[] = [
  {
    id: "spa",
    name: "Circuit de Spa-Francorchamps",
    country: "Belgium",
    city: "Stavelot",
    length: "7004 m",
    lengthMeters: 7004,
    pitboxes: 24,
    tags: ["circuit", "gp", "endurance"],
    path: "/ac/content/tracks/spa",
    layouts: [{ id: "default", name: "Grand Prix", lengthMeters: 7004, pitboxes: 24 }],
    enabled: true,
  },
  {
    id: "ks_nordschleife",
    name: "Nürburgring Nordschleife",
    country: "Germany",
    city: "Nürburg",
    length: "20832 m",
    lengthMeters: 20832,
    pitboxes: 32,
    tags: ["circuit", "endurance", "tourist"],
    path: "/ac/content/tracks/ks_nordschleife",
    layouts: [
      { id: "nordschleife", name: "Nordschleife", lengthMeters: 20832, pitboxes: 24 },
      { id: "endurance", name: "Endurance 24h", lengthMeters: 25378, pitboxes: 32 },
      { id: "touristenfahrten", name: "Touristenfahrten", lengthMeters: 19100, pitboxes: 32 },
    ],
    enabled: true,
  },
  {
    id: "monza",
    name: "Autodromo Nazionale Monza",
    country: "Italy",
    city: "Monza",
    length: "5793 m",
    lengthMeters: 5793,
    pitboxes: 26,
    tags: ["circuit", "gp", "high-speed"],
    path: "/ac/content/tracks/monza",
    layouts: [{ id: "default", name: "Grand Prix", lengthMeters: 5793, pitboxes: 26 }],
    enabled: true,
  },
  {
    id: "imola",
    name: "Autodromo Enzo e Dino Ferrari",
    country: "Italy",
    city: "Imola",
    length: "4909 m",
    lengthMeters: 4909,
    pitboxes: 24,
    tags: ["circuit", "gp", "historic"],
    path: "/ac/content/tracks/imola",
    layouts: [{ id: "default", name: "Grand Prix", lengthMeters: 4909, pitboxes: 24 }],
    enabled: true,
  },
];

const DEMO_SETTINGS: AppSettings = {
  acRoot: "",
  accent: "MotorsportRed",
  steamLaunch: false,
  closeOnLaunch: false,
  checkUpdates: true,
  telemetryPort: 9996,
  webDashboardPort: 8787,
  cspAutoSync: true,
};

function browserFallback<T>(cmd: string, args?: Record<string, unknown>): T {
  switch (cmd) {
    case "scan_content":
      return {
        cars: DEMO_CARS,
        tracks: DEMO_TRACKS,
        gameRunning: false,
        cspBuild: "0.2.5-preview1",
        drivingStats: {
          totalKm: 4286.4,
          totalHours: 38.5,
          totalLaps: 612,
          favoriteCar: "Porsche 911 GT3 R",
          favoriteTrack: "Spa-Francorchamps",
          sessionsCount: 47,
        },
        acRoot: "",
      } as T;
    case "search_cars": {
      const q = String(args?.queryStr ?? args?.query ?? "").toLowerCase();
      return (q
        ? DEMO_CARS.filter(
            (c) =>
              c.name.toLowerCase().includes(q) ||
              c.brand.toLowerCase().includes(q) ||
              c.tags.some((t) => t.toLowerCase().includes(q))
          )
        : DEMO_CARS) as T;
    }
    case "search_tracks": {
      const q = String(args?.queryStr ?? args?.query ?? "").toLowerCase();
      return (q
        ? DEMO_TRACKS.filter(
            (t) =>
              t.name.toLowerCase().includes(q) ||
              t.country.toLowerCase().includes(q)
          )
        : DEMO_TRACKS) as T;
    }
    case "get_car_skins":
      return [
        { id: "00_factory_white", name: "Factory White", number: "911", team: "Works Motorsport", path: "" },
        { id: "01_obsidian_carbon", name: "Obsidian Carbon", number: "24", team: "ACXS Racing", path: "" },
        { id: "02_crimson_gt", name: "Crimson GT", number: "07", team: "Scuderia Corsa", path: "" },
      ] as T;
    case "get_settings":
      return { ...DEMO_SETTINGS } as T;
    case "save_settings":
      return (args?.settings ?? DEMO_SETTINGS) as T;
    case "list_weather_controllers":
      return [
        { id: "sol", name: "Sol 2.2.9", author: "Peter Boese" },
        { id: "pure", name: "Pure 0.254", author: "Peter Boese" },
        { id: "default", name: "Kunos Default" },
      ] as T;
    case "fetch_directory_servers":
      return [
        {
          id: "demo-1",
          name: "ACXS GT3 Endurance | Spa 24h | CSP",
          host: "198.51.100.12",
          port: 9600,
          httpPort: 8081,
          trackId: "spa",
          clients: 19,
          maxClients: 24,
          pingMs: 28,
          cars: ["ks_porsche_911_gt3_r_2016", "ks_ferrari_488_gt3"],
          passworded: false,
          country: "DE",
          cspRequired: 2144,
          sessionType: "Race",
        },
        {
          id: "demo-2",
          name: "Nordschleife Touristenfahrten | Trackday",
          host: "198.51.100.44",
          port: 9601,
          httpPort: 8082,
          trackId: "ks_nordschleife",
          trackLayout: "touristenfahrten",
          clients: 27,
          maxClients: 32,
          pingMs: 34,
          cars: ["ks_nissan_gtr_nismo", "bmw_m3_e30_dtm"],
          passworded: false,
          country: "NL",
          cspRequired: 2144,
          sessionType: "Practice",
        },
      ] as T;
    case "get_live_telemetry":
      return {
        connected: false,
        speedKmh: 0,
        rpm: 0,
        maxRpm: 8500,
        gear: 0,
        throttle: 0,
        brake: 0,
        clutch: 0,
        steerDeg: 0,
        fuelL: 42,
        lapNumber: 0,
        currentLapMs: 0,
        lastLapMs: 0,
        bestLapMs: 0,
        tyreTempsC: [78, 79, 76, 77],
        tyrePressuresPsi: [26.8, 26.9, 26.5, 26.6],
        gLat: 0,
        gLong: 0,
      } as T;
    case "get_dedicated_server_status":
      return { running: false, pid: null, uptimeSec: 0, recentLogs: [] } as T;
    case "get_video_settings":
      return {
        width: 2560,
        height: 1440,
        refreshHz: 144,
        fullscreen: true,
        vsync: false,
        fpsCap: 0,
        msaa: 4,
        anisotropic: 16,
        worldDetail: 5,
        shadowMapSize: 2048,
        postProcessing: true,
        ppFilter: "default",
      } as T;
    case "get_vr_triple_settings":
      return {
        renderMode: "SINGLE",
        tripleFovDeg: 54,
        screenWidthMm: 600,
        distanceMm: 650,
        bezelMm: 10,
        sideAngleDeg: 45,
        vrPixelDensity: 1.2,
      } as T;
    case "get_ffb_config":
      return {
        gain: 0.75,
        filter: 0.0,
        minForce: 0.02,
        kerbEffect: 0.3,
        roadEffect: 0.25,
        slipEffect: 0.15,
        absEffect: 0.2,
        enhancedUndersteer: false,
        gammaEnabled: false,
        gammaValue: 1.0,
      } as T;
    case "get_presets":
    case "list_events":
    case "list_championships":
    case "list_watchlist":
    case "list_recent_servers":
    case "scan_lan_servers":
    case "list_setups":
    case "list_jsgme_mods":
    case "run_content_health":
    case "list_sessions":
    case "list_replays":
    case "list_laptimes":
    case "load_timing_tower":
    case "list_rig_profiles":
    case "list_csp_patches":
    case "list_csp_config_files":
    case "list_config_snapshots":
      return [] as T;
    default:
      return null as T;
  }
}

// ============================================================================
// Unified Invoke Wrapper
// ============================================================================

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!isTauri()) {
    return browserFallback<T>(cmd, args);
  }
  return tauriInvoke<T>(cmd, args);
}

// ============================================================================
// Typed API Facade (All 6 Pillars)
// ============================================================================

export const api = {
  // Content
  scanContent: () => invoke<ContentScanResult>("scan_content"),
  searchCars: (queryStr: string) => invoke<CarEntry[]>("search_cars", { queryStr }),
  searchTracks: (queryStr: string) => invoke<TrackEntry[]>("search_tracks", { queryStr }),
  getCarSkins: (carPath: string) => invoke<SkinEntry[]>("get_car_skins", { carPath }),
  openPathInExplorer: (path: string) => invoke<void>("open_path_in_explorer", { path }),

  // Drive
  launchQuickRace: (req: QuickRaceRequest) => invoke<string>("launch_quick_race", { req }),
  launchGameDirect: () => invoke<string>("launch_game_direct"),
  getPresets: () => invoke<QuickRacePreset[]>("get_presets"),
  savePreset: (preset: QuickRacePreset) => invoke<void>("save_preset", { preset }),
  deletePreset: (name: string) => invoke<void>("delete_preset", { name }),
  computeAdaptiveAi: (carId: string, trackId: string, count: number, spread: number) =>
    invoke<AdaptiveAiSuggestion>("compute_adaptive_ai", { carId, trackId, count, spread }),
  listEvents: () => invoke<SpecialEvent[]>("list_events"),
  launchEvent: (eventId: string) => invoke<string>("launch_event", { eventId }),
  listChampionships: () => invoke<Championship[]>("list_championships"),
  saveChampionship: (champ: Championship) => invoke<void>("save_championship", { champ }),
  launchChampionshipRound: (champName: string, roundIdx: number) =>
    invoke<string>("launch_championship_round", { champName, roundIdx }),
  importRaceOutResults: () => invoke<unknown>("import_race_out_results"),
  listWeatherControllers: () => invoke<WeatherController[]>("list_weather_controllers"),

  // Online
  fetchDirectoryServers: (query = "", hasPlayers = false) =>
    invoke<OnlineServerSummary[]>("fetch_directory_servers", { query, hasPlayers }),
  fetchServerDetail: (host: string, port: number) =>
    invoke<OnlineServerDetail>("fetch_server_detail", { host, port }),
  listWatchlist: () => invoke<WatchlistEntry[]>("list_watchlist"),
  saveWatchlist: (entries: WatchlistEntry[]) => invoke<void>("save_watchlist", { entries }),
  listRecentServers: () => invoke<OnlineServerSummary[]>("list_recent_servers"),
  scanLanServers: () => invoke<OnlineServerSummary[]>("scan_lan_servers"),
  parseJoinUri: (raw: string) => invoke<{ host: string; port: number; password?: string }>("parse_join_uri", { raw }),
  formatInviteUri: (host: string, port: number, password?: string) =>
    invoke<string>("format_invite_uri", { host, port, password }),
  joinOnlineServer: (req: JoinOnlineRequest) => invoke<string>("join_online_server", { req }),
  bookOnlineServer: (req: JoinOnlineRequest) => invoke<string>("book_online_server", { req }),
  loadDedicatedServer: (dir: string) => invoke<DedicatedServerConfig>("load_dedicated_server", { dir }),
  saveDedicatedServer: (dir: string, cfg: DedicatedServerConfig, entries: unknown[]) =>
    invoke<void>("save_dedicated_server", { dir, cfg, entries }),
  startDedicatedServer: (dir: string) => invoke<DedicatedServerStatus>("start_dedicated_server", { dir }),
  stopDedicatedServer: () => invoke<void>("stop_dedicated_server"),
  getDedicatedServerStatus: () => invoke<DedicatedServerStatus>("get_dedicated_server_status"),
  rconConnect: (host: string, port: number) => invoke<RconResponse>("rcon_connect", { host, port }),
  rconSendCommand: (cmd: string) => invoke<RconResponse>("rcon_send_command", { cmd }),

  // Garage
  loadCar3DScene: (carPath: string, skin = "") =>
    invoke<Scene3DMeshData>("load_car_3d_scene", { carPath, skin }),
  loadTrack3DScene: (trackPath: string, layout = "") =>
    invoke<Track3DSceneData>("load_track_3d_scene", { trackPath, layout }),
  getCarPhysics: (carPath: string) => invoke<CarPhysicsData>("get_car_physics", { carPath }),
  getCarTuning: (carPath: string) => invoke<CarTuningValues>("get_car_tuning", { carPath }),
  applyCarTuning: (carPath: string, tuning: CarTuningValues) =>
    invoke<void>("apply_car_tuning", { carPath, tuning }),
  unpackCarAcd: (carPath: string) => invoke<string>("unpack_car_acd", { carPath }),
  packCarAcd: (carPath: string) => invoke<string>("pack_car_acd", { carPath }),
  inspectCarSfx: (carPath: string) => invoke<CarSfxInfo>("inspect_car_sfx", { carPath }),
  swapCarSfx: (targetCar: string, donorCar: string) =>
    invoke<string>("swap_car_sfx", { targetCar, donorCar }),
  listSetups: () => invoke<SetupFileSummary[]>("list_setups"),
  readSetup: (path: string, carPath: string) => invoke<SetupDocument>("read_setup", { path, carPath }),
  saveSetup: (path: string, values: Record<string, number>) => invoke<void>("save_setup", { path, values }),
  diffSetups: (left: string, right: string) => invoke<SetupDiffEntry[]>("diff_setups", { left, right }),
  inspectPackage: (path: string) => invoke<PackageEntry[]>("inspect_package", { path }),
  installPackage: (path: string, selectedIds: string[]) =>
    invoke<string>("install_package", { path, selectedIds }),
  listJsgmeMods: () => invoke<JsgmeModEntry[]>("list_jsgme_mods"),
  enableJsgmeMod: (name: string) => invoke<void>("enable_jsgme_mod", { name }),
  disableJsgmeMod: (name: string) => invoke<void>("disable_jsgme_mod", { name }),
  toggleContentEnabled: (kind: "car" | "track", id: string, enable: boolean) =>
    invoke<void>("toggle_content_enabled", { kind, id, enable }),
  runContentHealth: () => invoke<ContentHealthIssue[]>("run_content_health"),
  applyContentRepairs: () => invoke<string>("apply_content_repairs"),

  // Telemetry
  listSessions: () => invoke<TelemetrySessionSummary[]>("list_sessions"),
  getSessionAnalysis: (id: string) => invoke<TelemetrySessionAnalysis>("get_session_analysis", { id }),
  listReplays: () => invoke<ReplaySummary[]>("list_replays"),
  launchReplayInAc: (filename: string) => invoke<string>("launch_replay_in_ac", { filename }),
  computeStrategy: (req: StrategyRequest) => invoke<StrategyPlan>("compute_strategy", { req }),
  loadTimingTower: (csvOrNone?: string | null) =>
    invoke<TimingRow[]>("load_timing_tower", { csvOrNone: csvOrNone ?? null }),
  udpConnect: (host: string, port: number) => invoke<void>("udp_connect", { host, port }),
  udpDisconnect: () => invoke<void>("udp_disconnect"),
  getLiveTelemetry: () => invoke<LiveTelemetryFrame>("get_live_telemetry"),
  toggleWebDashboard: (port: number, lan: boolean, start: boolean) =>
    invoke<string>("toggle_web_dashboard", { port, lan, start }),

  // Setup
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings }),
  getVideoSettings: () => invoke<VideoSettings>("get_video_settings"),
  saveVideoSettings: (cfg: VideoSettings) => invoke<void>("save_video_settings", { cfg }),
  getVrTripleSettings: () => invoke<VrTripleSettings>("get_vr_triple_settings"),
  saveVrTripleSettings: (cfg: VrTripleSettings) => invoke<void>("save_vr_triple_settings", { cfg }),
  getFfbConfig: () => invoke<FfbConfig>("get_ffb_config"),
  saveFfbConfig: (cfg: FfbConfig) => invoke<void>("save_ffb_config", { cfg }),
  listRigProfiles: () => invoke<RigProfile[]>("list_rig_profiles"),
  applyRigProfile: (name: string) => invoke<void>("apply_rig_profile", { name }),
  listCspPatches: () => invoke<CspPatchInfo[]>("list_csp_patches"),
  installCspPatch: (name: string) => invoke<string>("install_csp_patch", { name }),
  uninstallCspPatch: () => invoke<string>("uninstall_csp_patch"),
  syncCspConfigs: () => invoke<string>("sync_csp_configs"),
  listCspConfigFiles: () => invoke<unknown[]>("list_csp_config_files"),
  readCspConfig: (file: string) => invoke<unknown>("read_csp_config", { file }),
  saveCspConfig: (file: string, edits: Record<string, string>) =>
    invoke<void>("save_csp_config", { file, edits }),
  listConfigSnapshots: () => invoke<ConfigSnapshot[]>("list_config_snapshots"),
  createConfigSnapshot: (label: string) => invoke<ConfigSnapshot>("create_config_snapshot", { label }),
  restoreConfigSnapshot: (id: string) => invoke<void>("restore_config_snapshot", { id }),
};

// ============================================================================
// ACXS Manager — Core TypeScript Domain & IPC Contract Types
// ============================================================================

export type Pillar =
  | "paddock"
  | "drive"
  | "garage"
  | "online"
  | "telemetry"
  | "setup";

export type DriveSubTab = "quick-race" | "events" | "championships";
export type GarageSubTab =
  | "showroom"
  | "cars"
  | "tracks"
  | "setups"
  | "studio"
  | "library";
export type OnlineSubTab = "servers" | "dedicated";
export type TelemetrySubTab =
  | "sessions"
  | "live"
  | "replay"
  | "trajectory"
  | "strategy";
export type SetupSubTab = "game" | "csp" | "csp-patches" | "csp-weather" | "app";

export type SubTab =
  | DriveSubTab
  | GarageSubTab
  | OnlineSubTab
  | TelemetrySubTab
  | SetupSubTab
  | "";

export type AccentPreset =
  | "MotorsportRed"
  | "TelemetryCyan"
  | "AmberGold"
  | "Emerald";

export interface AccentDefinition {
  id: AccentPreset;
  label: string;
  hex: string;
  rgb: string;
}

export const ACCENT_PRESETS: AccentDefinition[] = [
  { id: "MotorsportRed", label: "Motorsport Red", hex: "#FF4D2E", rgb: "255, 77, 46" },
  { id: "TelemetryCyan", label: "Telemetry Cyan", hex: "#00D1FF", rgb: "0, 209, 255" },
  { id: "AmberGold", label: "Amber Gold", hex: "#FFB020", rgb: "255, 176, 32" },
  { id: "Emerald", label: "Emerald", hex: "#10B981", rgb: "16, 185, 129" },
];

export type SessionMode =
  | "Practice"
  | "QuickRace"
  | "Hotlap"
  | "TimeAttack"
  | "Drift"
  | "TrackDay"
  | "Drag"
  | "Weekend";

export interface ArmedSession {
  carId: string;
  carName: string;
  skinId: string;
  trackId: string;
  trackName: string;
  layoutId: string;
  mode: SessionMode;
  carPreviewPath?: string | null;
  trackPreviewPath?: string | null;
}

// ----------------------------------------------------------------------------
// Content: Cars, Tracks, Skins, Scan Summary
// ----------------------------------------------------------------------------

export interface CarEntry {
  id: string;
  name: string;
  brand: string;
  class: string;
  country?: string;
  year?: number | null;
  bhp?: string;
  torque?: string;
  weight?: string;
  topSpeed?: string;
  pwRatio?: string;
  tags: string[];
  path: string;
  previewPath?: string | null;
  badgePath?: string | null;
  skinsCount?: number;
  enabled?: boolean;
  isMod?: boolean;
}

export interface TrackLayout {
  id: string;
  name: string;
  lengthMeters?: number | null;
  pitboxes?: number | null;
  previewPath?: string | null;
  outlinePath?: string | null;
}

export interface TrackEntry {
  id: string;
  name: string;
  country: string;
  city?: string;
  length?: string;
  lengthMeters?: number | null;
  pitboxes?: number | null;
  tags: string[];
  path: string;
  previewPath?: string | null;
  outlinePath?: string | null;
  layouts: TrackLayout[];
  enabled?: boolean;
  isMod?: boolean;
}

export interface SkinEntry {
  id: string;
  name: string;
  driverName?: string;
  team?: string;
  number?: string;
  country?: string;
  path: string;
  previewPath?: string | null;
  liveryPath?: string | null;
}

export interface DrivingStatsSummary {
  totalKm: number;
  totalHours: number;
  totalLaps: number;
  favoriteCar?: string | null;
  favoriteTrack?: string | null;
  sessionsCount: number;
}

export interface ContentScanResult {
  cars: CarEntry[];
  tracks: TrackEntry[];
  gameRunning: boolean;
  cspBuild: string | null;
  drivingStats: DrivingStatsSummary;
  acRoot: string;
}

// ----------------------------------------------------------------------------
// Drive: Quick Race, Presets, Adaptive AI, Events, Championships, Weather
// ----------------------------------------------------------------------------

export interface AiOpponent {
  carId: string;
  skinId?: string;
  driverName?: string;
  aiLevel: number;
  aiAggression: number;
  ballastKg?: number;
  restrictor?: number;
}

export interface QuickRaceRequest {
  carId: string;
  skinId: string;
  trackId: string;
  layoutId: string;
  mode: SessionMode;
  laps: number;
  durationMinutes: number;
  aiOpponents: AiOpponent[];
  timeOfDayMinutes: number;
  weatherId: string;
  weatherController?: string;
  ambientTempC: number;
  roadTempC: number;
  trackGripPreset: string;
  ghostEnabled: boolean;
  ghostAdvantageSec: number;
  penaltiesEnabled: boolean;
  damageMultiplier: number;
  fuelRate: number;
  tyreWearRate: number;
  aiFlood?: boolean;
}

export interface QuickRacePreset {
  name: string;
  description?: string;
  createdAt?: string;
  request: QuickRaceRequest;
}

export interface AdaptiveAiSuggestion {
  recommendedLevel: number;
  minLevel: number;
  maxLevel: number;
  confidence: number;
  sampleLaps: number;
  opponents: AiOpponent[];
}

export interface SpecialEvent {
  id: string;
  name: string;
  category: string;
  carId: string;
  trackId: string;
  layoutId: string;
  mode: SessionMode;
  bronzeTarget?: string;
  silverTarget?: string;
  goldTarget?: string;
  earnedMedal?: "None" | "Bronze" | "Silver" | "Gold";
  bestResult?: string | null;
  description?: string;
}

export interface ChampionshipRound {
  name: string;
  trackId: string;
  layoutId: string;
  laps: number;
  weatherId: string;
  completed: boolean;
  winner?: string | null;
  playerPosition?: number | null;
  playerPoints?: number | null;
}

export interface Championship {
  name: string;
  carClass: string;
  playerCarId: string;
  rounds: ChampionshipRound[];
  pointsSystem: number[];
  standings?: Array<{ driver: string; points: number; wins: number }>;
}

export interface WeatherController {
  id: string;
  name: string;
  author?: string;
  version?: string;
}

// ----------------------------------------------------------------------------
// Online: Directory, Server Detail, Watchlist, LAN, Join, Dedicated, RCON
// ----------------------------------------------------------------------------

export interface OnlineServerSummary {
  id: string;
  name: string;
  host: string;
  port: number;
  httpPort?: number;
  trackId: string;
  trackLayout?: string;
  clients: number;
  maxClients: number;
  pingMs?: number | null;
  cars: string[];
  passworded: boolean;
  country?: string;
  cspRequired?: number | null;
  sessionType?: string;
  timeLeftSec?: number | null;
  tags?: string[];
}

export interface ServerCarSlot {
  carId: string;
  skinId: string;
  driverName?: string;
  connected: boolean;
  owned: boolean;
}

export interface MissingContentItem {
  kind: "car" | "track";
  id: string;
  downloadUrl?: string | null;
}

export interface OnlineServerDetail {
  summary: OnlineServerSummary;
  resolvedTrackId: string;
  trackOwned: boolean;
  minCspBuild?: number | null;
  slots: ServerCarSlot[];
  missingContent: MissingContentItem[];
  description?: string;
  bookingMode?: boolean;
}

export interface WatchlistEntry {
  name: string;
  host: string;
  port: number;
  notes?: string;
  favorite?: boolean;
}

export interface JoinOnlineRequest {
  host: string;
  port: number;
  httpPort?: number;
  carId: string;
  skinId: string;
  password?: string;
  driverName?: string;
}

export interface DedicatedServerConfig {
  name: string;
  track: string;
  layout: string;
  cars: string[];
  maxClients: number;
  udpPort: number;
  tcpPort: number;
  httpPort: number;
  password: string;
  adminPassword: string;
  registerToLobby: boolean;
  loopMode: boolean;
  pickupMode: boolean;
  practiceMinutes: number;
  qualifyMinutes: number;
  raceLaps: number;
}

export interface DedicatedServerStatus {
  running: boolean;
  pid?: number | null;
  uptimeSec?: number;
  recentLogs: string[];
}

export interface RconResponse {
  connected: boolean;
  output: string;
}

// ----------------------------------------------------------------------------
// Garage: 3D Scene, Physics, Tuning, SFX, Setups, Packages, JSGME, Health
// ----------------------------------------------------------------------------

export interface Scene3DMeshData {
  positions: number[];
  normals: number[];
  indices: number[];
  bounds?: {
    min: [number, number, number];
    max: [number, number, number];
  };
  triangleCount?: number;
  nodeCount?: number;
  sourcePath?: string;
}

export interface Track3DSceneData {
  centerline: Array<[number, number, number]>;
  leftEdge?: Array<[number, number, number]>;
  rightEdge?: Array<[number, number, number]>;
  mesh?: Scene3DMeshData | null;
  drsZones?: Array<{ start: number; end: number }>;
  camerasCount?: number;
  sectionsCount?: number;
  aiHintsCount?: number;
}

export interface CarPhysicsData {
  rpmCurve: number[];
  torqueNm: number[];
  powerHp: number[];
  gearRatios: number[];
  gearTopSpeedsKmh: number[];
  redlineRpm: number;
  massKg: number;
  aeroCd?: number;
  aeroCl?: number;
  tyreCompounds?: string[];
  hasPackedAcd: boolean;
  hasUnpackedData: boolean;
}

export interface CarTuningValues {
  powerMultiplier: number;
  torqueMultiplier: number;
  massOffsetKg: number;
  aeroDragMultiplier: number;
  aeroDownforceMultiplier: number;
  brakeTorqueMultiplier: number;
  finalDriveRatio?: number;
}

export interface CarSfxInfo {
  carId: string;
  bankPath?: string | null;
  bankSizeBytes: number;
  guidsPresent: boolean;
}

export interface SetupFileSummary {
  name: string;
  carId: string;
  trackId: string;
  path: string;
  modifiedAt?: string;
}

export interface SetupParameter {
  key: string;
  section: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit?: string;
}

export interface SetupDocument {
  path: string;
  carId: string;
  trackId: string;
  parameters: SetupParameter[];
}

export interface SetupDiffEntry {
  key: string;
  label: string;
  leftValue: number | string;
  rightValue: number | string;
  delta?: number;
}

export interface PackageEntry {
  id: string;
  kind: "car" | "track" | "skin" | "app" | "csp" | "mod" | "other";
  name: string;
  targetPath: string;
  sizeBytes: number;
  selected: boolean;
}

export interface JsgmeModEntry {
  name: string;
  enabled: boolean;
  filesCount: number;
  conflictsWith: string[];
}

export interface ContentHealthIssue {
  id: string;
  severity: "error" | "warning" | "info";
  kind: string;
  target: string;
  message: string;
  autoFixable: boolean;
}

// ----------------------------------------------------------------------------
// Telemetry: Sessions, Replays, Trajectory, Strategy, Timing, Live Pit-Wall
// ----------------------------------------------------------------------------

export interface TelemetrySessionSummary {
  id: string;
  carId: string;
  trackId: string;
  date: string;
  lapsCount: number;
  bestLapMs: number;
  topSpeedKmh: number;
}

export interface CornerCoachNote {
  cornerIndex: number;
  name: string;
  timeLossMs: number;
  entrySpeedDeltaKmh: number;
  apexSpeedDeltaKmh: number;
  advice: string;
}

export interface TelemetrySessionAnalysis {
  summary: TelemetrySessionSummary;
  lapTimesMs: number[];
  speedTrace: Array<{ distanceM: number; speedKmh: number; throttle: number; brake: number; gear: number }>;
  corners: CornerCoachNote[];
  trackPath: Array<[number, number]>;
}

export interface ReplaySummary {
  filename: string;
  path: string;
  sizeBytes: number;
  modifiedAt: string;
  carId?: string;
  trackId?: string;
  durationSec?: number;
}

export interface StrategyRequest {
  raceLaps: number;
  lapTimeSec: number;
  fuelPerLapL: number;
  tankCapacityL: number;
  pitLossSec: number;
  tyreLifeLaps: number;
}

export interface StrategyPlan {
  totalFuelL: number;
  stopsRequired: number;
  recommendedPitLaps: number[];
  stintLengths: number[];
  estimatedRaceTimeSec: number;
  fuelMarginL: number;
}

export interface TimingRow {
  position: number;
  driver: string;
  carId: string;
  bestLapMs: number;
  lastLapMs: number;
  gapMs: number;
  laps: number;
  sector1Ms?: number;
  sector2Ms?: number;
  sector3Ms?: number;
}

export interface LiveTelemetryFrame {
  connected: boolean;
  speedKmh: number;
  rpm: number;
  maxRpm: number;
  gear: number;
  throttle: number;
  brake: number;
  clutch: number;
  steerDeg: number;
  fuelL: number;
  lapNumber: number;
  currentLapMs: number;
  lastLapMs: number;
  bestLapMs: number;
  tyreTempsC: [number, number, number, number];
  tyrePressuresPsi: [number, number, number, number];
  gLat: number;
  gLong: number;
}

// ----------------------------------------------------------------------------
// Setup: App Settings, Game Video/VR/Controls/FFB, Rig Profiles, CSP
// ----------------------------------------------------------------------------

export interface AppSettings {
  acRoot: string;
  accent: AccentPreset;
  steamLaunch: boolean;
  closeOnLaunch: boolean;
  checkUpdates: boolean;
  telemetryPort: number;
  webDashboardPort: number;
  cspAutoSync: boolean;
  windowScale?: number;
  armedSession?: ArmedSession;
}

export interface VideoSettings {
  width: number;
  height: number;
  refreshHz: number;
  fullscreen: boolean;
  vsync: boolean;
  fpsCap: number;
  msaa: number;
  anisotropic: number;
  worldDetail: number;
  shadowMapSize: number;
  postProcessing: boolean;
  ppFilter: string;
}

export interface VrTripleSettings {
  renderMode: "SINGLE" | "TRIPLE" | "OPENVR" | "OCULUS";
  tripleFovDeg: number;
  screenWidthMm: number;
  distanceMm: number;
  bezelMm: number;
  sideAngleDeg: number;
  vrPixelDensity: number;
}

export interface FfbConfig {
  gain: number;
  filter: number;
  minForce: number;
  kerbEffect: number;
  roadEffect: number;
  slipEffect: number;
  absEffect: number;
  enhancedUndersteer: boolean;
  gammaEnabled: boolean;
  gammaValue: number;
}

export interface RigProfile {
  name: string;
  description?: string;
  videoMode: string;
  ffbProfile?: string;
  active?: boolean;
}

export interface CspPatchInfo {
  name: string;
  version: string;
  buildNumber?: number;
  installed: boolean;
  recommended?: boolean;
  notes?: string;
}

export interface ConfigSnapshot {
  id: string;
  label: string;
  createdAt: string;
  filesCount: number;
}

// ----------------------------------------------------------------------------
// UI Notifications / Toasts
// ----------------------------------------------------------------------------

export type ToastVariant = "info" | "success" | "warning" | "error";

export interface ToastItem {
  id: string;
  title: string;
  description?: string;
  variant: ToastVariant;
  createdAt: number;
}

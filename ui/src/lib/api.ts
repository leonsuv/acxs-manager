import { convertFileSrc, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AdaptiveAiSuggestion,
  AppSettings,
  CachedDirectoryResult,
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
  DirectoryProgressEvent,
  FfbConfig,
  JoinOnlineRequest,
  JoinUri,
  JsgmeModEntry,
  LiveTelemetryFrame,
  OnlineBookingReply,
  OnlineJoinResult,
  OnlineServerDetail,
  OnlineServerSummary,
  PackageEntry,
  QuickRacePreset,
  QuickRaceRequest,
  RconResponse,
  ReplaySummary,
  RigProfile,
  Scene3DMeshData,
  ServerCspBundle,
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
// Unified Invoke Wrapper
// ============================================================================

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!isTauri()) {
    throw new Error(`Tauri backend unavailable: ${cmd}`);
  }
  return tauriInvoke<T>(cmd, args);
}

// ============================================================================
// Rust <-> TypeScript DTO Normalizers
// ============================================================================

type AnyRec = Record<string, unknown>;

function asRec(v: unknown): AnyRec {
  return v && typeof v === "object" ? (v as AnyRec) : {};
}

function asArr(v: unknown): unknown[] {
  return Array.isArray(v) ? v : [];
}

function formatUnixMs(ms: unknown): string {
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return "";
  try {
    return new Date(n).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

function formatLayoutLabel(id: string): string {
  if (!id || id === "default") return "Default";
  return id
    .replace(/^layout_?/i, "")
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

function normalizeCar(raw: unknown): CarEntry {
  const r = asRec(raw);
  const id = String(r.id ?? "");
  const bhpVal = r.bhp;
  const bhp =
    typeof bhpVal === "number" && Number.isFinite(bhpVal)
      ? `${Math.round(bhpVal)} bhp`
      : typeof bhpVal === "string" && bhpVal.trim()
        ? bhpVal
        : undefined;

  const weightVal = r.weight ?? r.weightKg;
  const weight =
    typeof weightVal === "number" && Number.isFinite(weightVal)
      ? `${Math.round(weightVal)} kg`
      : typeof weightVal === "string" && weightVal.trim()
        ? weightVal
        : undefined;

  const pwVal = r.pwRatio ?? r.pwratio;
  const pwRatio =
    typeof pwVal === "number" && Number.isFinite(pwVal)
      ? `${pwVal.toFixed(2)} kg/hp`
      : typeof pwVal === "string" && pwVal.trim()
        ? pwVal
        : undefined;

  const yearRaw = r.year;
  const parsedYear =
    typeof yearRaw === "number"
      ? yearRaw
      : typeof yearRaw === "string" && yearRaw.trim()
        ? Number.parseInt(yearRaw, 10)
        : NaN;

  return {
    id,
    name: String(r.name ?? id),
    brand: String(r.brand ?? "Unknown"),
    class: String(r.class ?? r.carClass ?? "street") || "street",
    country: typeof r.country === "string" ? r.country : undefined,
    year: Number.isFinite(parsedYear) && parsedYear > 0 ? parsedYear : null,
    bhp,
    torque: typeof r.torque === "string" ? r.torque : undefined,
    weight,
    topSpeed: typeof r.topSpeed === "string" ? r.topSpeed : undefined,
    pwRatio,
    tags: asArr(r.tags).map((t) => String(t)),
    path: String(r.path ?? ""),
    previewPath: typeof r.previewPath === "string" ? r.previewPath : null,
    badgePath: typeof r.badgePath === "string" ? r.badgePath : null,
    skinsCount: typeof r.skinsCount === "number" ? r.skinsCount : undefined,
    enabled: typeof r.enabled === "boolean" ? r.enabled : true,
    isMod: typeof r.isMod === "boolean" ? r.isMod : !id.startsWith("ks_"),
  };
}

function normalizeTrack(raw: unknown): TrackEntry {
  const r = asRec(raw);
  const id = String(r.id ?? "");
  const rawLengthM = r.lengthMeters ?? r.lengthM;
  const lengthMeters =
    typeof rawLengthM === "number" && Number.isFinite(rawLengthM) && rawLengthM > 0
      ? rawLengthM
      : null;
  const length =
    typeof r.length === "string" && r.length.trim()
      ? r.length
      : lengthMeters !== null
        ? lengthMeters >= 1000
          ? `${(lengthMeters / 1000).toFixed(2)} km`
          : `${Math.round(lengthMeters)} m`
        : undefined;

  const pitboxes =
    typeof r.pitboxes === "number" && Number.isFinite(r.pitboxes)
      ? r.pitboxes
      : null;

  const rawLayouts = asArr(r.layouts);
  const layouts =
    rawLayouts.length > 0
      ? rawLayouts.map((lay) => {
          if (typeof lay === "string") {
            return {
              id: lay,
              name: formatLayoutLabel(lay),
              lengthMeters,
              pitboxes,
              previewPath: null,
              outlinePath: null,
            };
          }
          const lr = asRec(lay);
          const lid = String(lr.id ?? "default");
          return {
            id: lid,
            name: String(lr.name ?? formatLayoutLabel(lid)),
            lengthMeters:
              typeof lr.lengthMeters === "number" ? lr.lengthMeters : lengthMeters,
            pitboxes: typeof lr.pitboxes === "number" ? lr.pitboxes : pitboxes,
            previewPath: typeof lr.previewPath === "string" ? lr.previewPath : null,
            outlinePath: typeof lr.outlinePath === "string" ? lr.outlinePath : null,
          };
        })
      : [
          {
            id: "default",
            name: "Default",
            lengthMeters,
            pitboxes,
            previewPath: null,
            outlinePath: null,
          },
        ];

  return {
    id,
    name: String(r.name ?? id),
    country: String(r.country ?? ""),
    city: typeof r.city === "string" ? r.city : undefined,
    length,
    lengthMeters,
    pitboxes,
    tags: asArr(r.tags).map((t) => String(t)),
    path: String(r.path ?? ""),
    previewPath: typeof r.previewPath === "string" ? r.previewPath : null,
    outlinePath: typeof r.outlinePath === "string" ? r.outlinePath : null,
    layouts,
    enabled: typeof r.enabled === "boolean" ? r.enabled : true,
    isMod: typeof r.isMod === "boolean" ? r.isMod : !id.startsWith("ks_"),
  };
}

function normalizeSkin(raw: unknown): SkinEntry {
  const r = asRec(raw);
  const id = String(r.id ?? r.skinId ?? "");
  return {
    id,
    name: String(r.name ?? id),
    driverName: String(r.driverName ?? r.driver ?? ""),
    team: String(r.team ?? ""),
    number: String(r.number ?? ""),
    country: String(r.country ?? ""),
    path: String(r.path ?? ""),
    previewPath: typeof r.previewPath === "string" ? r.previewPath : null,
    liveryPath: typeof r.liveryPath === "string" ? r.liveryPath : null,
  };
}

function normalizeScanResult(raw: unknown): ContentScanResult {
  const r = asRec(raw);
  const statsRaw = asRec(r.drivingStats);
  const seconds = Number(statsRaw.seconds ?? 0);
  const totalHours =
    typeof statsRaw.totalHours === "number"
      ? statsRaw.totalHours
      : Number.isFinite(seconds)
        ? seconds / 3600
        : 0;
  const totalKm = Number(statsRaw.totalKm ?? statsRaw.distanceKm ?? 0) || 0;
  const sessionsCount = Number(statsRaw.sessionsCount ?? statsRaw.sessions ?? 0) || 0;
  const totalLaps = Number(statsRaw.totalLaps ?? 0) || 0;
  const topCars = asArr(statsRaw.topCars);
  const topTracks = asArr(statsRaw.topTracks);
  const favoriteCar =
    (typeof statsRaw.favoriteCar === "string" ? statsRaw.favoriteCar : null) ??
    (topCars[0] ? String(asRec(topCars[0]).id ?? "") || null : null);
  const favoriteTrack =
    (typeof statsRaw.favoriteTrack === "string" ? statsRaw.favoriteTrack : null) ??
    (topTracks[0] ? String(asRec(topTracks[0]).id ?? "") || null : null);

  let cspBuild: string | null = null;
  if (
    typeof r.cspVersion === "string" &&
    r.cspVersion.trim() &&
    typeof r.cspBuild === "number" &&
    Number.isFinite(r.cspBuild)
  ) {
    cspBuild = `${r.cspVersion.trim()} (${r.cspBuild})`;
  } else if (typeof r.cspVersion === "string" && r.cspVersion.trim()) {
    cspBuild = r.cspVersion.trim();
  } else if (typeof r.cspBuild === "string" && r.cspBuild.trim()) {
    cspBuild = r.cspBuild.trim();
  } else if (typeof r.cspBuild === "number" && Number.isFinite(r.cspBuild)) {
    cspBuild = String(r.cspBuild);
  }

  return {
    acRoot: String(r.acRoot ?? ""),
    cars: asArr(r.cars).map(normalizeCar),
    tracks: asArr(r.tracks).map(normalizeTrack),
    gameRunning: Boolean(r.gameRunning),
    cspBuild,
    drivingStats: {
      totalKm,
      totalHours,
      totalLaps,
      favoriteCar,
      favoriteTrack,
      sessionsCount,
    },
  };
}

function normalizeEvent(raw: unknown): SpecialEvent {
  const r = asRec(raw);
  const goals = asArr(r.goals).map((g) => String(g));
  const kind = String(r.kind ?? r.mode ?? "Race");
  const modeMap: Record<string, SpecialEvent["mode"]> = {
    hotlap: "Hotlap",
    "time attack": "TimeAttack",
    timeattack: "TimeAttack",
    drift: "Drift",
    drag: "Drag",
    practice: "Practice",
    trackday: "TrackDay",
    weekend: "Weekend",
  };
  const mode: SpecialEvent["mode"] =
    modeMap[kind.toLowerCase()] ?? "QuickRace";
  const series = String(r.series ?? "").trim();
  const category = String(r.category ?? (series || kind || "Special Event"));
  const medalRaw = String(r.earnedMedal ?? r.medal ?? "None");
  const earnedMedal: SpecialEvent["earnedMedal"] =
    medalRaw === "Gold" || medalRaw === "Silver" || medalRaw === "Bronze"
      ? medalRaw
      : "None";

  return {
    id: String(r.id ?? ""),
    name: String(r.name ?? ""),
    category,
    carId: String(r.carId ?? r.car ?? ""),
    trackId: String(r.trackId ?? r.track ?? ""),
    layoutId: String(r.layoutId ?? r.layout ?? ""),
    mode,
    bronzeTarget:
      typeof r.bronzeTarget === "string" ? r.bronzeTarget : goals[0],
    silverTarget:
      typeof r.silverTarget === "string" ? r.silverTarget : goals[1],
    goldTarget: typeof r.goldTarget === "string" ? r.goldTarget : goals[2],
    earnedMedal,
    bestResult: typeof r.bestResult === "string" ? r.bestResult : null,
    description: typeof r.description === "string" ? r.description : undefined,
  };
}

function normalizeTrack3D(raw: unknown): Track3DSceneData | null {
  if (!raw || typeof raw !== "object") return null;
  const r = asRec(raw);
  let centerline: Array<[number, number, number]> = [];
  if (Array.isArray(r.centerline)) {
    centerline = r.centerline as Array<[number, number, number]>;
  } else if (Array.isArray(r.splinePoints)) {
    const pts = r.splinePoints as AnyRec[];
    const step = Math.max(1, Math.floor(pts.length / 260));
    for (let i = 0; i < pts.length; i += step) {
      const p = pts[i];
      centerline.push([Number(p.x ?? 0), Number(p.y ?? 0), Number(p.z ?? 0)]);
    }
  }
  const drsZones = asArr(r.drsZones).map((z) => {
    const zr = asRec(z);
    return { start: Number(zr.start ?? 0), end: Number(zr.end ?? 0) };
  });
  const aiHints = asArr(r.aiHints).map((h) => {
    const hr = asRec(h);
    return {
      section: String(hr.section ?? "HINT"),
      start: Number(hr.start ?? 0),
      end: Number(hr.end ?? 0),
      value: Number(hr.value ?? 0),
    };
  });
  return {
    centerline,
    drsZones,
    camerasCount: Array.isArray(r.cameras)
      ? r.cameras.length
      : typeof r.camerasCount === "number"
        ? r.camerasCount
        : 0,
    sectionsCount: Array.isArray(r.sections)
      ? r.sections.length
      : typeof r.sectionsCount === "number"
        ? r.sectionsCount
        : 0,
    aiHintsCount: Array.isArray(r.aiHints)
      ? r.aiHints.length
      : typeof r.aiHintsCount === "number"
        ? r.aiHintsCount
        : 0,
    aiHints,
  };
}

function normalizeCarPhysics(raw: unknown): CarPhysicsData | null {
  if (!raw || typeof raw !== "object") return null;
  const r = asRec(raw);
  if (Array.isArray(r.rpmCurve) && r.rpmCurve.length > 0) {
    return r as unknown as CarPhysicsData;
  }
  const dyno = asRec(r.dyno);
  const dynoPts = asArr(dyno.points).map(asRec);
  const drivetrain = asRec(r.drivetrain);
  const specs = asRec(r.specs);
  const aero = asRec(r.aero);
  const tyres = asArr(r.tyres).map(asRec);

  const rpmCurve = dynoPts.map((p) => Number(p.rpm ?? 0));
  const torqueNm = dynoPts.map((p) => Number(p.torqueNm ?? 0));
  const powerHp = dynoPts.map((p) => Number(p.powerHp ?? 0));
  const redlineRpm = Number(
    dyno.limiterRpm ?? drivetrain.limiterRpm ?? specs.maxRpm ?? rpmCurve[rpmCurve.length - 1] ?? 8000
  );
  const massKg = Number(specs.massKg ?? 0);
  const forceCurve = asArr(aero.forceCurve).map(asRec);
  const sampleAero =
    forceCurve.find((pt) => Number(pt.speedKmh) >= 180) ??
    forceCurve[forceCurve.length - 1];
  let aeroCd: number | undefined;
  let aeroCl: number | undefined;
  if (sampleAero) {
    const vMs = Number(sampleAero.speedKmh ?? 0) / 3.6;
    const q = 0.5 * 1.225 * vMs * vMs;
    if (q > 1) {
      const dragN = Number(sampleAero.totalDragN ?? 0);
      const downN = Number(sampleAero.totalDownforceN ?? 0);
      if (Number.isFinite(dragN)) aeroCd = Number((dragN / q).toFixed(2));
      if (Number.isFinite(downN)) aeroCl = Number((downN / q).toFixed(2));
    }
  }
  const tyreCompounds = tyres
    .map((t) => String(t.frontName ?? t.rearName ?? "").trim())
    .filter(Boolean);

  return {
    rpmCurve,
    torqueNm,
    powerHp,
    gearRatios: asArr(drivetrain.gears).map(Number),
    gearTopSpeedsKmh: asArr(drivetrain.gearTopSpeedsKmh).map(Number),
    redlineRpm: Number.isFinite(redlineRpm) && redlineRpm > 0 ? Math.round(redlineRpm) : 8000,
    massKg: Number.isFinite(massKg) ? Math.round(massKg) : 0,
    aeroCd,
    aeroCl,
    tyreCompounds,
    hasPackedAcd: Boolean(r.usesPackedData ?? r.hasPackedAcd),
    hasUnpackedData: Boolean(r.hasUnpackedData),
  };
}

function normalizeSfxInfo(raw: unknown): CarSfxInfo | null {
  if (!raw || typeof raw !== "object") return null;
  const r = asRec(raw);
  const banks = asArr(r.bankFiles).map(String);
  return {
    carId: String(r.carId ?? ""),
    bankPath:
      typeof r.bankPath === "string"
        ? r.bankPath
        : banks.length > 0
          ? banks[0]
          : null,
    bankSizeBytes: Number(r.bankSizeBytes ?? 0) || 0,
    guidsPresent: Boolean(r.guidsPresent ?? r.guidsPath),
  };
}

function sessionNumberLabel(session: unknown): string | undefined {
  if (typeof session === "string" && session.trim()) return session;
  switch (Number(session)) {
    case 1:
      return "Practice";
    case 2:
      return "Qualifying";
    case 3:
      return "Race";
    default:
      return undefined;
  }
}

function normalizeOnlineServer(raw: unknown): OnlineServerSummary {
  const r = asRec(raw);
  const host = String(r.host ?? r.ip ?? "");
  const rawHttpPort =
    typeof r.httpPort === "number" && r.httpPort > 0 ? r.httpPort : undefined;
  const rawRacePort =
    typeof r.port === "number" && r.port > 0
      ? r.port
      : typeof r.racePort === "number" && r.racePort > 0
        ? r.racePort
        : undefined;
  const port = rawRacePort ?? rawHttpPort ?? 9600;
  const httpPort = rawHttpPort ?? port;
  const countries = asArr(r.countries).map(String);
  const cspRaw = r.cspRequired;
  const cspRequired =
    typeof cspRaw === "number"
      ? cspRaw
      : cspRaw === true
        ? 2144
        : null;

  const rawTrack =
    (typeof r.trackId === "string" && r.trackId.trim()) ||
    (typeof r.track === "string" && r.track.trim()) ||
    (typeof r.trackName === "string" && r.trackName.trim()) ||
    "unknown";

  return {
    id: String(r.id ?? r.key ?? `${host}:${httpPort}`),
    name: String(r.name ?? `${host}:${port}`),
    host,
    port,
    httpPort,
    trackId: rawTrack,
    trackLayout:
      typeof r.trackLayout === "string" ? r.trackLayout : undefined,
    clients: Number(r.clients ?? 0) || 0,
    maxClients: Number(r.maxClients ?? r.capacity ?? 0) || 0,
    pingMs:
      typeof r.pingMs === "number"
        ? r.pingMs
        : typeof r.latencyMs === "number"
          ? r.latencyMs
          : null,
    cars: (asArr(r.cars).length > 0 ? asArr(r.cars) : asArr(r.carNames)).map(String),
    playerNames: asArr(r.playerNames).map(String),
    passworded: Boolean(r.passworded ?? r.password),
    country:
      typeof r.country === "string" && r.country.trim()
        ? r.country
        : countries[0] ?? (typeof r.region === "string" && r.region.trim() ? r.region : undefined),
    region:
      typeof r.region === "string" && r.region.trim()
        ? r.region
        : undefined,
    cspRequired,
    sessionType: sessionNumberLabel(r.sessionType ?? r.session),
    timeLeftSec: typeof r.timeLeftSec === "number" ? r.timeLeftSec : null,
    tags: asArr(r.tags).map(String),
  };
}

function normalizeServerDetail(
  raw: unknown,
  fallbackHost: string,
  fallbackPort: number
): OnlineServerDetail | null {
  if (!raw || typeof raw !== "object") return null;
  const r = asRec(raw);
  if (r.summary && Array.isArray(r.slots)) {
    return r as unknown as OnlineServerDetail;
  }
  const cars = asArr(r.cars).map((c) => {
    const cr = asRec(c);
    return {
      carId: String(cr.carId ?? cr.model ?? ""),
      skinId: String(cr.skinId ?? cr.skin ?? ""),
      driverName: String(cr.driverName ?? cr.driver ?? "") || undefined,
      connected: Boolean(cr.connected),
      owned: Boolean(cr.owned),
    };
  });
  const missingContent: OnlineServerDetail["missingContent"] = [];
  const plan = asRec(r.downloadPlan);
  const planUrls = new Map<string, string>();
  for (const item of asArr(plan.items)) {
    const ir = asRec(item);
    const key = `${String(ir.kind ?? "").toLowerCase()}:${String(ir.id ?? "")}`;
    if (typeof ir.url === "string" && ir.url) planUrls.set(key, ir.url);
  }
  const resolvedTrackId = String(r.trackId ?? r.rawTrack ?? "unknown");
  const trackOwned = Boolean(r.trackOwned);
  if (!trackOwned && resolvedTrackId && resolvedTrackId !== "unknown") {
    missingContent.push({
      kind: "track",
      id: resolvedTrackId,
      downloadUrl: planUrls.get(`track:${resolvedTrackId}`) ?? null,
    });
  }
  for (const cid of asArr(r.missingCars).map(String)) {
    missingContent.push({
      kind: "car",
      id: cid,
      downloadUrl: planUrls.get(`car:${cid}`) ?? null,
    });
  }
  const detailHttpPort =
    typeof r.httpPort === "number" && r.httpPort > 0
      ? r.httpPort
      : fallbackPort;
  const detailRacePort =
    typeof r.racePort === "number" && r.racePort > 0
      ? r.racePort
      : detailHttpPort;
  const summary: OnlineServerSummary = {
    id: `${String(r.host ?? fallbackHost)}:${detailHttpPort}`,
    name: String(r.name ?? fallbackHost),
    host: String(r.host ?? fallbackHost),
    port: detailRacePort,
    httpPort: detailHttpPort,
    trackId: resolvedTrackId,
    trackLayout: typeof r.trackLayout === "string" ? r.trackLayout : undefined,
    clients: Number(r.clients ?? 0) || 0,
    maxClients: Number(r.maxClients ?? cars.length) || 0,
    pingMs: typeof r.latencyMs === "number" ? r.latencyMs : null,
    cars: Array.from(new Set(cars.map((c) => c.carId).filter(Boolean))),
    passworded: false,
    cspRequired: typeof r.minCspBuild === "number" ? r.minCspBuild : null,
    sessionType: typeof r.session === "string" ? r.session : undefined,
  };
  return {
    summary,
    resolvedTrackId,
    trackOwned,
    minCspBuild: typeof r.minCspBuild === "number" ? r.minCspBuild : null,
    slots: cars,
    missingContent,
    features: asArr(r.features).map(String),
    description: undefined,
    bookingMode: false,
  };
}

function normalizeSessionSummary(raw: unknown): TelemetrySessionSummary {
  const r = asRec(raw);
  return {
    id: String(r.id ?? ""),
    carId: String(r.carId ?? ""),
    trackId: String(r.fullTrackId ?? r.trackId ?? ""),
    date: String(r.date ?? r.startedAt ?? ""),
    lapsCount: Number(r.lapsCount ?? r.lapCount ?? 0) || 0,
    bestLapMs: Number(r.bestLapMs ?? 0) || 0,
    topSpeedKmh: Math.round(Number(r.topSpeedKmh ?? r.maxSpeed ?? 0) || 0),
  };
}

function normalizeSessionAnalysis(raw: unknown): TelemetrySessionAnalysis | null {
  if (!raw || typeof raw !== "object") return null;
  const r = asRec(raw);
  const summary = normalizeSessionSummary(r.summary);
  const lapTimesMs = Array.isArray(r.lapTimesMs)
    ? (r.lapTimesMs as number[])
    : asArr(r.laps).map((l) => Number(asRec(l).timeMs ?? 0)).filter((ms) => ms > 0);
  const speedTrace = Array.isArray(r.speedTrace)
    ? (r.speedTrace as TelemetrySessionAnalysis["speedTrace"])
    : asArr(r.trace).map((pt) => {
        if (Array.isArray(pt)) {
          return {
            distanceM: Number(pt[0] ?? 0),
            speedKmh: Number(pt[1] ?? 0),
            throttle: 0,
            brake: 0,
            gear: 0,
          };
        }
        const pr = asRec(pt);
        return {
          distanceM: Number(pr.distanceM ?? 0),
          speedKmh: Number(pr.speedKmh ?? 0),
          throttle: Number(pr.throttle ?? 0),
          brake: Number(pr.brake ?? 0),
          gear: Number(pr.gear ?? 0),
        };
      });
  const trackPath: Array<[number, number]> = Array.isArray(r.trackPath)
    ? (r.trackPath as Array<[number, number]>)
    : asArr(r.path)
        .filter((p) => Array.isArray(p) && p.length >= 3)
        .map((p) => [Number((p as unknown[])[1]), Number((p as unknown[])[2])]);
  const corners = asArr(r.corners).map((c, idx) => {
    const cr = asRec(c);
    const num = Number(cr.cornerIndex ?? cr.number ?? idx + 1);
    return {
      cornerIndex: num,
      name: String(cr.name ?? `Turn ${num}`),
      timeLossMs: Math.round(Number(cr.timeLossMs ?? cr.timeDeltaMs ?? 0) || 0),
      entrySpeedDeltaKmh: Number(cr.entrySpeedDeltaKmh ?? 0) || 0,
      apexSpeedDeltaKmh: Number(cr.apexSpeedDeltaKmh ?? 0) || 0,
      advice: String(cr.advice ?? ""),
    };
  });
  return {
    summary,
    lapTimesMs,
    speedTrace,
    corners,
    trackPath,
  };
}

function normalizeReplay(raw: unknown): ReplaySummary {
  const r = asRec(raw);
  const filename = String(r.filename ?? "");
  const title = String(r.title ?? "");
  const sizeBytes = Number(r.sizeBytes ?? r.bytes ?? 0) || 0;
  const modifiedAt =
    typeof r.modifiedAt === "string" && r.modifiedAt
      ? r.modifiedAt
      : formatUnixMs(r.modifiedUnixMs);
  return {
    filename,
    path: String(r.path ?? ""),
    sizeBytes,
    modifiedAt,
    carId: typeof r.carId === "string" ? r.carId : undefined,
    trackId:
      typeof r.trackId === "string"
        ? r.trackId
        : title && title !== filename
          ? title
          : undefined,
    durationSec: typeof r.durationSec === "number" ? r.durationSec : undefined,
    place: typeof r.place === "string" ? r.place : undefined,
  };
}

function normalizeAccentPreset(raw: unknown): AppSettings["accent"] {
  const s = String(raw ?? "").toLowerCase();
  if (s === "telemetrycyan" || s === "telemetry_cyan") return "TelemetryCyan";
  if (s === "ambergold" || s === "amber_gold") return "AmberGold";
  if (s === "emerald") return "Emerald";
  return "MotorsportRed";
}

function normalizeSettings(raw: unknown): AppSettings {
  const r = asRec(raw);
  return {
    acRoot: String(r.acRoot ?? ""),
    accent: normalizeAccentPreset(r.accent ?? r.accentPreset),
    steamLaunch: Boolean(r.steamLaunch ?? false),
    closeOnLaunch: Boolean(r.closeOnLaunch ?? false),
    checkUpdates: r.checkUpdates !== undefined ? Boolean(r.checkUpdates) : true,
    telemetryPort: Number(r.telemetryPort ?? 9996) || 9996,
    webDashboardPort: Number(r.webDashboardPort ?? 8787) || 8787,
    cspAutoSync: r.cspAutoSync !== undefined ? Boolean(r.cspAutoSync) : true,
  };
}

const GRIP_TO_PCT: Record<string, number> = {
  Dusty: 86,
  Slow: 90,
  Green: 95,
  Fast: 98,
  Optimum: 100,
};

function gripPctToPreset(pct: unknown): string {
  const n = Number(pct);
  if (!Number.isFinite(n)) return "Optimum";
  if (n <= 87) return "Dusty";
  if (n <= 92) return "Slow";
  if (n <= 96) return "Green";
  if (n <= 99) return "Fast";
  return "Optimum";
}

function toQuickRaceDto(req: QuickRaceRequest) {
  const mins = Math.max(0, Math.min(1439, Math.round(req.timeOfDayMinutes ?? 660)));
  const rustMode =
    req.mode === "TrackDay" ||
    req.mode === "Hotlap" ||
    req.mode === "TimeAttack" ||
    req.mode === "Drift" ||
    req.mode === "Drag"
      ? req.mode
      : "Weekend";
  const practiceMinutes =
    rustMode === "Weekend"
      ? req.practiceEnabled === false
        ? 0
        : Math.max(0, Math.round(req.practiceMinutes ?? 0))
      : req.mode === "Practice"
        ? req.durationMinutes || 15
        : 0;
  const qualifyMinutes =
    rustMode === "Weekend"
      ? req.qualifyEnabled === false
        ? 0
        : Math.max(0, Math.round(req.qualifyMinutes ?? 0))
      : 0;
  const raceEnabled =
    req.mode === "Practice" ? false : req.raceEnabled !== undefined ? req.raceEnabled : true;
  const ghostOn = Boolean(req.ghostEnabled);

  return {
    track: req.trackId,
    trackLayout: req.layoutId && req.layoutId !== "default" ? req.layoutId : null,
    car: req.carId,
    skin: req.skinId || "",
    aiCount: req.aiOpponents?.length ?? 0,
    laps: req.laps || 5,
    weather: req.weatherId || "3_clear",
    timeOfDayHour: Math.floor(mins / 60),
    timeOfDayMinute: mins % 60,
    aiLevel: Math.max(0, Math.min(100, Math.round(req.aiLevel ?? 90))),
    aiAggression:
      req.aiAggression !== undefined
        ? Math.max(0, Math.min(100, Math.round(req.aiAggression)))
        : null,
    mode: rustMode,
    practiceMinutes,
    qualifyMinutes,
    raceEnabled,
    raceMinutes: req.mode === "Drift" ? Math.max(0, Math.round(req.durationMinutes || 0)) : 0,
    trackDayMinutes: Math.max(0, Math.round(req.trackDayMinutes ?? 0)),
    dragMatches: Math.max(1, Math.round(req.dragMatches ?? 3)),
    ambientTemp: req.ambientTempC ?? 24,
    roadTemp: req.roadTempC ?? 32,
    trackGrip: GRIP_TO_PCT[req.trackGripPreset] ?? 100,
    penalties: req.penaltiesEnabled ?? true,
    aiFlood: Boolean(req.aiFlood),
    weatherController: req.weatherController || null,
    playerBallast: Math.max(0, Math.round(req.playerBallastKg ?? 0)),
    playerRestrictor: Math.max(0, Math.min(100, Math.round(req.playerRestrictor ?? 0))),
    ghostCar: {
      enabled: ghostOn,
      recording: ghostOn,
      playing: ghostOn,
      load: ghostOn,
      file: "",
      advantage_centis: Math.round((req.ghostAdvantageSec ?? 0) * 100),
    },
    writeAssists: false,
    assists: {
      damage: Math.max(0, Math.min(100, Math.round(req.damageMultiplier ?? 100))),
      fuel_rate: Math.max(0, Math.round((req.fuelRate ?? 100) / 100)),
      tyre_wear: Math.max(0, Math.round((req.tyreWearRate ?? 100) / 100)),
    },
    opponents: (req.aiOpponents ?? []).map((o) => ({
      car: o.carId,
      skin: o.skinId ?? "",
      driver_name: o.driverName ?? "",
      ai_level: o.aiLevel,
      ai_aggression: o.aiAggression,
      ballast: o.ballastKg ?? 0,
      restrictor: o.restrictor ?? 0,
    })),
  };
}

function normalizePresetDto(raw: unknown): QuickRacePreset {
  const item = asRec(raw);
  if (item.request && typeof item.request === "object") {
    return item as unknown as QuickRacePreset;
  }
  const r = asRec(item.race);
  const ghost = asRec(r.ghostCar);
  const assists = asRec(r.assists);
  const practiceMin = Number(r.practiceMinutes ?? 0) || 0;
  const qualifyMin = Number(r.qualifyMinutes ?? 0) || 0;
  const trackDayMin = Number(r.trackDayMinutes ?? 0) || 0;
  const raceMin = Number(r.raceMinutes ?? 0) || 0;
  const hour = Number(r.timeOfDayHour ?? 11) || 0;
  const minute = Number(r.timeOfDayMinute ?? 0) || 0;
  return {
    name: String(item.name ?? "Preset"),
    description:
      typeof item.description === "string"
        ? item.description
        : `${String(r.car ?? "")} @ ${String(r.track ?? "")}`,
    createdAt: typeof item.createdAt === "string" ? item.createdAt : undefined,
    request: {
      carId: String(r.car ?? ""),
      skinId: String(r.skin ?? ""),
      trackId: String(r.track ?? ""),
      layoutId: typeof r.trackLayout === "string" && r.trackLayout ? r.trackLayout : "default",
      mode: (r.mode as QuickRaceRequest["mode"]) ?? "Weekend",
      laps: Number(r.laps ?? 8) || 8,
      durationMinutes: raceMin || trackDayMin || 20,
      practiceEnabled: practiceMin > 0,
      practiceMinutes: practiceMin || 15,
      qualifyEnabled: qualifyMin > 0,
      qualifyMinutes: qualifyMin || 10,
      raceEnabled: r.raceEnabled !== undefined ? Boolean(r.raceEnabled) : true,
      trackDayMinutes: trackDayMin,
      dragMatches: Number(r.dragMatches ?? 3) || 3,
      playerBallastKg: Number(r.playerBallast ?? 0) || 0,
      playerRestrictor: Number(r.playerRestrictor ?? 0) || 0,
      aiLevel: Number(r.aiLevel ?? 94) || 94,
      aiAggression: typeof r.aiAggression === "number" ? r.aiAggression : 45,
      aiOpponents: asArr(r.opponents).map((op) => {
        const o = asRec(op);
        return {
          carId: String(o.car ?? ""),
          skinId: typeof o.skin === "string" ? o.skin : "",
          driverName: String(o.driver_name ?? o.name ?? ""),
          aiLevel: Number(o.ai_level ?? 90) || 90,
          aiAggression: Number(o.ai_aggression ?? 45) || 45,
          ballastKg: Number(o.ballast ?? 0) || 0,
          restrictor: Number(o.restrictor ?? 0) || 0,
        };
      }),
      timeOfDayMinutes: hour * 60 + minute,
      weatherId: String(r.weather ?? "3_clear"),
      weatherController: typeof r.weatherController === "string" ? r.weatherController : undefined,
      ambientTempC: Number(r.ambientTemp ?? 24),
      roadTempC: Number(r.roadTemp ?? 32),
      trackGripPreset: gripPctToPreset(r.trackGrip),
      ghostEnabled: Boolean(ghost.enabled),
      ghostAdvantageSec: Number(((Number(ghost.advantage_centis ?? 0) || 0) / 100).toFixed(1)),
      penaltiesEnabled: r.penalties !== undefined ? Boolean(r.penalties) : true,
      damageMultiplier: Number(assists.damage ?? 50),
      fuelRate: Number(assists.fuel_rate ?? 1) * 100,
      tyreWearRate: Number(assists.tyre_wear ?? 1) * 100,
      aiFlood: Boolean(r.aiFlood),
    },
  };
}

function normalizeAdaptiveAi(
  raw: unknown,
  fallbackCarId = ""
): AdaptiveAiSuggestion | null {
  if (!raw || typeof raw !== "object") return null;
  const r = asRec(raw);

  const rawSlots = asArr(r.slots);
  const rawOpponents = asArr(r.opponents);

  const opponents =
    rawOpponents.length > 0
      ? rawOpponents.map((op, i) => {
          const o = asRec(op);
          const aiLevelNum = Number(o.aiLevel ?? o.ai_level ?? 90);
          const aiAggNum = Number(o.aiAggression ?? o.ai_aggression ?? 40);
          const ballastNum = Number(o.ballastKg ?? o.ballast_kg ?? 0);
          return {
            carId: String(o.carId ?? o.car ?? fallbackCarId),
            skinId: typeof o.skinId === "string" ? o.skinId : undefined,
            driverName: String(o.driverName ?? `AI Driver ${i + 1}`),
            aiLevel: Number.isFinite(aiLevelNum) ? aiLevelNum : 90,
            aiAggression: Number.isFinite(aiAggNum) ? aiAggNum : 40,
            ballastKg: Number.isFinite(ballastNum) ? ballastNum : 0,
          };
        })
      : rawSlots.map((sl, i) => {
          const s = asRec(sl);
          const aiLevelNum = Number(s.aiLevel ?? s.ai_level ?? 90);
          const aiAggNum = Number(s.aiAggression ?? s.ai_aggression ?? 40);
          const ballastNum = Number(s.ballastKg ?? s.ballast_kg ?? 0);
          return {
            carId: fallbackCarId,
            driverName: `AI Driver ${i + 1}`,
            aiLevel: Number.isFinite(aiLevelNum) ? aiLevelNum : 90,
            aiAggression: Number.isFinite(aiAggNum) ? aiAggNum : 40,
            ballastKg: Number.isFinite(ballastNum) ? ballastNum : 0,
          };
        });

  const recRaw = Number(r.recommendedLevel ?? r.targetLevel ?? r.target_level);
  const recommendedLevel = Number.isFinite(recRaw)
    ? recRaw
    : opponents.length > 0
      ? Math.round(
          opponents.reduce((sum, o) => sum + o.aiLevel, 0) / opponents.length
        )
      : 90;

  const oppLevels = opponents
    .map((o) => o.aiLevel)
    .filter((lvl) => Number.isFinite(lvl));
  const minRaw = Number(r.minLevel);
  const maxRaw = Number(r.maxLevel);
  const minLevel = Number.isFinite(minRaw)
    ? minRaw
    : oppLevels.length > 0
      ? Math.min(...oppLevels)
      : Math.max(70, recommendedLevel - 2);
  const maxLevel = Number.isFinite(maxRaw)
    ? maxRaw
    : oppLevels.length > 0
      ? Math.max(...oppLevels)
      : Math.min(100, recommendedLevel + 2);

  const hasBest =
    r.playerBestMs != null && Number.isFinite(Number(r.playerBestMs));
  const hasGap =
    r.lastRaceGapMsPerLap != null &&
    Number.isFinite(Number(r.lastRaceGapMsPerLap));
  const confRaw = Number(r.confidence);
  const confidence = Number.isFinite(confRaw)
    ? Math.max(0, Math.min(1, confRaw))
    : hasBest && hasGap
      ? 0.9
      : hasBest || hasGap
        ? 0.75
        : 0.5;

  const lapsRaw = Number(r.sampleLaps);
  const sampleLaps = Number.isFinite(lapsRaw)
    ? lapsRaw
    : (hasBest ? 1 : 0) + (hasGap ? 1 : 0);

  return {
    recommendedLevel,
    minLevel,
    maxLevel,
    confidence,
    sampleLaps,
    opponents,
  };
}

let cachedRawVideoSettings: AnyRec | null = null;
let cachedRawVrTripleSettings: AnyRec | null = null;
let cachedRawFfbConfig: AnyRec | null = null;

function toRustCameraMode(
  mode?: "SINGLE" | "TRIPLE" | "OPENVR" | "OCULUS" | "Default" | "Triple" | "Oculus" | "OpenVr"
): "Default" | "Triple" | "Oculus" | "OpenVr" {
  switch (String(mode ?? "Default").toUpperCase()) {
    case "TRIPLE":
      return "Triple";
    case "OCULUS":
      return "Oculus";
    case "OPENVR":
      return "OpenVr";
    default:
      return "Default";
  }
}

function fromRustCameraMode(mode: unknown): VrTripleSettings["renderMode"] {
  switch (String(mode ?? "").toUpperCase()) {
    case "TRIPLE":
      return "TRIPLE";
    case "OCULUS":
      return "OCULUS";
    case "OPENVR":
      return "OPENVR";
    default:
      return "SINGLE";
  }
}

function normalizeVideoSettings(raw: unknown): VideoSettings {
  const r = asRec(raw);
  cachedRawVideoSettings = r;
  const rawAa = Number(r.aa_samples ?? r.msaa ?? 4);
  const rawShadow = Number(r.shadow_map_size ?? r.shadowMapSize ?? 2048);
  return {
    width: Number(r.width ?? 2560) || 2560,
    height: Number(r.height ?? 1440) || 1440,
    refreshHz: Number(r.refresh_hz ?? r.refreshHz ?? 144) || 144,
    fullscreen: r.fullscreen !== undefined ? Boolean(r.fullscreen) : true,
    vsync: Boolean(r.vsync ?? false),
    fpsCap: Number(r.fps_cap ?? r.fpsCap ?? 0) || 0,
    msaa: rawAa <= 1 ? 0 : rawAa,
    anisotropic: Number(r.anisotropic ?? 8) || 0,
    worldDetail: Number(r.world_detail ?? r.worldDetail ?? 4),
    shadowMapSize: rawShadow < 0 ? 0 : rawShadow,
    postProcessing:
      r.post_processing !== undefined
        ? Boolean(r.post_processing)
        : r.postProcessing !== undefined
          ? Boolean(r.postProcessing)
          : true,
    ppFilter: String(r.ppFilter ?? "default"),
    cameraMode: toRustCameraMode(r.camera_mode as VideoSettings["cameraMode"]),
  };
}

function normalizeVrTripleSettings(raw: unknown): VrTripleSettings {
  const r = asRec(raw);
  cachedRawVrTripleSettings = r;
  const triple = asRec(r.tripleScreen ?? r.triple_screen);
  const openvr = asRec(r.openvr);
  const oculus = asRec(r.oculus);
  const rawMode =
    r.renderMode ??
    (cachedRawVideoSettings?.camera_mode
      ? fromRustCameraMode(cachedRawVideoSettings.camera_mode)
      : "SINGLE");
  return {
    renderMode: fromRustCameraMode(rawMode),
    tripleFovDeg: 54,
    screenWidthMm: Number(r.screenWidthMm ?? triple.width_mm ?? 597) || 597,
    distanceMm: Number(r.distanceMm ?? triple.distance_mm ?? 650) || 650,
    bezelMm: Number(r.bezelMm ?? triple.bezel_mm ?? 10) || 10,
    sideAngleDeg: Number(r.sideAngleDeg ?? triple.angle_deg ?? 45) || 45,
    vrPixelDensity:
      Number(
        r.vrPixelDensity ??
          openvr.pixel_per_display ??
          oculus.pixel_per_display ??
          1.2
      ) || 1.2,
  };
}

function normalizeFfbConfig(raw: unknown): FfbConfig {
  const r = asRec(raw);
  cachedRawFfbConfig = r;
  const pp = asRec(r.postProcess ?? r.post_process);
  const tweaks = asRec(r.tweaks);
  return {
    gain: Number(r.gain ?? 0.75),
    filter: Number(r.filter ?? 0),
    minForce: Number(r.minForce ?? 0.02),
    kerbEffect: Number(r.kerbEffect ?? 0.3),
    roadEffect: Number(r.roadEffect ?? 0.25),
    slipEffect: Number(r.slipEffect ?? 0.15),
    absEffect: Number(r.absEffect ?? 0.2),
    enhancedUndersteer: Boolean(
      r.enhancedUndersteer ?? tweaks.strutFfbAdjustment ?? false
    ),
    gammaEnabled: Boolean(r.gammaEnabled ?? pp.enabled ?? false),
    gammaValue: Number(r.gammaValue ?? pp.gamma ?? 1.0) || 1.0,
  };
}

// Track section mapping for setups so saveSetup can send SetupValueDto[]
const setupSectionByKey = new Map<string, { section: string; key: string }>();

// ============================================================================
// Typed API Facade (All 6 Pillars)
// ============================================================================

export const api = {
  // Content
  scanContent: async () =>
    normalizeScanResult(await invoke<unknown>("scan_content")),
  searchCars: async (queryStr: string) =>
    asArr(await invoke<unknown>("search_cars", { queryStr })).map(normalizeCar),
  searchTracks: async (queryStr: string) =>
    asArr(await invoke<unknown>("search_tracks", { queryStr })).map(normalizeTrack),
  getCarSkins: async (carPath: string) =>
    asArr(await invoke<unknown>("get_car_skins", { carPath })).map(normalizeSkin),
  saveSkinMeta: (
    skinPath: string,
    meta: {
      skinname?: string;
      drivername?: string;
      team?: string;
      number?: string;
      country?: string;
    }
  ) => invoke<string | null>("save_skin_meta", { skinPath, meta }),
  openPathInExplorer: (path: string) => invoke<void>("open_path_in_explorer", { path }),

  // Drive
  launchQuickRace: (req: QuickRaceRequest) =>
    invoke<string>("launch_quick_race", {
      req: toQuickRaceDto(req),
    }),
  launchGameDirect: () => invoke<string>("launch_game_direct"),
  getPresets: async () =>
    asArr(await invoke<unknown>("get_presets")).map(normalizePresetDto),
  savePreset: (preset: QuickRacePreset) =>
    invoke<void>("save_preset", {
      preset: {
        name: preset.name,
        race: toQuickRaceDto(preset.request),
      },
    }),
  deletePreset: (name: string) => invoke<void>("delete_preset", { name }),
  computeAdaptiveAi: async (
    carId: string,
    trackId: string,
    count: number,
    spread: number
  ): Promise<AdaptiveAiSuggestion | null> =>
    normalizeAdaptiveAi(
      await invoke<unknown>("compute_adaptive_ai", {
        carId,
        trackId,
        count,
        spread,
      }),
      carId
    ),
  listEvents: async () =>
    asArr(await invoke<unknown>("list_events")).map(normalizeEvent),
  launchEvent: (eventId: string) => invoke<string>("launch_event", { eventId }),
  listChampionships: async () => {
    const raw = asArr(await invoke<unknown>("list_championships"));
    return raw.map((item) => {
      const r = asRec(item);
      if (r.championship) {
        const c = asRec(r.championship);
        return {
          name: String(c.name ?? ""),
          carClass: String(c.carClass ?? c.car_class ?? "GT3"),
          playerCarId: String(c.playerCarId ?? c.car ?? ""),
          rounds: asArr(c.rounds).map((rd) => {
            const rr = asRec(rd);
            return {
              name: String(rr.name ?? rr.track ?? "Round"),
              trackId: String(rr.trackId ?? rr.track ?? ""),
              layoutId: String(rr.layoutId ?? rr.layout ?? "default"),
              laps: Number(rr.laps ?? 10) || 10,
              weatherId: String(rr.weatherId ?? rr.weather ?? "3_clear"),
              completed: Boolean(rr.completed ?? (Array.isArray(rr.results) && rr.results.length > 0)),
            };
          }),
          pointsSystem: asArr(c.pointsSystem).map(Number),
          standings: asArr(r.standings).map((st) => {
            const sr = asRec(st);
            return {
              driver: String(sr.driver ?? ""),
              points: Number(sr.points ?? 0) || 0,
              wins: Number(sr.wins ?? 0) || 0,
            };
          }),
        } as Championship;
      }
      return item as Championship;
    });
  },
  saveChampionship: (champ: Championship) => invoke<void>("save_championship", { champ }),
  launchChampionshipRound: (champName: string, roundIdx: number) =>
    invoke<string>("launch_championship_round", { champName, roundIdx }),
  importRaceOutResults: () => invoke<unknown>("import_race_out_results"),
  listWeatherControllers: async (): Promise<WeatherController[]> => {
    const raw = await invoke<unknown>("list_weather_controllers");
    if (Array.isArray(raw)) return raw as WeatherController[];
    const r = asRec(raw);
    return asArr(r.controllers).map((c) => {
      if (typeof c === "string") {
        return { id: c, name: c };
      }
      const cr = asRec(c);
      return {
        id: String(cr.id ?? cr.name ?? ""),
        name: String(cr.name ?? cr.id ?? ""),
        author: typeof cr.author === "string" ? cr.author : undefined,
        version: typeof cr.version === "string" ? cr.version : undefined,
      };
    });
  },

  // Online
  getCachedDirectoryServers: async (): Promise<CachedDirectoryResult> => {
    const raw = asRec(await invoke<unknown>("get_cached_directory_servers"));
    return {
      servers: asArr(raw.servers).map(normalizeOnlineServer),
      savedUnix: typeof raw.savedUnix === "number" ? raw.savedUnix : null,
    };
  },
  fetchDirectoryServers: async (
    query = "",
    hasPlayers = false,
    maxPages = 60,
    requestId?: number
  ) =>
    asArr(
      await invoke<unknown>("fetch_directory_servers", {
        query,
        hasPlayers,
        maxPages,
        requestId,
      })
    ).map(normalizeOnlineServer),
  cancelDirectorySearch: () => invoke<void>("cancel_directory_search"),
  onDirectoryProgress: async (
    handler: (payload: DirectoryProgressEvent) => void
  ): Promise<() => void> => {
    if (!isTauri()) {
      return () => {};
    }
    return listen<unknown>("directory:progress", (event) => {
      const r = asRec(event.payload);
      handler({
        requestId: Number(r.requestId ?? 0),
        servers: asArr(r.servers).map(normalizeOnlineServer),
        loaded: Number(r.loaded ?? 0),
        total: Number(r.total ?? 0),
        done: Boolean(r.done),
      });
    });
  },
  fetchServerDetail: async (host: string, port: number) =>
    normalizeServerDetail(
      await invoke<unknown>("fetch_server_detail", { host, port }),
      host,
      port
    ) as OnlineServerDetail,
  listWatchlist: async (): Promise<WatchlistEntry[]> =>
    asArr(await invoke<unknown>("list_watchlist")).map((w) => {
      const wr = asRec(w);
      const host = String(wr.host ?? "");
      const port = Number(wr.port ?? 8081);
      const rawName = typeof wr.name === "string" ? wr.name.trim() : "";
      return {
        name: rawName || `${host}:${port}`,
        host,
        port,
        notes: typeof wr.notes === "string" ? wr.notes : undefined,
        favorite: Boolean(wr.favorite ?? true),
      };
    }),
  saveWatchlist: (entries: WatchlistEntry[]) => invoke<void>("save_watchlist", { entries }),
  listRecentServers: async () =>
    asArr(await invoke<unknown>("list_recent_servers")).map(normalizeOnlineServer),
  scanLanServers: async () =>
    asArr(await invoke<unknown>("scan_lan_servers")).map(normalizeOnlineServer),
  parseJoinUri: (raw: string) => invoke<JoinUri>("parse_join_uri", { raw }),
  formatInviteUri: (host: string, port: number, password?: string) =>
    invoke<string>("format_invite_uri", { host, port, password }),
  joinOnlineServer: (req: JoinOnlineRequest) =>
    invoke<OnlineJoinResult | null>("join_online_server", {
      req: {
        ip: req.host,
        port: req.port,
        httpPort: req.httpPort ?? req.port,
        serverName: req.serverName ?? "",
        track: req.track ?? "",
        trackLayout: req.trackLayout ?? null,
        car: req.carId,
        skin: req.skinId || "",
        driver: req.driverName ?? "",
        password: req.password ?? "",
        features: req.features ?? [],
      },
    }),
  bookOnlineServer: (req: JoinOnlineRequest) =>
    invoke<OnlineBookingReply | null>("book_online_server", {
      req: {
        host: req.host,
        httpPort: req.httpPort ?? req.port,
        driverName: req.driverName ?? "Driver",
        carModel: req.carId,
        carSkin: req.skinId || "",
        password: req.password ?? "",
      },
    }),
  loadDedicatedServer: async (dir: string): Promise<DedicatedServerConfig> => {
    const raw = asRec(await invoke<unknown>("load_dedicated_server", { dir }));
    const c = raw.cfg ? asRec(raw.cfg) : raw;
    const practice = asRec(c.practice);
    const qualify = asRec(c.qualify);
    const race = asRec(c.race);
    const votingPluginsRaw =
      c.votingPlugins && typeof c.votingPlugins === "object"
        ? asRec(c.votingPlugins)
        : null;
    const votingPlugins = votingPluginsRaw ?? {};
    const sunAngle =
      typeof c.sunAngle === "number" && Number.isFinite(c.sunAngle)
        ? c.sunAngle
        : undefined;
    const dtRaw =
      c.dynamicTrack && typeof c.dynamicTrack === "object"
        ? asRec(c.dynamicTrack)
        : null;
    const realRaw =
      c.realism && typeof c.realism === "object" ? asRec(c.realism) : null;
    return {
      name: String(c.name ?? "ACXS Dedicated"),
      track: String(c.track ?? "spa"),
      layout: String(c.layout ?? (c.configTrack || "default")),
      cars: asArr(c.cars).map(String),
      maxClients: Number(c.maxClients ?? 24) || 24,
      udpPort: Number(c.udpPort ?? 9600) || 9600,
      tcpPort: Number(c.tcpPort ?? 9600) || 9600,
      httpPort: Number(c.httpPort ?? 8081) || 8081,
      password: String(c.password ?? ""),
      adminPassword: String(c.adminPassword ?? ""),
      registerToLobby: Boolean(
        c.registerToLobby ??
          votingPlugins.register_to_lobby ??
          votingPlugins.registerToLobby ??
          true
      ),
      loopMode: Boolean(c.loopMode ?? true),
      pickupMode: Boolean(c.pickupMode ?? true),
      practiceMinutes:
        Number(
          c.practiceMinutes ??
            practice.time_min ??
            practice.timeMin ??
            practice.time ??
            15
        ) || 0,
      qualifyMinutes:
        Number(
          c.qualifyMinutes ??
            qualify.time_min ??
            qualify.timeMin ??
            qualify.time ??
            10
        ) || 0,
      raceLaps: Number(c.raceLaps ?? race.laps ?? 12) || 0,
      sunAngle,
      dynamicTrack: dtRaw
        ? {
            sessionStart: Number(dtRaw.sessionStart ?? 95),
            randomness: Number(dtRaw.randomness ?? 2),
            sessionTransfer: Number(dtRaw.sessionTransfer ?? 80),
            lapGain: Number(dtRaw.lapGain ?? 10),
          }
        : undefined,
      realism: realRaw
        ? {
            absAllowed: Number(realRaw.abs_allowed ?? 1),
            tcAllowed: Number(realRaw.tc_allowed ?? 1),
            stabilityAllowed: Boolean(realRaw.stability_allowed ?? false),
            autoclutchAllowed: Boolean(realRaw.autoclutch_allowed ?? false),
            tyreBlanketsAllowed: Boolean(realRaw.tyre_blankets_allowed ?? true),
            forceVirtualMirror: Boolean(realRaw.force_virtual_mirror ?? true),
            fuelRate: Number(realRaw.fuel_rate ?? 100),
            damageMultiplier: Number(realRaw.damage_multiplier ?? 100),
            tyreWearRate: Number(realRaw.tyre_wear_rate ?? 100),
            allowedTyresOut: Number(realRaw.allowed_tyres_out ?? 2),
          }
        : undefined,
      votingPlugins: votingPluginsRaw,
    };
  },
  saveDedicatedServer: (dir: string, cfg: DedicatedServerConfig, entries: unknown[]) =>
    invoke<void>("save_dedicated_server", {
      dir,
      cfg: {
        name: cfg.name,
        password: cfg.password,
        adminPassword: cfg.adminPassword,
        cars: cfg.cars,
        track: cfg.track,
        configTrack: cfg.layout === "default" ? "" : cfg.layout,
        maxClients: cfg.maxClients,
        udpPort: cfg.udpPort,
        tcpPort: cfg.tcpPort,
        httpPort: cfg.httpPort,
        loopMode: cfg.loopMode,
        sunAngle: cfg.sunAngle ?? 48,
        pickupMode: cfg.pickupMode,
        dynamicTrack: cfg.dynamicTrack
          ? {
              sessionStart: cfg.dynamicTrack.sessionStart,
              randomness: cfg.dynamicTrack.randomness,
              sessionTransfer: cfg.dynamicTrack.sessionTransfer,
              lapGain: cfg.dynamicTrack.lapGain,
            }
          : null,
        booking: null,
        practice:
          cfg.practiceMinutes > 0
            ? { name: "Practice", time_min: cfg.practiceMinutes, is_open: 1 }
            : null,
        qualify:
          cfg.qualifyMinutes > 0
            ? { name: "Qualify", time_min: cfg.qualifyMinutes, is_open: 1 }
            : null,
        race:
          cfg.raceLaps > 0
            ? {
                name: "Race",
                laps: cfg.raceLaps,
                time_min: 0,
                wait_time_sec: 60,
                is_open: 1,
                reversed_grid_positions: 0,
              }
            : null,
        weather: [],
        realism: cfg.realism
          ? {
              abs_allowed: cfg.realism.absAllowed,
              tc_allowed: cfg.realism.tcAllowed,
              stability_allowed: cfg.realism.stabilityAllowed,
              autoclutch_allowed: cfg.realism.autoclutchAllowed,
              tyre_blankets_allowed: cfg.realism.tyreBlanketsAllowed,
              force_virtual_mirror: cfg.realism.forceVirtualMirror,
              fuel_rate: cfg.realism.fuelRate,
              damage_multiplier: cfg.realism.damageMultiplier,
              tyre_wear_rate: cfg.realism.tyreWearRate,
              allowed_tyres_out: cfg.realism.allowedTyresOut,
              max_ballast_kg: 50,
              qualify_max_wait_perc: 120,
              start_rule: 2,
              race_gas_penalty_disabled: false,
              legal_tyres: [],
              fixed_setup: "",
            }
          : null,
        votingPlugins: {
          kick_quorum: 85,
          voting_quorum: 80,
          vote_duration: 20,
          blacklist_mode: 1,
          max_contacts_per_km: -1,
          client_send_interval_hz: 18,
          udp_plugin_local_port: 0,
          udp_plugin_address: "",
          auth_plugin_address: "",
          ...(cfg.votingPlugins ?? {}),
          register_to_lobby: cfg.registerToLobby,
        },
      },
      entries,
    }),
  loadServerCsp: (dir: string) =>
    invoke<ServerCspBundle | null>("load_server_csp", { dir }),
  saveServerCsp: (dir: string, csp: ServerCspBundle) =>
    invoke<void>("save_server_csp", { dir, csp }),
  startDedicatedServer: (dir: string) => invoke<DedicatedServerStatus>("start_dedicated_server", { dir }),
  stopDedicatedServer: () => invoke<void>("stop_dedicated_server"),
  getDedicatedServerStatus: () => invoke<DedicatedServerStatus>("get_dedicated_server_status"),
  rconConnect: async (host: string, port: number): Promise<RconResponse> => {
    const raw = asRec(await invoke<unknown>("rcon_connect", { host, port }));
    return {
      connected: typeof raw.connected === "boolean" ? raw.connected : Boolean(raw.serverAddr),
      output: String(raw.output ?? (raw.serverAddr ? `Bound ${raw.localAddr} â†’ ${raw.serverAddr}` : "")),
    };
  },
  rconSendCommand: async (cmd: string): Promise<RconResponse> => {
    const raw = asRec(await invoke<unknown>("rcon_send_command", { cmd }));
    return {
      connected: Boolean(raw.connected ?? raw.sent ?? true),
      output: String(raw.output ?? raw.event ?? ""),
    };
  },

  // Garage
  loadCar3DScene: (carPath: string, skin = "") =>
    invoke<Scene3DMeshData>("load_car_3d_scene", { carPath, skin }),
  loadTrack3DScene: async (trackPath: string, layout = "") =>
    normalizeTrack3D(
      await invoke<unknown>("load_track_3d_scene", {
        trackPath,
        layout: layout === "default" ? "" : layout,
      })
    ) as Track3DSceneData,
  getCarPhysics: async (carPath: string) =>
    normalizeCarPhysics(await invoke<unknown>("get_car_physics", { carPath })) as CarPhysicsData,
  getCarTuning: (carPath: string) => invoke<CarTuningValues>("get_car_tuning", { carPath }),
  applyCarTuning: (carPath: string, tuning: CarTuningValues) =>
    invoke<void>("apply_car_tuning", { carPath, tuning }),
  unpackCarAcd: (carPath: string) => invoke<string>("unpack_car_acd", { carPath }),
  packCarAcd: (carPath: string) => invoke<string>("pack_car_acd", { carPath }),
  inspectCarSfx: async (carPath: string) =>
    normalizeSfxInfo(await invoke<unknown>("inspect_car_sfx", { carPath })) as CarSfxInfo,
  swapCarSfx: (targetCar: string, donorCar: string) =>
    invoke<string>("swap_car_sfx", { targetCar, donorCar }),
  listSetups: async (): Promise<SetupFileSummary[]> =>
    asArr(await invoke<unknown>("list_setups")).map((s) => {
      const sr = asRec(s);
      return {
        name: String(sr.name ?? ""),
        carId: String(sr.carId ?? ""),
        trackId: String(sr.trackId ?? sr.track ?? "general"),
        path: String(sr.path ?? ""),
        modifiedAt:
          typeof sr.modifiedAt === "string"
            ? sr.modifiedAt
            : formatUnixMs(sr.modifiedUnixMs) || undefined,
      };
    }),
  readSetup: async (path: string, carPath: string): Promise<SetupDocument> => {
    const raw = asRec(await invoke<unknown>("read_setup", { path, carPath }));
    if (Array.isArray(raw.parameters)) {
      return raw as unknown as SetupDocument;
    }
    const vals = asArr(raw.values).map(asRec);
    const schema = asArr(raw.schema).map(asRec);
    const valBySection = new Map<string, { key: string; value: string }>();
    for (const v of vals) {
      valBySection.set(String(v.section ?? "").toUpperCase(), {
        key: String(v.key ?? "VALUE"),
        value: String(v.value ?? "0"),
      });
    }
    const parameters: SetupDocument["parameters"] = [];
    if (schema.length > 0) {
      for (const sp of schema) {
        const section = String(sp.section ?? "");
        const matched = valBySection.get(section.toUpperCase());
        const numVal = Number(matched?.value ?? sp.min ?? 0);
        if (!Number.isFinite(numVal)) continue;
        const paramKey = section;
        setupSectionByKey.set(paramKey, {
          section,
          key: matched?.key ?? "VALUE",
        });
        parameters.push({
          key: paramKey,
          section: String(sp.tab ?? "General"),
          label: String(sp.name ?? section),
          value: numVal,
          min: Number(sp.min ?? 0),
          max: Number(sp.max ?? 100),
          step: Math.max(0.01, Number(sp.step ?? 1) || 1),
          unit: sp.showClicks ? " clicks" : "",
        });
      }
    } else {
      for (const v of vals) {
        const section = String(v.section ?? "GENERAL");
        const key = String(v.key ?? "VALUE");
        const numVal = Number(v.value ?? "");
        if (!Number.isFinite(numVal)) continue;
        const paramKey = `${section}.${key}`;
        setupSectionByKey.set(paramKey, { section, key });
        const abs = Math.max(Math.abs(numVal), 10);
        parameters.push({
          key: paramKey,
          section: "Parameters",
          label: section,
          value: numVal,
          min: numVal < 0 ? Math.floor(numVal * 2) : 0,
          max: Math.ceil(abs * 2),
          step: Number.isInteger(numVal) ? 1 : 0.1,
        });
      }
    }
    return {
      path: String(raw.path ?? path),
      carId: "",
      trackId: "",
      parameters,
    };
  },
  saveSetup: (path: string, values: Record<string, number>) => {
    const deduped = new Map<string, { section: string; key: string; value: string }>();
    for (const [k, v] of Object.entries(values)) {
      if (!Number.isFinite(v)) continue;
      const mapped = setupSectionByKey.get(k);
      let entry: { section: string; key: string; value: string };
      if (mapped) {
        entry = { section: mapped.section, key: mapped.key, value: String(v) };
      } else {
        const dot = k.indexOf(".");
        if (dot > 0) {
          entry = { section: k.slice(0, dot), key: k.slice(dot + 1), value: String(v) };
        } else {
          entry = { section: k, key: "VALUE", value: String(v) };
        }
      }
      deduped.set(`${entry.section.toUpperCase()}.${entry.key.toUpperCase()}`, entry);
    }
    return invoke<void>("save_setup", { path, values: Array.from(deduped.values()) });
  },
  diffSetups: async (left: string, right: string): Promise<SetupDiffEntry[]> =>
    asArr(await invoke<unknown>("diff_setups", { left, right })).map((d) => {
      const dr = asRec(d);
      if ("leftValue" in dr || "rightValue" in dr) {
        return dr as unknown as SetupDiffEntry;
      }
      const section = String(dr.section ?? "");
      const key = String(dr.key ?? "");
      const leftVal = dr.left ?? "â€”";
      const rightVal = dr.right ?? "â€”";
      const ln = Number(dr.left);
      const rn = Number(dr.right);
      const delta =
        Number.isFinite(ln) && Number.isFinite(rn)
          ? Number((rn - ln).toFixed(2))
          : undefined;
      return {
        key: section ? `${section}.${key}` : key,
        label: section || key,
        leftValue: String(leftVal),
        rightValue: String(rightVal),
        delta,
      };
    }),
  inspectPackage: async (path: string): Promise<PackageEntry[]> => {
    const raw = await invoke<unknown>("inspect_package", { path });
    if (Array.isArray(raw)) return raw as PackageEntry[];
    const r = asRec(raw);
    return asArr(r.items).map((it) => {
      const ir = asRec(it);
      const kindRaw = String(ir.kind ?? "mod").toLowerCase();
      const kind: PackageEntry["kind"] =
        kindRaw === "car" || kindRaw === "track" || kindRaw === "skin" || kindRaw === "csp" || kindRaw === "mod"
          ? kindRaw
          : kindRaw.includes("skin")
            ? "skin"
            : kindRaw.includes("app")
              ? "app"
              : kindRaw.includes("csp") || kindRaw.includes("pp")
                ? "csp"
                : "mod";
      return {
        id: String(ir.id ?? ""),
        kind,
        name: String(ir.name ?? ir.id ?? ""),
        targetPath: String(ir.targetPath ?? ir.destRel ?? ""),
        sizeBytes: Number(ir.sizeBytes ?? ir.totalSize ?? 0) || 0,
        selected: ir.selected !== undefined ? Boolean(ir.selected) : true,
      };
    });
  },
  installPackage: (path: string, selectedIds: string[]) =>
    invoke<string>("install_package", { path, selectedIds }),
  listJsgmeMods: async (): Promise<JsgmeModEntry[]> =>
    asArr(await invoke<unknown>("list_jsgme_mods")).map((m) => {
      const mr = asRec(m);
      return {
        name: String(mr.name ?? ""),
        enabled: Boolean(mr.enabled),
        filesCount: Number(mr.filesCount ?? mr.fileCount ?? 0) || 0,
        conflictsWith: asArr(mr.conflictsWith).map(String),
      };
    }),
  enableJsgmeMod: (name: string) => invoke<void>("enable_jsgme_mod", { name }),
  disableJsgmeMod: (name: string) => invoke<void>("disable_jsgme_mod", { name }),
  toggleContentEnabled: (kind: "car" | "track", id: string, enable: boolean) =>
    invoke<void>("toggle_content_enabled", { kind, id, enable }),
  runContentHealth: async (): Promise<ContentHealthIssue[]> => {
    const raw = await invoke<unknown>("run_content_health");
    if (Array.isArray(raw)) return raw as ContentHealthIssue[];
    const r = asRec(raw);
    const repairPaths = new Set(
      asArr(r.plannedRepairs).map((p) => String(asRec(p).path ?? ""))
    );
    return asArr(r.issues).map((iss, idx) => {
      const ir = asRec(iss);
      const target = String(ir.target ?? ir.path ?? "");
      const sevRaw = String(ir.severity ?? "info").toLowerCase();
      const severity: ContentHealthIssue["severity"] =
        sevRaw === "error" || sevRaw === "warning" ? sevRaw : "info";
      return {
        id: String(ir.id ?? `issue-${idx}`),
        severity,
        kind: String(ir.kind ?? severity),
        target,
        message: String(ir.message ?? ""),
        autoFixable: Boolean(ir.autoFixable ?? repairPaths.has(target)),
      };
    });
  },
  applyContentRepairs: () => invoke<string>("apply_content_repairs"),

  // Telemetry
  listSessions: async () =>
    asArr(await invoke<unknown>("list_sessions")).map(normalizeSessionSummary),
  getSessionAnalysis: async (id: string) =>
    normalizeSessionAnalysis(
      await invoke<unknown>("get_session_analysis", { id })
    ) as TelemetrySessionAnalysis,
  listReplays: async () =>
    asArr(await invoke<unknown>("list_replays")).map(normalizeReplay),
  launchReplayInAc: (filename: string) => invoke<string>("launch_replay_in_ac", { filename }),
  computeStrategy: async (req: StrategyRequest): Promise<StrategyPlan> => {
    const raw = asRec(
      await invoke<unknown>("compute_strategy", {
        req: {
          fuelLeft: req.tankCapacityL,
          fuelCapacity: req.tankCapacityL,
          fuelPerLap: req.fuelPerLapL,
          raceLaps: req.raceLaps,
          completedLaps: 0,
          tyreLifeLaps: req.tyreLifeLaps,
          minStintLaps: Math.max(1, Math.min(5, req.tyreLifeLaps)),
        },
      })
    );
    if (Array.isArray(raw.stintLengths)) {
      return raw as unknown as StrategyPlan;
    }
    const fuel = asRec(raw.fuel);
    const stints = asArr(raw.stints).map(asRec);
    const stintLengths = stints.map((s) => Number(s.laps ?? 0)).filter((n) => n > 0);
    const stopsRequired = Number(raw.stops ?? Math.max(0, stintLengths.length - 1));
    const maxStintFuel =
      stintLengths.length > 0
        ? Math.max(...stintLengths) * req.fuelPerLapL
        : req.raceLaps * req.fuelPerLapL;
    return {
      totalFuelL: Number(fuel.fuelNeeded ?? req.raceLaps * req.fuelPerLapL),
      stopsRequired,
      recommendedPitLaps: asArr(raw.stopLaps).map(Number),
      stintLengths,
      estimatedRaceTimeSec: req.raceLaps * req.lapTimeSec + stopsRequired * req.pitLossSec,
      fuelMarginL: Number((req.tankCapacityL - maxStintFuel).toFixed(1)),
    };
  },
  loadTimingTower: async (csvOrNone?: string | null): Promise<TimingRow[]> => {
    const raw = await invoke<unknown>("load_timing_tower", {
      csvOrNone: csvOrNone ?? null,
    });
    const list = Array.isArray(raw) ? raw : asArr(asRec(raw).entries);
    return list.map((row, idx) => {
      const rr = asRec(row);
      return {
        position: Number(rr.position ?? idx + 1),
        driver: String(rr.driver ?? ""),
        carId: String(rr.carId ?? ""),
        bestLapMs: Number(rr.bestLapMs ?? rr.bestMs ?? 0) || 0,
        lastLapMs: Number(rr.lastLapMs ?? rr.lastMs ?? 0) || 0,
        gapMs: Number(rr.gapMs ?? 0) || 0,
        laps: Number(rr.laps ?? 0) || 0,
        sector1Ms: typeof rr.sector1Ms === "number" ? rr.sector1Ms : undefined,
        sector2Ms: typeof rr.sector2Ms === "number" ? rr.sector2Ms : undefined,
        sector3Ms: typeof rr.sector3Ms === "number" ? rr.sector3Ms : undefined,
      };
    });
  },
  udpConnect: (host: string, port: number) => invoke<void>("udp_connect", { host, port }),
  udpDisconnect: () => invoke<void>("udp_disconnect"),
  getLiveTelemetry: async (): Promise<LiveTelemetryFrame> => {
    const raw = asRec(await invoke<unknown>("get_live_telemetry"));
    if ("speedKmh" in raw) {
      return raw as unknown as LiveTelemetryFrame;
    }
    const u = asRec(raw.carUpdate);
    const lap = asRec(raw.lastLap);
    return {
      connected: Boolean(raw.connected),
      speedKmh: Math.round(Number(u.speedKmh ?? 0) || 0),
      rpm: Math.round(Number(u.rpm ?? 0) || 0),
      maxRpm: 8500,
      gear: Number(u.gear ?? 0) || 0,
      throttle: Number(u.throttle ?? 0) || 0,
      brake: Number(u.brake ?? 0) || 0,
      clutch: 0,
      steerDeg: Number(u.steerAngle ?? 0) || 0,
      fuelL: 0,
      lapNumber: Number(lap.laps ?? 0) || 0,
      currentLapMs: Number(u.lapTimeMs ?? 0) || 0,
      lastLapMs: Number(lap.lapTimeMs ?? 0) || 0,
      bestLapMs: Number(lap.lapTimeMs ?? 0) || 0,
      tyreTempsC: [80, 80, 80, 80],
      tyrePressuresPsi: [26.5, 26.5, 26.5, 26.5],
      gLat: Number(u.gLateral ?? 0) || 0,
      gLong: Number(u.gLongitudinal ?? 0) || 0,
    };
  },
  toggleWebDashboard: (port: number, lan: boolean, start: boolean) =>
    invoke<string>("toggle_web_dashboard", { port, lan, start }),

  // Setup
  getSettings: async () => normalizeSettings(await invoke<unknown>("get_settings")),
  saveSettings: async (settings: AppSettings) => {
    const accentPreset =
      settings.accent === "TelemetryCyan"
        ? "telemetryCyan"
        : settings.accent === "AmberGold"
          ? "amberGold"
          : settings.accent === "Emerald"
            ? "emerald"
            : "motorsportRed";
    const saved = await invoke<unknown>("save_settings", {
      settings: {
        acRoot: settings.acRoot,
        patchLibrary: "",
        crashReports: true,
        accentPreset,
        quickSwitches: {
          vrMode: false,
          showFps: false,
          autoRecord: true,
          skipIntro: false,
        },
        activeRigProfile: null,
        adaptiveAiEnabled: true,
        sevenZipPath: null,
        modsDir: null,
        cspCatalogUrl: null,
      },
    });
    return { ...settings, ...normalizeSettings(saved) };
  },
  getVideoSettings: async () =>
    normalizeVideoSettings(await invoke<unknown>("get_video_settings")),
  saveVideoSettings: (cfg: VideoSettings) => {
    const base = cachedRawVideoSettings ?? {};
    const validAa = [1, 2, 4, 8];
    const aaSamples = cfg.msaa <= 1 ? 1 : validAa.includes(cfg.msaa) ? cfg.msaa : 4;
    const validAniso = [0, 2, 4, 8, 16];
    const anisotropic = validAniso.includes(cfg.anisotropic) ? cfg.anisotropic : 8;
    const validShadows = [-1, 32, 64, 128, 256, 512, 1024, 2048, 3072, 4096, 6144, 8192];
    const shadowMapSize =
      cfg.shadowMapSize <= 0
        ? -1
        : validShadows.includes(cfg.shadowMapSize)
          ? cfg.shadowMapSize
          : 2048;
    const payload = {
      width: cfg.width > 0 ? Math.round(cfg.width) : null,
      height: cfg.height > 0 ? Math.round(cfg.height) : null,
      refresh_hz: cfg.refreshHz > 0 ? Math.round(cfg.refreshHz) : null,
      display_index:
        typeof base.display_index === "number" ? base.display_index : null,
      fullscreen: Boolean(cfg.fullscreen),
      vsync: Boolean(cfg.vsync),
      fps_cap: cfg.fpsCap > 0 ? Math.round(cfg.fpsCap) : null,
      aa_samples: aaSamples,
      anisotropic,
      shadow_map_size: shadowMapSize,
      world_detail: Math.max(0, Math.min(5, Math.round(cfg.worldDetail))),
      post_processing: Boolean(cfg.postProcessing),
      camera_mode: toRustCameraMode(cfg.cameraMode ?? (base.camera_mode as VideoSettings["cameraMode"])),
      fxaa: Boolean(base.fxaa ?? false),
      motion_blur: Number(base.motion_blur ?? 0),
      smoke: Number(base.smoke ?? 3),
      render_smoke_in_mirror: Boolean(base.render_smoke_in_mirror ?? false),
      cubemap_size: Number(base.cubemap_size ?? 512),
      cubemap_faces_per_frame: Number(base.cubemap_faces_per_frame ?? 2),
      cubemap_farplane: Number(base.cubemap_farplane ?? 500),
      mirror_size: Number(base.mirror_size ?? 512),
      mirror_hq: Boolean(base.mirror_hq ?? false),
      glare: Number(base.glare ?? 2),
      dof: Number(base.dof ?? 2),
      rays_of_god: base.rays_of_god !== undefined ? Boolean(base.rays_of_god) : true,
      heat_shimmer: base.heat_shimmer !== undefined ? Boolean(base.heat_shimmer) : true,
      saturation: Number(base.saturation ?? 100),
    };
    cachedRawVideoSettings = payload;
    return invoke<void>("save_video_settings", { cfg: payload });
  },
  getVrTripleSettings: async () =>
    normalizeVrTripleSettings(await invoke<unknown>("get_vr_triple_settings")),
  saveVrTripleSettings: (cfg: VrTripleSettings) => {
    const base = cachedRawVrTripleSettings ?? {};
    const baseTriple = asRec(base.tripleScreen ?? base.triple_screen);
    const baseOculus = asRec(base.oculus);
    const baseOpenvr = asRec(base.openvr);
    const pixelDensity = Math.max(0.5, Math.min(2.5, Number(cfg.vrPixelDensity) || 1.0));
    const payload = {
      tripleScreen: {
        width_mm: Math.max(100, Math.min(3000, Number(cfg.screenWidthMm) || 597)),
        height_mm: Number(baseTriple.height_mm ?? 336),
        distance_mm: Math.max(100, Math.min(3000, Number(cfg.distanceMm) || 650)),
        angle_deg: Math.max(0, Math.min(89, Number(cfg.sideAngleDeg) || 45)),
        bezel_mm: Math.max(0, Number(cfg.bezelMm) || 0),
        margin_l_mm: Number(baseTriple.margin_l_mm ?? 0),
        margin_r_mm: Number(baseTriple.margin_r_mm ?? 0),
        rotation_deg: Number(baseTriple.rotation_deg ?? 0),
        sp_width_px: Number(baseTriple.sp_width_px ?? 1920),
      },
      oculus: {
        pixel_per_display: pixelDensity,
        mirror_texture:
          baseOculus.mirror_texture !== undefined ? Boolean(baseOculus.mirror_texture) : true,
        autoselect_audio:
          baseOculus.autoselect_audio !== undefined ? Boolean(baseOculus.autoselect_audio) : true,
        mirror_enable:
          baseOculus.mirror_enable !== undefined ? Boolean(baseOculus.mirror_enable) : true,
      },
      openvr: {
        pixel_per_display: pixelDensity,
        mirror_texture:
          baseOpenvr.mirror_texture !== undefined ? Boolean(baseOpenvr.mirror_texture) : true,
        autoselect_audio:
          baseOpenvr.autoselect_audio !== undefined ? Boolean(baseOpenvr.autoselect_audio) : true,
        mirror_enable:
          baseOpenvr.mirror_enable !== undefined ? Boolean(baseOpenvr.mirror_enable) : true,
      },
    };
    cachedRawVrTripleSettings = payload;
    return invoke<void>("save_vr_triple_settings", { cfg: payload });
  },
  getFfbConfig: async () =>
    normalizeFfbConfig(await invoke<unknown>("get_ffb_config")),
  saveFfbConfig: (cfg: FfbConfig) => {
    const base = cachedRawFfbConfig ?? {};
    const baseTweaks = asRec(base.tweaks);
    const basePp = asRec(base.postProcess ?? base.post_process);
    const basePerCar = asRec(base.perCarGain ?? base.per_car_gain);
    const payload = {
      tweaks: {
        basicEnabled:
          baseTweaks.basicEnabled !== undefined ? Boolean(baseTweaks.basicEnabled) : true,
        gyro2Enabled: Boolean(baseTweaks.gyro2Enabled ?? false),
        rangeCompression: Number(baseTweaks.rangeCompression ?? 1.0),
        rangeCompressionAssist: Boolean(baseTweaks.rangeCompressionAssist ?? false),
        cleanFfb: baseTweaks.cleanFfb !== undefined ? Boolean(baseTweaks.cleanFfb) : true,
        dirtVibration: Number(baseTweaks.dirtVibration ?? 0.0),
        postprocessingScriptEnabled: Boolean(baseTweaks.postprocessingScriptEnabled ?? false),
        softLockEnabled:
          baseTweaks.softLockEnabled !== undefined ? Boolean(baseTweaks.softLockEnabled) : true,
        softLockPadding: Number(baseTweaks.softLockPadding ?? 10.0),
        softLockShiftPadding: Number(baseTweaks.softLockShiftPadding ?? 0.8),
        softLockForceFactor: Number(baseTweaks.softLockForceFactor ?? 1.0),
        softLockSpeedFactor: Number(baseTweaks.softLockSpeedFactor ?? 1.0),
        softLockDamper: Number(baseTweaks.softLockDamper ?? 0.0),
        realFeelEnabled: Boolean(baseTweaks.realFeelEnabled ?? false),
        realFeelWheelMaxNm: Number(baseTweaks.realFeelWheelMaxNm ?? 35.0),
        realFeelEnableForAll: Boolean(baseTweaks.realFeelEnableForAll ?? false),
        realFeelBrakeEnabled: Boolean(baseTweaks.realFeelBrakeEnabled ?? false),
        brakeMaxKg: Number(baseTweaks.brakeMaxKg ?? 0.0),
        brakeMaxInactivePerc: Number(baseTweaks.brakeMaxInactivePerc ?? 100.0),
        brakeClampForces:
          baseTweaks.brakeClampForces !== undefined ? Boolean(baseTweaks.brakeClampForces) : true,
        adaptiveFfb: Number(baseTweaks.adaptiveFfb ?? 0.0),
        averageSkippedFfb:
          baseTweaks.averageSkippedFfb !== undefined ? Boolean(baseTweaks.averageSkippedFfb) : true,
        skipDampingFrames: Number(baseTweaks.skipDampingFrames ?? 3.0),
        strutFfbAdjustment:
          baseTweaks.strutFfbAdjustment !== undefined
            ? Boolean(baseTweaks.strutFfbAdjustment)
            : Boolean(cfg.enhancedUndersteer),
        fadeInAfterLoading:
          baseTweaks.fadeInAfterLoading !== undefined ? Boolean(baseTweaks.fadeInAfterLoading) : true,
      },
      postProcess: {
        enabled: Boolean(cfg.gammaEnabled),
        mode: String(basePp.mode ?? "Gamma") === "Lut" ? "Lut" : "Gamma",
        gamma: Math.max(0.1, Math.min(2.0, Number(cfg.gammaValue) || 1.0)),
        lut_curve: String(basePp.lut_curve ?? ""),
      },
      perCarGain: basePerCar,
    };
    cachedRawFfbConfig = payload;
    return invoke<void>("save_ffb_config", { cfg: payload });
  },
  listRigProfiles: () => invoke<RigProfile[]>("list_rig_profiles"),
  applyRigProfile: (name: string) => invoke<void>("apply_rig_profile", { name }),
  listCspPatches: async (): Promise<CspPatchInfo[]> => {
    const raw = await invoke<unknown>("list_csp_patches");
    if (Array.isArray(raw)) return raw as CspPatchInfo[];
    const r = asRec(raw);
    const installedVer = String(r.installedVersion ?? "");
    return asArr(r.patches).map((p) => {
      const pr = asRec(p);
      const ver = String(pr.version ?? pr.name ?? "");
      return {
        name: ver,
        version: ver,
        buildNumber: typeof pr.buildId === "number" ? pr.buildId : undefined,
        installed: Boolean(installedVer && ver === installedVer),
        recommended: Boolean(pr.known),
        notes: typeof pr.family === "string" ? pr.family : undefined,
      };
    });
  },
  installCspPatch: (name: string) => invoke<string>("install_csp_patch", { name }),
  uninstallCspPatch: () => invoke<string>("uninstall_csp_patch"),
  syncCspConfigs: () => invoke<string>("sync_csp_configs"),
  listCspConfigFiles: () => invoke<unknown[]>("list_csp_config_files"),
  readCspConfig: (file: string) => invoke<unknown>("read_csp_config", { file }),
  saveCspConfig: (file: string, edits: Record<string, string>) => {
    const list = Object.entries(edits).map(([k, v]) => {
      const dot = k.indexOf(".");
      return dot > 0
        ? { section: k.slice(0, dot), key: k.slice(dot + 1), value: v }
        : { section: "BASIC", key: k, value: v };
    });
    return invoke<void>("save_csp_config", { file, edits: list });
  },
  listConfigSnapshots: () => invoke<ConfigSnapshot[]>("list_config_snapshots"),
  createConfigSnapshot: (label: string) => invoke<ConfigSnapshot>("create_config_snapshot", { label }),
  restoreConfigSnapshot: (id: string) => invoke<void>("restore_config_snapshot", { id }),
};

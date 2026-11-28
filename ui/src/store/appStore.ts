import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api, isTauri } from "../lib/api";
import type {
  AccentPreset,
  AppSettings,
  ArmedSession,
  CarEntry,
  ContentOwnershipFilter,
  CspRequirementFilter,
  DrivingStatsSummary,
  OnlineServerDetail,
  OnlineServerSummary,
  Pillar,
  ServerCarSlot,
  ServerSortKey,
  ServerTab,
  SubTab,
  ToastItem,
  ToastVariant,
  TrackEntry,
  WatchlistEntry,
} from "../lib/types";

// ============================================================================
// Strictly 2-Level Navigation Hierarchy (Pillar -> SubTab)
// ============================================================================

export interface SubTabDefinition {
  id: SubTab;
  label: string;
  group?: string;
}

export interface PillarDefinition {
  id: Pillar;
  label: string;
  subTabs: SubTabDefinition[];
}

export const PILLAR_NAV: PillarDefinition[] = [
  {
    id: "paddock",
    label: "Paddock",
    subTabs: [],
  },
  {
    id: "drive",
    label: "Drive",
    subTabs: [
      { id: "quick-race", label: "Quick Race", group: "Racing" },
      { id: "events", label: "Events & Career", group: "Racing" },
      { id: "championships", label: "Championships", group: "Racing" },
    ],
  },
  {
    id: "garage",
    label: "Garage",
    subTabs: [
      { id: "showroom", label: "3D Showroom", group: "Showcase" },
      { id: "cars", label: "Cars", group: "Showcase" },
      { id: "tracks", label: "Tracks", group: "Showcase" },
      { id: "setups", label: "Setups", group: "Workshop" },
      { id: "skins", label: "Skins & Metadata", group: "Workshop" },
      { id: "painter", label: "Livery Painter", group: "Workshop" },
      { id: "plates", label: "License Plates", group: "Workshop" },
      { id: "models", label: "3D Models & AO", group: "Workshop" },
      { id: "installer", label: "Install Packages", group: "Content" },
      { id: "jsgme", label: "JSGME Mods", group: "Content" },
      { id: "health", label: "Health & Repairs", group: "Content" },
      { id: "tuning", label: "Car Tuning & ACD", group: "Content" },
      { id: "sfx", label: "SFX Swapper", group: "Content" },
    ],
  },
  {
    id: "online",
    label: "Online",
    subTabs: [],
  },
  {
    id: "server",
    label: "Server",
    subTabs: [],
  },
  {
    id: "telemetry",
    label: "Telemetry",
    subTabs: [
      { id: "sessions", label: "Sessions & Coach", group: "Analysis" },
      { id: "live", label: "Live Pit-Wall", group: "Analysis" },
      { id: "replay", label: "Replay Viewer", group: "Analysis" },
      { id: "trajectory", label: "Trajectory Studio", group: "Analysis" },
      { id: "strategy", label: "Race Strategy", group: "Race Ops" },
      { id: "timing", label: "Timing Tower", group: "Race Ops" },
      { id: "laptimes", label: "Lap Times", group: "Race Ops" },
    ],
  },
  {
    id: "setup",
    label: "Setup",
    subTabs: [
      { id: "game", label: "Game Settings", group: "Simulation" },
      { id: "csp", label: "CSP Config", group: "Shaders Patch" },
      { id: "csp-patches", label: "CSP Patches", group: "Shaders Patch" },
      { id: "csp-weather", label: "WeatherFX", group: "Shaders Patch" },
      { id: "app", label: "App Settings", group: "Application" },
    ],
  },
];

export function defaultSubTabFor(pillar: Pillar): SubTab {
  const def = PILLAR_NAV.find((p) => p.id === pillar);
  return def?.subTabs[0]?.id ?? "";
}

// ============================================================================
// Store Context Shape
// ============================================================================

const DEFAULT_ARMED: ArmedSession = {
  carId: "ks_porsche_911_gt3_r_2016",
  carName: "Porsche 911 GT3 R",
  skinId: "00_factory_white",
  trackId: "spa",
  trackName: "Circuit de Spa-Francorchamps",
  layoutId: "default",
  mode: "QuickRace",
  carPreviewPath: null,
  trackPreviewPath: null,
};

const DEFAULT_SETTINGS: AppSettings = {
  acRoot: "",
  accent: "MotorsportRed",
  steamLaunch: false,
  closeOnLaunch: false,
  checkUpdates: true,
  telemetryPort: 9996,
  webDashboardPort: 8787,
  cspAutoSync: true,
};

const DEFAULT_STATS: DrivingStatsSummary = {
  totalKm: 0,
  totalHours: 0,
  totalLaps: 0,
  favoriteCar: null,
  favoriteTrack: null,
  sessionsCount: 0,
};

export interface OnlineStreamProgress {
  loading: boolean;
  loaded: number;
  total: number;
}

export interface OnlineSortState {
  key: ServerSortKey;
  dir: "asc" | "desc";
}

function buildBrowserPreviewDetail(
  server: OnlineServerSummary,
  cars: CarEntry[],
  tracks: TrackEntry[]
): OnlineServerDetail {
  const ownedCarIds = new Set(cars.map((c) => c.id));
  const ownedTrackIds = new Set(tracks.map((t) => t.id));
  const trackOwned = ownedTrackIds.has(server.trackId);
  const synthSlots: ServerCarSlot[] = (
    server.cars.length > 0
      ? server.cars
      : ["ks_porsche_911_gt3_r_2016", "ks_ferrari_488_gt3"]
  ).flatMap((cid, idx) => [
    {
      carId: cid,
      skinId: "00_factory_white",
      driverName: idx === 0 ? "L. Rossi" : "K. Sato",
      connected: true,
      owned: ownedCarIds.has(cid),
    },
    {
      carId: cid,
      skinId: "01_obsidian_carbon",
      connected: false,
      owned: ownedCarIds.has(cid),
    },
    {
      carId: cid,
      skinId: "02_crimson_gt",
      connected: false,
      owned: ownedCarIds.has(cid),
    },
  ]);
  const missing: OnlineServerDetail["missingContent"] = [];
  if (!trackOwned && server.trackId && server.trackId !== "unknown") {
    missing.push({ kind: "track", id: server.trackId });
  }
  server.cars.forEach((cid) => {
    if (!ownedCarIds.has(cid)) missing.push({ kind: "car", id: cid });
  });

  return {
    summary: server,
    resolvedTrackId: server.trackId,
    trackOwned,
    minCspBuild: server.cspRequired ?? null,
    slots: synthSlots,
    missingContent: missing,
    features: [],
    description:
      "Clean racing lobby with live timing, dynamic rubbering, and Sol/Pure WeatherFX sync.",
    bookingMode: false,
  };
}

function mergeServersInPlace(
  existing: OnlineServerSummary[],
  incoming: Map<string, OnlineServerSummary>
): OnlineServerSummary[] {
  if (incoming.size === 0) return existing;
  const seen = new Set<string>();
  const merged = existing.map((srv) => {
    seen.add(srv.id);
    return incoming.get(srv.id) ?? srv;
  });
  for (const [id, srv] of incoming) {
    if (!seen.has(id)) {
      merged.push(srv);
    }
  }
  return merged;
}

function mergeServerSummaryFromDetail(
  existing: OnlineServerSummary,
  live: OnlineServerSummary
): OnlineServerSummary {
  const hasLiveName = Boolean(live.name && live.name !== live.host);
  return {
    ...existing,
    name: hasLiveName ? live.name : existing.name || live.name,
    port: live.port > 0 ? live.port : existing.port,
    httpPort:
      live.httpPort && live.httpPort > 0 ? live.httpPort : existing.httpPort,
    clients: live.clients,
    maxClients: live.maxClients > 0 ? live.maxClients : existing.maxClients,
    trackId:
      live.trackId && live.trackId !== "unknown"
        ? live.trackId
        : existing.trackId,
    trackLayout: live.trackLayout ?? existing.trackLayout,
    pingMs: live.pingMs ?? existing.pingMs,
    sessionType: live.sessionType ?? existing.sessionType,
    cspRequired: live.cspRequired ?? existing.cspRequired,
    cars: live.cars.length > 0 ? live.cars : existing.cars,
    country: live.country ?? existing.country,
    passworded: existing.passworded || live.passworded,
  };
}

export interface AppStoreValue {
  pillar: Pillar;
  subTab: SubTab;
  armedSession: ArmedSession;
  cars: CarEntry[];
  tracks: TrackEntry[];
  drivingStats: DrivingStatsSummary;
  settings: AppSettings;
  gameRunning: boolean;
  cspBuild: string | null;
  accent: AccentPreset;
  toasts: ToastItem[];
  commandPaletteOpen: boolean;
  quickSwitchesOpen: boolean;
  scanning: boolean;
  launching: boolean;

  // Online Lifted State
  onlineTab: ServerTab;
  onlineServers: OnlineServerSummary[];
  onlineCacheSavedUnix: number | null;
  onlineStreamProgress: OnlineStreamProgress | null;
  remoteSearchResults: OnlineServerSummary[];
  remoteSearchLoading: boolean;
  watchlist: WatchlistEntry[];
  recentServers: OnlineServerSummary[];
  lanServers: OnlineServerSummary[];
  onlineTabLoading: Record<ServerTab, boolean>;
  onlineSearchInput: string;
  onlineContentFilter: ContentOwnershipFilter;
  onlineOnlyJoinable: boolean;
  onlineHideEmpty: boolean;
  onlineHideFull: boolean;
  onlineHidePassworded: boolean;
  onlineRegionFilter: string;
  onlineCspFilter: CspRequirementFilter;
  onlineSessionFilter: string;
  onlineMinPlayers: number;
  probedOwnership: Record<
    string,
    { trackOwned: boolean; ownedCars: number; totalCars: number; resolvedTrackId?: string }
  >;
  onlineSort: OnlineSortState;
  selectedOnlineServer: OnlineServerSummary | null;
  selectedOnlineDetail: OnlineServerDetail | null;
  loadingOnlineDetail: boolean;
  watchedSlots: Record<string, OnlineServerSummary>;

  navigate: (pillar: Pillar, subTab?: SubTab) => void;
  setArmedSession: (patch: Partial<ArmedSession>) => void;
  armCar: (car: CarEntry, skinId?: string) => void;
  armTrack: (track: TrackEntry, layoutId?: string) => void;
  setAccent: (accent: AccentPreset) => void;
  updateSettings: (patch: Partial<AppSettings>) => Promise<void>;
  rescan: (silent?: boolean) => Promise<void>;
  launchArmed: () => Promise<void>;
  notify: (title: string, variant?: ToastVariant, description?: string) => void;
  dismissToast: (id: string) => void;
  setCommandPaletteOpen: (open: boolean) => void;
  setQuickSwitchesOpen: (open: boolean) => void;

  // Online Actions
  setOnlineTab: (tab: ServerTab) => void;
  setOnlineSearchInput: (q: string) => void;
  setOnlineContentFilter: (v: ContentOwnershipFilter) => void;
  setOnlineOnlyJoinable: (v: boolean) => void;
  setOnlineHideEmpty: (v: boolean) => void;
  setOnlineHideFull: (v: boolean) => void;
  setOnlineHidePassworded: (v: boolean) => void;
  setOnlineRegionFilter: (v: string) => void;
  setOnlineCspFilter: (v: CspRequirementFilter) => void;
  setOnlineSessionFilter: (v: string) => void;
  setOnlineMinPlayers: (v: number) => void;
  resetOnlineFilters: () => void;
  setOnlineSort: (sort: OnlineSortState) => void;
  ensureOnlineInitialized: () => void;
  refreshOnlineDirectory: () => Promise<void>;
  runRemoteDirectorySearch: (query: string) => Promise<void>;
  clearRemoteDirectorySearch: () => void;
  loadWatchlist: () => Promise<void>;
  toggleWatchlistServer: (server: OnlineServerSummary) => Promise<void>;
  loadRecentServers: () => Promise<void>;
  scanLanServers: () => Promise<void>;
  selectOnlineServer: (server: OnlineServerSummary | null) => void;
  toggleWatchSlot: (server: OnlineServerSummary) => void;
}

const AppStoreContext = createContext<AppStoreValue | null>(null);

export function AppProvider({ children }: { children: React.ReactNode }) {
  const [pillar, setPillar] = useState<Pillar>("paddock");
  const [subTab, setSubTab] = useState<SubTab>("");
  const [armedSession, setArmedSessionState] = useState<ArmedSession>(DEFAULT_ARMED);
  const [cars, setCars] = useState<CarEntry[]>([]);
  const [tracks, setTracks] = useState<TrackEntry[]>([]);
  const [drivingStats, setDrivingStats] = useState<DrivingStatsSummary>(DEFAULT_STATS);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [gameRunning, setGameRunning] = useState(false);
  const [cspBuild, setCspBuild] = useState<string | null>(null);
  const [accent, setAccentState] = useState<AccentPreset>("MotorsportRed");
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [quickSwitchesOpen, setQuickSwitchesOpen] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [launching, setLaunching] = useState(false);

  // Online Lifted State (persists across top-level pillar navigation)
  const [onlineTab, setOnlineTab] = useState<ServerTab>("online");
  const [onlineServers, setOnlineServers] = useState<OnlineServerSummary[]>([]);
  const [onlineCacheSavedUnix, setOnlineCacheSavedUnix] = useState<number | null>(null);
  const [onlineStreamProgress, setOnlineStreamProgress] =
    useState<OnlineStreamProgress | null>(null);
  const [remoteSearchResults, setRemoteSearchResults] = useState<OnlineServerSummary[]>([]);
  const [remoteSearchLoading, setRemoteSearchLoading] = useState(false);
  const [watchlist, setWatchlist] = useState<WatchlistEntry[]>([]);
  const [recentServers, setRecentServers] = useState<OnlineServerSummary[]>([]);
  const [lanServers, setLanServers] = useState<OnlineServerSummary[]>([]);
  const [onlineTabLoading, setOnlineTabLoading] = useState<Record<ServerTab, boolean>>({
    online: false,
    watchlist: false,
    recent: false,
    lan: false,
  });
  const [onlineSearchInput, setOnlineSearchInput] = useState("");
  const [onlineContentFilter, setOnlineContentFilterState] =
    useState<ContentOwnershipFilter>("all");
  const [onlineHideEmpty, setOnlineHideEmpty] = useState(false);
  const [onlineHideFull, setOnlineHideFull] = useState(false);
  const [onlineHidePassworded, setOnlineHidePassworded] = useState(false);
  const [onlineRegionFilter, setOnlineRegionFilter] = useState("all");
  const [onlineCspFilter, setOnlineCspFilter] = useState<CspRequirementFilter>("all");
  const [onlineSessionFilter, setOnlineSessionFilter] = useState("all");
  const [onlineMinPlayers, setOnlineMinPlayers] = useState(0);
  const [probedOwnership, setProbedOwnership] = useState<
    Record<string, { trackOwned: boolean; ownedCars: number; totalCars: number; resolvedTrackId?: string }>
  >({});
  const onlineOnlyJoinable =
    onlineContentFilter === "ready" || onlineContentFilter === "full";
  const setOnlineOnlyJoinable = useCallback((v: boolean) => {
    setOnlineContentFilterState(v ? "ready" : "all");
  }, []);
  const setOnlineContentFilter = useCallback((v: ContentOwnershipFilter) => {
    setOnlineContentFilterState(v);
  }, []);
  const resetOnlineFilters = useCallback(() => {
    setOnlineSearchInput("");
    setOnlineContentFilterState("all");
    setOnlineHideEmpty(false);
    setOnlineHideFull(false);
    setOnlineHidePassworded(false);
    setOnlineRegionFilter("all");
    setOnlineCspFilter("all");
    setOnlineSessionFilter("all");
    setOnlineMinPlayers(0);
  }, []);
  const [onlineSort, setOnlineSort] = useState<OnlineSortState>({
    key: "clients",
    dir: "desc",
  });
  const [selectedOnlineServer, setSelectedOnlineServer] =
    useState<OnlineServerSummary | null>(null);
  const [selectedOnlineDetail, setSelectedOnlineDetail] =
    useState<OnlineServerDetail | null>(null);
  const [loadingOnlineDetail, setLoadingOnlineDetail] = useState(false);
  const [watchedSlots, setWatchedSlots] = useState<Record<string, OnlineServerSummary>>({});

  // Monotonic request guards & refs
  const carsRef = useRef(cars);
  carsRef.current = cars;
  const tracksRef = useRef(tracks);
  tracksRef.current = tracks;
  const onlineServersRef = useRef(onlineServers);
  onlineServersRef.current = onlineServers;
  const selectedOnlineServerRef = useRef(selectedOnlineServer);
  selectedOnlineServerRef.current = selectedOnlineServer;
  const watchedSlotsRef = useRef(watchedSlots);
  watchedSlotsRef.current = watchedSlots;

  const onlineInitRef = useRef(false);
  const globalDirSeqRef = useRef(0);
  const activeDirReqIdRef = useRef(0);
  const activeSearchReqIdRef = useRef(0);
  const dirAccumRef = useRef<Map<string, OnlineServerSummary>>(new Map());
  const searchAccumRef = useRef<Map<string, OnlineServerSummary>>(new Map());
  const watchlistReqIdRef = useRef(0);
  const recentReqIdRef = useRef(0);
  const lanReqIdRef = useRef(0);
  const detailReqIdRef = useRef(0);

  // Sync dynamic accent CSS variables on :root
  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);

  // Global Cmd+K / Ctrl+K shortcut for Command Palette
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setCommandPaletteOpen((prev) => !prev);
      } else if (e.key === "Escape") {
        setCommandPaletteOpen(false);
        setQuickSwitchesOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const notify = useCallback(
    (title: string, variant: ToastVariant = "info", description?: string) => {
      const id = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      const item: ToastItem = { id, title, description, variant, createdAt: Date.now() };
      setToasts((prev) => [...prev.slice(-4), item]);
      window.setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, 4200);
    },
    []
  );

  const navigate = useCallback((nextPillar: Pillar, nextSubTab?: SubTab) => {
    setPillar(nextPillar);
    const pillarDef = PILLAR_NAV.find((p) => p.id === nextPillar);
    if (!pillarDef || pillarDef.subTabs.length === 0) {
      setSubTab("");
      return;
    }
    if (nextSubTab && pillarDef.subTabs.some((t) => t.id === nextSubTab)) {
      setSubTab(nextSubTab);
    } else {
      setSubTab(pillarDef.subTabs[0].id);
    }
  }, []);

  const setArmedSession = useCallback((patch: Partial<ArmedSession>) => {
    setArmedSessionState((prev) => ({ ...prev, ...patch }));
  }, []);

  const armCar = useCallback((car: CarEntry, skinId?: string) => {
    setArmedSessionState((prev) => ({
      ...prev,
      carId: car.id,
      carName: car.name,
      skinId: skinId ?? prev.skinId,
      carPreviewPath: car.previewPath ?? null,
    }));
  }, []);

  const armTrack = useCallback((track: TrackEntry, layoutId?: string) => {
    const resolvedLayout = layoutId ?? track.layouts[0]?.id ?? "default";
    setArmedSessionState((prev) => ({
      ...prev,
      trackId: track.id,
      trackName: track.name,
      layoutId: resolvedLayout,
      trackPreviewPath: track.previewPath ?? null,
    }));
  }, []);

  const setAccent = useCallback(
    (next: AccentPreset) => {
      setAccentState(next);
      setSettings((prev) => {
        const updated = { ...prev, accent: next };
        void api.saveSettings(updated).catch(() => {});
        return updated;
      });
    },
    []
  );

  const updateSettings = useCallback(
    async (patch: Partial<AppSettings>) => {
      const merged = { ...settings, ...patch };
      setSettings(merged);
      if (patch.accent) {
        setAccentState(patch.accent);
      }
      try {
        await api.saveSettings(merged);
        notify("Settings saved", "success");
      } catch (err) {
        notify("Failed to save settings", "error", String(err));
      }
    },
    [settings, notify]
  );

  const rescan = useCallback(
    async (silent = false) => {
      setScanning(true);
      try {
        const [scan, loadedSettings] = await Promise.all([
          api.scanContent(),
          api.getSettings().catch(() => null),
        ]);
        if (scan) {
          setCars(scan.cars ?? []);
          setTracks(scan.tracks ?? []);
          setGameRunning(Boolean(scan.gameRunning));
          setCspBuild(scan.cspBuild ?? null);
          if (scan.drivingStats) setDrivingStats(scan.drivingStats);

          // Hydrate armed session with real scanned car/track if current ID is missing
          setArmedSessionState((prev) => {
            const matchedCar =
              scan.cars.find((c) => c.id === prev.carId) ?? scan.cars[0];
            const matchedTrack =
              scan.tracks.find((t) => t.id === prev.trackId) ?? scan.tracks[0];
            return {
              ...prev,
              carId: matchedCar?.id ?? prev.carId,
              carName: matchedCar?.name ?? prev.carName,
              carPreviewPath: matchedCar?.previewPath ?? prev.carPreviewPath,
              trackId: matchedTrack?.id ?? prev.trackId,
              trackName: matchedTrack?.name ?? prev.trackName,
              layoutId: matchedTrack?.layouts[0]?.id ?? prev.layoutId,
              trackPreviewPath: matchedTrack?.previewPath ?? prev.trackPreviewPath,
            };
          });
        }
        if (loadedSettings) {
          setSettings(loadedSettings);
          if (loadedSettings.accent) {
            setAccentState(loadedSettings.accent);
          }
        }
        if (!silent) {
          notify(
            "Content rescan complete",
            "success",
            `${scan?.cars?.length ?? 0} cars · ${scan?.tracks?.length ?? 0} tracks indexed`
          );
        }
      } catch (err) {
        if (!silent) {
          notify("Content scan failed", "error", String(err));
        }
      } finally {
        setScanning(false);
      }
    },
    [notify]
  );

  const launchArmed = useCallback(async () => {
    if (gameRunning) {
      notify("Assetto Corsa is already running", "warning");
      return;
    }
    setLaunching(true);
    try {
      await api.launchQuickRace({
        carId: armedSession.carId,
        skinId: armedSession.skinId,
        trackId: armedSession.trackId,
        layoutId: armedSession.layoutId,
        mode: armedSession.mode,
        laps: 5,
        durationMinutes: 15,
        aiOpponents: [],
        timeOfDayMinutes: 660,
        weatherId: "3_clear",
        ambientTempC: 24,
        roadTempC: 32,
        trackGripPreset: "Optimum",
        ghostEnabled: false,
        ghostAdvantageSec: 0,
        penaltiesEnabled: true,
        damageMultiplier: 50,
        fuelRate: 100,
        tyreWearRate: 100,
      });
      notify(
        "Launching Assetto Corsa",
        "success",
        `${armedSession.carName} @ ${armedSession.trackName}`
      );
    } catch (err) {
      notify("Launch failed", "error", String(err));
    } finally {
      setLaunching(false);
    }
  }, [armedSession, gameRunning, notify]);

  // Initial scan on mount
  useEffect(() => {
    void rescan(true);
  }, [rescan]);

  // Listen for incremental directory streaming events from Rust
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void api.onDirectoryProgress((evt) => {
      if (disposed) return;
      if (evt.requestId === activeDirReqIdRef.current) {
        for (const srv of evt.servers) {
          dirAccumRef.current.set(srv.id, srv);
        }
        setOnlineStreamProgress({
          loading: !evt.done,
          loaded: evt.loaded,
          total: evt.total,
        });
        if (evt.done) {
          if (dirAccumRef.current.size > 0) {
            setOnlineServers(Array.from(dirAccumRef.current.values()));
            setOnlineCacheSavedUnix(Math.floor(Date.now() / 1000));
          }
          setOnlineTabLoading((prev) => ({ ...prev, online: false }));
          setOnlineStreamProgress(null);
        } else if (evt.servers.length > 0) {
          setOnlineTabLoading((prev) => ({ ...prev, online: false }));
          setOnlineServers((prev) =>
            prev.length > 0
              ? mergeServersInPlace(prev, dirAccumRef.current)
              : Array.from(dirAccumRef.current.values())
          );
        }
      } else if (evt.requestId === activeSearchReqIdRef.current) {
        for (const srv of evt.servers) {
          searchAccumRef.current.set(srv.id, srv);
        }
        if (evt.servers.length > 0) {
          setRemoteSearchResults(Array.from(searchAccumRef.current.values()));
        }
        if (evt.done) {
          setRemoteSearchLoading(false);
        }
      }
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const selectOnlineServer = useCallback((server: OnlineServerSummary | null) => {
    const prevId = selectedOnlineServerRef.current?.id;
    selectedOnlineServerRef.current = server;
    setSelectedOnlineServer(server);
    const reqId = ++detailReqIdRef.current;
    if (!server) {
      setSelectedOnlineDetail(null);
      setLoadingOnlineDetail(false);
      return;
    }
    if (prevId !== server.id) {
      setSelectedOnlineDetail(null);
    }
    setLoadingOnlineDetail(true);
    void api
      .fetchServerDetail(server.host, server.httpPort ?? server.port)
      .then((res) => {
        if (detailReqIdRef.current !== reqId) return;
        if (res) {
          const uniqueModels = new Map<string, boolean>();
          for (const slot of res.slots ?? []) {
            if (!slot.carId) continue;
            uniqueModels.set(
              slot.carId,
              Boolean(uniqueModels.get(slot.carId) || slot.owned)
            );
          }
          let ownedCars = 0;
          for (const owned of uniqueModels.values()) {
            if (owned) ownedCars += 1;
          }
          setProbedOwnership((prev) => ({
            ...prev,
            [server.id]: {
              trackOwned: res.trackOwned,
              ownedCars,
              totalCars: uniqueModels.size,
              resolvedTrackId: res.resolvedTrackId,
            },
          }));
          const currentSelected = selectedOnlineServerRef.current;
          if (currentSelected && currentSelected.id === server.id) {
            const reconciled = mergeServerSummaryFromDetail(
              currentSelected,
              res.summary
            );
            selectedOnlineServerRef.current = reconciled;
            setSelectedOnlineServer(reconciled);
            setSelectedOnlineDetail({ ...res, summary: reconciled });
          } else {
            setSelectedOnlineDetail(res);
          }
        } else if (!isTauri()) {
          setSelectedOnlineDetail(
            buildBrowserPreviewDetail(server, carsRef.current, tracksRef.current)
          );
        } else {
          setSelectedOnlineDetail(null);
        }
      })
      .catch(() => {
        if (detailReqIdRef.current !== reqId) return;
        if (!isTauri()) {
          setSelectedOnlineDetail(
            buildBrowserPreviewDetail(server, carsRef.current, tracksRef.current)
          );
        } else {
          setSelectedOnlineDetail(null);
        }
      })
      .finally(() => {
        if (detailReqIdRef.current === reqId) {
          setLoadingOnlineDetail(false);
        }
      });
  }, []);

  const refreshOnlineDirectory = useCallback(async () => {
    const reqId = ++globalDirSeqRef.current;
    activeDirReqIdRef.current = reqId;
    dirAccumRef.current = new Map();

    const hadCached = onlineServersRef.current.length > 0;
    if (!hadCached) {
      setOnlineTabLoading((prev) => ({ ...prev, online: true }));
    }
    setOnlineStreamProgress({ loading: true, loaded: 0, total: 0 });

    try {
      const list = await api.fetchDirectoryServers("", true, 60, reqId);
      if (activeDirReqIdRef.current !== reqId) return;
      if (list && list.length > 0) {
        setOnlineServers(list);
        setOnlineCacheSavedUnix(Math.floor(Date.now() / 1000));
        if (!selectedOnlineServerRef.current) {
          selectOnlineServer(list[0]);
        }
      } else if (!hadCached) {
        setOnlineServers([]);
      }
    } catch (err) {
      if (activeDirReqIdRef.current === reqId) {
        notify("Failed to refresh online directory", "error", String(err));
      }
    } finally {
      if (activeDirReqIdRef.current === reqId) {
        setOnlineTabLoading((prev) => ({ ...prev, online: false }));
        setOnlineStreamProgress(null);
      }
    }
  }, [notify, selectOnlineServer]);

  const clearRemoteDirectorySearch = useCallback(() => {
    const hadActiveSearch = activeSearchReqIdRef.current !== 0;
    activeSearchReqIdRef.current = 0;
    searchAccumRef.current = new Map();
    setRemoteSearchResults([]);
    setRemoteSearchLoading(false);
    if (hadActiveSearch) {
      void api.cancelDirectorySearch().catch(() => {});
    }
  }, []);

  const runRemoteDirectorySearch = useCallback(
    async (query: string) => {
      const trimmed = query.trim();
      if (trimmed.length < 2) {
        clearRemoteDirectorySearch();
        return;
      }
      const reqId = ++globalDirSeqRef.current;
      activeSearchReqIdRef.current = reqId;
      searchAccumRef.current = new Map();
      setRemoteSearchLoading(true);
      try {
        const list = await api.fetchDirectoryServers(trimmed, false, 10, reqId);
        if (activeSearchReqIdRef.current !== reqId) return;
        setRemoteSearchResults(list ?? []);
      } catch {
        if (activeSearchReqIdRef.current === reqId) {
          setRemoteSearchResults([]);
        }
      } finally {
        if (activeSearchReqIdRef.current === reqId) {
          setRemoteSearchLoading(false);
        }
      }
    },
    [clearRemoteDirectorySearch]
  );

  const loadWatchlist = useCallback(async () => {
    const reqId = ++watchlistReqIdRef.current;
    setOnlineTabLoading((prev) => ({ ...prev, watchlist: true }));
    try {
      const wl = (await api.listWatchlist()) ?? [];
      if (watchlistReqIdRef.current !== reqId) return;
      setWatchlist(wl);
    } catch {
      // Keep existing watchlist if read fails
    } finally {
      if (watchlistReqIdRef.current === reqId) {
        setOnlineTabLoading((prev) => ({ ...prev, watchlist: false }));
      }
    }
  }, []);

  const toggleWatchlistServer = useCallback(
    async (s: OnlineServerSummary) => {
      const targetPort = s.httpPort ?? s.port;
      const exists = watchlist.some(
        (w) =>
          w.host === s.host && (w.port === targetPort || w.port === s.port)
      );
      const next = exists
        ? watchlist.filter(
            (w) =>
              !(w.host === s.host && (w.port === targetPort || w.port === s.port))
          )
        : [
            { name: s.name, host: s.host, port: targetPort, favorite: true },
            ...watchlist,
          ];
      setWatchlist(next);
      try {
        await api.saveWatchlist(next);
        notify(
          exists ? "Removed from watchlist" : "Added to watchlist",
          "info",
          s.name
        );
      } catch (err) {
        notify("Failed to update watchlist", "error", String(err));
      }
    },
    [watchlist, notify]
  );

  const loadRecentServers = useCallback(async () => {
    const reqId = ++recentReqIdRef.current;
    setOnlineTabLoading((prev) => ({ ...prev, recent: true }));
    try {
      const list = (await api.listRecentServers()) ?? [];
      if (recentReqIdRef.current !== reqId) return;
      setRecentServers(list);
    } catch (err) {
      if (recentReqIdRef.current === reqId) {
        notify("Failed to load recent servers", "error", String(err));
      }
    } finally {
      if (recentReqIdRef.current === reqId) {
        setOnlineTabLoading((prev) => ({ ...prev, recent: false }));
      }
    }
  }, [notify]);

  const scanLanServers = useCallback(async () => {
    const reqId = ++lanReqIdRef.current;
    setOnlineTabLoading((prev) => ({ ...prev, lan: true }));
    try {
      const list = (await api.scanLanServers()) ?? [];
      if (lanReqIdRef.current !== reqId) return;
      setLanServers(list);
    } catch (err) {
      if (lanReqIdRef.current === reqId) {
        notify("LAN scan failed", "error", String(err));
      }
    } finally {
      if (lanReqIdRef.current === reqId) {
        setOnlineTabLoading((prev) => ({ ...prev, lan: false }));
      }
    }
  }, [notify]);

  const ensureOnlineInitialized = useCallback(() => {
    if (onlineInitRef.current) return;
    onlineInitRef.current = true;

    void loadWatchlist();
    void loadRecentServers();
    void api
      .getCachedDirectoryServers()
      .then((cached) => {
        if (cached.servers.length > 0 && onlineServersRef.current.length === 0) {
          setOnlineServers(cached.servers);
          setOnlineCacheSavedUnix(cached.savedUnix);
          if (!selectedOnlineServerRef.current) {
            selectOnlineServer(cached.servers[0]);
          }
        }
      })
      .catch(() => {})
      .finally(() => {
        void refreshOnlineDirectory();
      });
  }, [loadWatchlist, loadRecentServers, refreshOnlineDirectory, selectOnlineServer]);

  const toggleWatchSlot = useCallback(
    (server: OnlineServerSummary) => {
      const key = server.id;
      const wasWatched = Boolean(watchedSlotsRef.current[key]);
      setWatchedSlots((prev) => {
        const next = { ...prev };
        if (wasWatched) {
          delete next[key];
        } else {
          next[key] = server;
        }
        return next;
      });
      if (wasWatched) {
        notify("Slot watch disabled", "info", server.name);
      } else {
        notify("Watching for open slot", "info", `${server.name} (polling every 10s)`);
      }
    },
    [notify]
  );

  // Background Slot-Watch polling loop
  useEffect(() => {
    const watchedEntries = Object.entries(watchedSlots);
    if (watchedEntries.length === 0) return;

    let cancelled = false;

    const pollOnce = async () => {
      for (const [key, srv] of watchedEntries) {
        if (cancelled) return;
        try {
          const d = await api.fetchServerDetail(srv.host, srv.httpPort ?? srv.port);
          if (cancelled || !watchedSlotsRef.current[key] || !d) continue;
          const clients = d.summary.clients;
          const maxClients =
            d.summary.maxClients > 0 ? d.summary.maxClients : srv.maxClients;
          setOnlineServers((prev) =>
            prev.map((item) =>
              item.id === key ? mergeServerSummaryFromDetail(item, d.summary) : item
            )
          );
          if (selectedOnlineServerRef.current?.id === key) {
            const reconciled = mergeServerSummaryFromDetail(
              selectedOnlineServerRef.current,
              d.summary
            );
            selectedOnlineServerRef.current = reconciled;
            setSelectedOnlineServer(reconciled);
            setSelectedOnlineDetail({ ...d, summary: reconciled });
          }
          if (maxClients > 0 && clients < maxClients) {
            if (cancelled || !watchedSlotsRef.current[key]) continue;
            const free = maxClients - clients;
            notify(
              "Slot Available!",
              "success",
              `${d.summary.name || srv.name} has ${free} open slot${free === 1 ? "" : "s"} (${clients}/${maxClients})`
            );
            setWatchedSlots((prev) => {
              if (!prev[key]) return prev;
              const next = { ...prev };
              delete next[key];
              return next;
            });
          }
        } catch {
          // Ignore transient poll errors; keep watching on next tick
        }
      }
    };

    const timer = window.setInterval(() => {
      void pollOnce();
    }, 10_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [watchedSlots, notify]);

  const value = useMemo<AppStoreValue>(
    () => ({
      pillar,
      subTab,
      armedSession,
      cars,
      tracks,
      drivingStats,
      settings,
      gameRunning,
      cspBuild,
      accent,
      toasts,
      commandPaletteOpen,
      quickSwitchesOpen,
      scanning,
      launching,
      onlineTab,
      onlineServers,
      onlineCacheSavedUnix,
      onlineStreamProgress,
      remoteSearchResults,
      remoteSearchLoading,
      watchlist,
      recentServers,
      lanServers,
      onlineTabLoading,
      onlineSearchInput,
      onlineContentFilter,
      onlineOnlyJoinable,
      onlineHideEmpty,
      onlineHideFull,
      onlineHidePassworded,
      onlineRegionFilter,
      onlineCspFilter,
      onlineSessionFilter,
      onlineMinPlayers,
      probedOwnership,
      onlineSort,
      selectedOnlineServer,
      selectedOnlineDetail,
      loadingOnlineDetail,
      watchedSlots,
      navigate,
      setArmedSession,
      armCar,
      armTrack,
      setAccent,
      updateSettings,
      rescan,
      launchArmed,
      notify,
      dismissToast,
      setCommandPaletteOpen,
      setQuickSwitchesOpen,
      setOnlineTab,
      setOnlineSearchInput,
      setOnlineContentFilter,
      setOnlineOnlyJoinable,
      setOnlineHideEmpty,
      setOnlineHideFull,
      setOnlineHidePassworded,
      setOnlineRegionFilter,
      setOnlineCspFilter,
      setOnlineSessionFilter,
      setOnlineMinPlayers,
      resetOnlineFilters,
      setOnlineSort,
      ensureOnlineInitialized,
      refreshOnlineDirectory,
      runRemoteDirectorySearch,
      clearRemoteDirectorySearch,
      loadWatchlist,
      toggleWatchlistServer,
      loadRecentServers,
      scanLanServers,
      selectOnlineServer,
      toggleWatchSlot,
    }),
    [
      pillar,
      subTab,
      armedSession,
      cars,
      tracks,
      drivingStats,
      settings,
      gameRunning,
      cspBuild,
      accent,
      toasts,
      commandPaletteOpen,
      quickSwitchesOpen,
      scanning,
      launching,
      onlineTab,
      onlineServers,
      onlineCacheSavedUnix,
      onlineStreamProgress,
      remoteSearchResults,
      remoteSearchLoading,
      watchlist,
      recentServers,
      lanServers,
      onlineTabLoading,
      onlineSearchInput,
      onlineContentFilter,
      onlineOnlyJoinable,
      onlineHideEmpty,
      onlineHideFull,
      onlineHidePassworded,
      onlineRegionFilter,
      onlineCspFilter,
      onlineSessionFilter,
      onlineMinPlayers,
      probedOwnership,
      onlineSort,
      selectedOnlineServer,
      selectedOnlineDetail,
      loadingOnlineDetail,
      watchedSlots,
      navigate,
      setArmedSession,
      armCar,
      armTrack,
      setAccent,
      updateSettings,
      rescan,
      launchArmed,
      notify,
      dismissToast,
      setOnlineContentFilter,
      setOnlineOnlyJoinable,
      resetOnlineFilters,
      ensureOnlineInitialized,
      refreshOnlineDirectory,
      runRemoteDirectorySearch,
      clearRemoteDirectorySearch,
      loadWatchlist,
      toggleWatchlistServer,
      loadRecentServers,
      scanLanServers,
      selectOnlineServer,
      toggleWatchSlot,
    ]
  );

  return React.createElement(AppStoreContext.Provider, { value }, children);
}

export function useAppStore(): AppStoreValue {
  const ctx = useContext(AppStoreContext);
  if (!ctx) {
    throw new Error("useAppStore must be used within <AppProvider>");
  }
  return ctx;
}

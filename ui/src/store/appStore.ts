import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { api } from "../lib/api";
import type {
  AccentPreset,
  AppSettings,
  ArmedSession,
  CarEntry,
  DrivingStatsSummary,
  Pillar,
  SubTab,
  ToastItem,
  ToastVariant,
  TrackEntry,
} from "../lib/types";

// ============================================================================
// Strictly 2-Level Navigation Hierarchy (Pillar -> SubTab)
// ============================================================================

export interface SubTabDefinition {
  id: SubTab;
  label: string;
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
      { id: "quick-race", label: "Quick Race" },
      { id: "events", label: "Events" },
      { id: "championships", label: "Championships" },
    ],
  },
  {
    id: "garage",
    label: "Garage",
    subTabs: [
      { id: "showroom", label: "Showroom" },
      { id: "cars", label: "Cars" },
      { id: "tracks", label: "Tracks" },
      { id: "setups", label: "Setups" },
      { id: "studio", label: "Studio" },
      { id: "library", label: "Library" },
    ],
  },
  {
    id: "online",
    label: "Online",
    subTabs: [
      { id: "servers", label: "Server Browser" },
      { id: "dedicated", label: "Dedicated Server" },
    ],
  },
  {
    id: "telemetry",
    label: "Telemetry",
    subTabs: [
      { id: "sessions", label: "Sessions" },
      { id: "live", label: "Live Pit-Wall" },
      { id: "replay", label: "Replay" },
      { id: "trajectory", label: "Trajectory" },
      { id: "strategy", label: "Strategy & Timing" },
    ],
  },
  {
    id: "setup",
    label: "Setup",
    subTabs: [
      { id: "game", label: "Game Settings" },
      { id: "csp", label: "CSP & Lights" },
      { id: "app", label: "App Settings" },
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

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Bookmark,
  Box,
  Car,
  Cloud,
  CloudRain,
  CloudSun,
  Compass,
  Flag,
  Flame,
  Gauge,
  Ghost,
  Image as ImageIcon,
  MapPin,
  Moon,
  Play,
  Plus,
  Save,
  Search,
  Sparkles,
  Sun,
  Trash2,
  Users,
  Wind,
  X,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import { ACCENT_PRESETS } from "../lib/types";
import type {
  AdaptiveAiSuggestion,
  AiOpponent,
  CarEntry,
  QuickRacePreset,
  QuickRaceRequest,
  SessionMode,
  SkinEntry,
  Track3DSceneData,
  TrackEntry,
  WeatherController,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import { Viewport3D } from "../components/Viewport3D";
import {
  Badge,
  Button,
  Card,
  SegmentedControl,
  Slider,
  Toggle,
} from "../components/Primitives";

// ============================================================================
// Constants & Helpers
// ============================================================================

const MODE_OPTIONS: Array<{ value: SessionMode; label: string }> = [
  { value: "Weekend", label: "Weekend" },
  { value: "TrackDay", label: "Track Day" },
  { value: "Hotlap", label: "Hotlap" },
  { value: "Drift", label: "Drift" },
  { value: "Drag", label: "Drag" },
  { value: "TimeAttack", label: "Time Attack" },
];

const WEATHER_PRESETS = [
  { id: "3_clear", label: "Clear", icon: Sun },
  { id: "4_mid_clear", label: "Few Clouds", icon: CloudSun },
  { id: "5_light_clouds", label: "Scattered", icon: Cloud },
  { id: "6_mid_clouds", label: "Overcast", icon: Cloud },
  { id: "7_heavy_clouds", label: "Light Rain", icon: CloudRain },
  { id: "2_light_fog", label: "Fog", icon: Wind },
];

const GRIP_PRESETS = ["Dusty", "Slow", "Green", "Fast", "Optimum"];

const TIME_QUICK_PRESETS = [
  { label: "Dawn", minutes: 390 },
  { label: "Morning", minutes: 600 },
  { label: "Noon", minutes: 780 },
  { label: "Golden", minutes: 1095 },
  { label: "Night", minutes: 1350 },
];

function formatMinutes(totalMinutes: number): string {
  const clamped = ((Math.round(totalMinutes) % 1440) + 1440) % 1440;
  const h = Math.floor(clamped / 60);
  const m = clamped % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

function getSkyPhase(minutes: number): {
  label: string;
  gradient: string;
  orbColor: string;
  isNight: boolean;
} {
  if (minutes < 330 || minutes >= 1260) {
    return {
      label: "Night",
      gradient: "linear-gradient(180deg, #060914 0%, #0D1528 65%, #121B30 100%)",
      orbColor: "#C7D8FF",
      isNight: true,
    };
  }
  if (minutes < 450) {
    return {
      label: "Dawn",
      gradient: "linear-gradient(180deg, #161B38 0%, #52345B 55%, #C96A4B 100%)",
      orbColor: "#FFB074",
      isNight: false,
    };
  }
  if (minutes < 1020) {
    return {
      label: "Daylight",
      gradient: "linear-gradient(180deg, #0E2A4A 0%, #17436E 55%, #235882 100%)",
      orbColor: "#FFE48A",
      isNight: false,
    };
  }
  return {
    label: "Golden Hour",
    gradient: "linear-gradient(180deg, #1A1D3A 0%, #6D3549 52%, #E27B3B 100%)",
    orbColor: "#FF9E44",
    isNight: false,
  };
}

// ============================================================================
// Interactive SVG Solar Arc Time-of-Day Dial
// ============================================================================

function SolarArcDial({
  minutes,
  onChange,
}: {
  minutes: number;
  onChange: (m: number) => void;
}) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const draggingRef = useRef(false);

  const cx = 180;
  const cy = 128;
  const r = 104;

  // Map 0..1439 minutes across left (180 deg) -> top (90 deg) -> right (0 deg)
  const frac = Math.min(1, Math.max(0, minutes / 1439));
  const angleRad = Math.PI * (1 - frac);
  const orbX = cx + r * Math.cos(angleRad);
  const orbY = cy - r * Math.sin(angleRad);

  const sky = getSkyPhase(minutes);

  const updateFromClientPoint = (clientX: number, clientY: number) => {
    const svg = svgRef.current;
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    const x = ((clientX - rect.left) / rect.width) * 360;
    const y = ((clientY - rect.top) / rect.height) * 150;
    const dx = x - cx;
    const dy = cy - y;
    let theta = Math.atan2(Math.max(0, dy), dx); // 0..PI
    if (theta < 0) theta = 0;
    if (theta > Math.PI) theta = Math.PI;
    const nextFrac = 1 - theta / Math.PI;
    onChange(Math.round(nextFrac * 1439));
  };

  return (
    <div className="space-y-3">
      <div
        className="relative rounded-xl overflow-hidden border border-white/[0.08] select-none"
        style={{ background: sky.gradient }}
      >
        <svg
          ref={svgRef}
          viewBox="0 0 360 150"
          className="w-full h-[146px] cursor-pointer touch-none"
          onPointerDown={(e) => {
            draggingRef.current = true;
            (e.currentTarget as SVGSVGElement).setPointerCapture(e.pointerId);
            updateFromClientPoint(e.clientX, e.clientY);
          }}
          onPointerMove={(e) => {
            if (!draggingRef.current) return;
            updateFromClientPoint(e.clientX, e.clientY);
          }}
          onPointerUp={() => {
            draggingRef.current = false;
          }}
        >
          <defs>
            <radialGradient id="solarGlow" cx="50%" cy="50%" r="50%">
              <stop offset="0%" stopColor={sky.orbColor} stopOpacity="0.95" />
              <stop offset="50%" stopColor={sky.orbColor} stopOpacity="0.32" />
              <stop offset="100%" stopColor={sky.orbColor} stopOpacity="0" />
            </radialGradient>
            <linearGradient id="arcGrad" x1="0%" y1="0%" x2="100%" y2="0%">
              <stop offset="0%" stopColor="#6366F1" stopOpacity="0.55" />
              <stop offset="28%" stopColor="#F59E0B" stopOpacity="0.9" />
              <stop offset="50%" stopColor="#FDE047" stopOpacity="1" />
              <stop offset="76%" stopColor="#F97316" stopOpacity="0.9" />
              <stop offset="100%" stopColor="#6366F1" stopOpacity="0.55" />
            </linearGradient>
          </defs>

          {/* Horizon line */}
          <line
            x1="28"
            y1={cy}
            x2="332"
            y2={cy}
            stroke="rgba(255,255,255,0.16)"
            strokeDasharray="4 4"
            strokeWidth="1"
          />

          {/* Base semicircular track */}
          <path
            d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
            fill="none"
            stroke="rgba(255,255,255,0.14)"
            strokeWidth="5"
            strokeLinecap="round"
          />

          {/* Colored solar arc */}
          <path
            d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
            fill="none"
            stroke="url(#arcGrad)"
            strokeWidth="3"
            strokeLinecap="round"
            strokeOpacity="0.85"
          />

          {/* Hour tick marks */}
          {[0, 0.25, 0.5, 0.75, 1].map((t) => {
            const a = Math.PI * (1 - t);
            const x1 = cx + (r - 6) * Math.cos(a);
            const y1 = cy - (r - 6) * Math.sin(a);
            const x2 = cx + (r + 6) * Math.cos(a);
            const y2 = cy - (r + 6) * Math.sin(a);
            return (
              <line
                key={t}
                x1={x1}
                y1={y1}
                x2={x2}
                y2={y2}
                stroke="rgba(255,255,255,0.3)"
                strokeWidth="1.2"
              />
            );
          })}

          {/* Orb halo */}
          <circle cx={orbX} cy={orbY} r="24" fill="url(#solarGlow)" />
          {/* Orb core */}
          <circle
            cx={orbX}
            cy={orbY}
            r="8.5"
            fill={sky.orbColor}
            stroke="#FFFFFF"
            strokeWidth="2"
          />

          {/* Center time readout */}
          <text
            x={cx}
            y={cy - 28}
            textAnchor="middle"
            fill="#FFFFFF"
            fontSize="28"
            fontWeight="700"
            fontFamily="JetBrains Mono, monospace"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {formatMinutes(minutes)}
          </text>
          <text
            x={cx}
            y={cy - 10}
            textAnchor="middle"
            fill="rgba(255,255,255,0.72)"
            fontSize="10"
            letterSpacing="2"
            fontFamily="JetBrains Mono, monospace"
          >
            {sky.label.toUpperCase()}
          </text>

          {/* Horizon labels */}
          <text
            x="40"
            y={cy + 14}
            fill="rgba(255,255,255,0.45)"
            fontSize="9"
            fontFamily="JetBrains Mono, monospace"
          >
            00:00
          </text>
          <text
            x={cx}
            y={cy + 14}
            textAnchor="middle"
            fill="rgba(255,255,255,0.45)"
            fontSize="9"
            fontFamily="JetBrains Mono, monospace"
          >
            12:00
          </text>
          <text
            x="320"
            y={cy + 14}
            textAnchor="end"
            fill="rgba(255,255,255,0.45)"
            fontSize="9"
            fontFamily="JetBrains Mono, monospace"
          >
            23:59
          </text>
        </svg>

        <div className="absolute top-2.5 right-2.5 flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-black/45 border border-white/[0.1] text-[10px] font-mono text-white/80">
          {sky.isNight ? (
            <Moon className="w-3 h-3 text-indigo-300" />
          ) : (
            <Sun className="w-3 h-3 text-amber-300" />
          )}
          Drag arc
        </div>
      </div>

      {/* Quick Time Pills */}
      <div className="flex flex-wrap items-center gap-1.5">
        {TIME_QUICK_PRESETS.map((p) => {
          const active = Math.abs(minutes - p.minutes) <= 15;
          return (
            <button
              key={p.label}
              type="button"
              onClick={() => onChange(p.minutes)}
              className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all cursor-pointer border ${
                active
                  ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                  : "bg-white/[0.04] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)] hover:bg-white/[0.08]"
              }`}
            >
              {p.label}
            </button>
          );
        })}
      </div>

      <Slider
        label="Exact Time of Day"
        value={minutes}
        min={0}
        max={1439}
        step={1}
        formatValue={formatMinutes}
        onChange={onChange}
      />
    </div>
  );
}

// ============================================================================
// Main DrivePage Component
// ============================================================================

export function DrivePage() {
  const {
    armedSession,
    cars,
    tracks,
    accent,
    gameRunning,
    armCar,
    armTrack,
    setArmedSession,
    notify,
  } = useAppStore();

  const accentHex = useMemo(
    () => ACCENT_PRESETS.find((p) => p.id === accent)?.hex ?? "#FF4D2E",
    [accent]
  );

  // Track 2D/3D preview state + cache
  const [trackViewMode, setTrackViewMode] = useState<"2d" | "3d">("2d");
  const [armedTrackScene, setArmedTrackScene] = useState<Track3DSceneData | null>(null);
  const [loadingTrackScene, setLoadingTrackScene] = useState(false);
  const [pickerTrackMode, setPickerTrackMode] = useState<"2d" | "3d">("2d");
  const [pickerPreviewTrack, setPickerPreviewTrack] = useState<TrackEntry | null>(null);
  const [pickerPreviewLayoutId, setPickerPreviewLayoutId] = useState<string>("default");
  const [pickerTrackScene, setPickerTrackScene] = useState<Track3DSceneData | null>(null);
  const [loadingPickerScene, setLoadingPickerScene] = useState(false);
  const trackSceneCacheRef = useRef<Map<string, Track3DSceneData>>(new Map());

  // Session params & combinable Weekend sessions
  const [practiceEnabled, setPracticeEnabled] = useState(true);
  const [practiceMinutes, setPracticeMinutes] = useState(15);
  const [qualifyEnabled, setQualifyEnabled] = useState(true);
  const [qualifyMinutes, setQualifyMinutes] = useState(10);
  const [raceEnabled, setRaceEnabled] = useState(true);
  const [laps, setLaps] = useState(8);
  const [durationMinutes, setDurationMinutes] = useState(20);
  const [dragMatches, setDragMatches] = useState(3);
  const [unlimitedTrackDay, setUnlimitedTrackDay] = useState(true);
  const [timeOfDayMinutes, setTimeOfDayMinutes] = useState(660);
  const [weatherId, setWeatherId] = useState("3_clear");
  const [weatherControllers, setWeatherControllers] = useState<
    WeatherController[]
  >([]);
  const [weatherController, setWeatherController] = useState("sol");
  const [ambientTempC, setAmbientTempC] = useState(24);
  const [roadTempC, setRoadTempC] = useState(32);
  const [trackGripPreset, setTrackGripPreset] = useState("Optimum");

  // Assists & Realism
  const [penaltiesEnabled, setPenaltiesEnabled] = useState(true);
  const [damageMultiplier, setDamageMultiplier] = useState(50);
  const [fuelRate, setFuelRate] = useState(100);
  const [tyreWearRate, setTyreWearRate] = useState(100);
  const [ghostEnabled, setGhostEnabled] = useState(false);
  const [ghostAdvantageSec, setGhostAdvantageSec] = useState(0);
  const [aiFlood, setAiFlood] = useState(false);

  // Player handicaps
  const [playerBallastKg, setPlayerBallastKg] = useState(0);
  const [playerRestrictor, setPlayerRestrictor] = useState(0);

  // AI Grid
  const [aiOpponents, setAiOpponents] = useState<AiOpponent[]>([]);
  const [defaultAiSkill, setDefaultAiSkill] = useState(94);
  const [defaultAiAggression, setDefaultAiAggression] = useState(45);
  const [adaptiveSuggestion, setAdaptiveSuggestion] =
    useState<AdaptiveAiSuggestion | null>(null);
  const [computingAdaptive, setComputingAdaptive] = useState(false);

  // Skins & Presets
  const [skins, setSkins] = useState<SkinEntry[]>([]);
  const [presets, setPresets] = useState<QuickRacePreset[]>([]);
  const [newPresetName, setNewPresetName] = useState("");
  const [showSavePreset, setShowSavePreset] = useState(false);
  const [launching, setLaunching] = useState(false);

  // Visual Picker Modal State
  const [pickerMode, setPickerMode] = useState<
    "car" | "track" | "opponent" | null
  >(null);
  const [pickerQuery, setPickerQuery] = useState("");
  const [pickerFilter, setPickerFilter] = useState("All");

  useEffect(() => {
    if (!pickerMode) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setPickerMode(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [pickerMode]);

  const armedCar = useMemo(
    () => cars.find((c) => c.id === armedSession.carId) ?? cars[0] ?? null,
    [cars, armedSession.carId]
  );
  const armedTrack = useMemo(
    () => tracks.find((t) => t.id === armedSession.trackId) ?? tracks[0] ?? null,
    [tracks, armedSession.trackId]
  );

  const openTrackPicker = () => {
    setPickerPreviewTrack(armedTrack);
    setPickerPreviewLayoutId(
      armedSession.layoutId || armedTrack?.layouts[0]?.id || "default"
    );
    setPickerMode("track");
    setPickerQuery("");
    setPickerFilter("All");
  };

  useEffect(() => {
    if (trackViewMode !== "3d" || !armedTrack) return;
    const layoutId =
      armedSession.layoutId || armedTrack.layouts[0]?.id || "default";
    const key = `${armedTrack.path}::${layoutId}`;
    const cached = trackSceneCacheRef.current.get(key);
    if (cached) {
      setArmedTrackScene(cached);
      return;
    }
    let cancelled = false;
    setLoadingTrackScene(true);
    api
      .loadTrack3DScene(armedTrack.path, layoutId)
      .then((scene) => {
        if (cancelled) return;
        if (scene) trackSceneCacheRef.current.set(key, scene);
        setArmedTrackScene(scene ?? null);
      })
      .catch(() => {
        if (!cancelled) setArmedTrackScene(null);
      })
      .finally(() => {
        if (!cancelled) setLoadingTrackScene(false);
      });
    return () => {
      cancelled = true;
    };
  }, [trackViewMode, armedTrack, armedSession.layoutId]);

  useEffect(() => {
    if (pickerMode !== "track" || pickerTrackMode !== "3d" || !pickerPreviewTrack) {
      return;
    }
    const layoutId =
      pickerPreviewLayoutId || pickerPreviewTrack.layouts[0]?.id || "default";
    const key = `${pickerPreviewTrack.path}::${layoutId}`;
    const cached = trackSceneCacheRef.current.get(key);
    if (cached) {
      setPickerTrackScene(cached);
      return;
    }
    let cancelled = false;
    setLoadingPickerScene(true);
    api
      .loadTrack3DScene(pickerPreviewTrack.path, layoutId)
      .then((scene) => {
        if (cancelled) return;
        if (scene) trackSceneCacheRef.current.set(key, scene);
        setPickerTrackScene(scene ?? null);
      })
      .catch(() => {
        if (!cancelled) setPickerTrackScene(null);
      })
      .finally(() => {
        if (!cancelled) setLoadingPickerScene(false);
      });
    return () => {
      cancelled = true;
    };
  }, [pickerMode, pickerTrackMode, pickerPreviewTrack, pickerPreviewLayoutId]);

  // Load weather controllers & presets on mount
  useEffect(() => {
    void api
      .listWeatherControllers()
      .then((list) => {
        if (list && list.length > 0) {
          setWeatherControllers(list);
          setWeatherController(list[0].id);
        }
      })
      .catch(() => {});
    void api
      .getPresets()
      .then((list) => setPresets(list ?? []))
      .catch(() => {});
  }, []);

  // Load skins when armed car changes
  useEffect(() => {
    if (!armedCar?.path) {
      setSkins([]);
      return;
    }
    void api
      .getCarSkins(armedCar.path)
      .then((list) => {
        setSkins(list ?? []);
        if (
          list &&
          list.length > 0 &&
          !list.some((s) => s.id === armedSession.skinId)
        ) {
          setArmedSession({ skinId: list[0].id });
        }
      })
      .catch(() => setSkins([]));
  }, [armedCar?.path, armedSession.skinId, setArmedSession]);

  const effectiveMode: SessionMode = MODE_OPTIONS.some(
    (m) => m.value === armedSession.mode
  )
    ? armedSession.mode
    : "Weekend";

  const buildRequest = (): QuickRaceRequest => ({
    carId: armedCar?.id ?? armedSession.carId,
    skinId: armedSession.skinId,
    trackId: armedTrack?.id ?? armedSession.trackId,
    layoutId: armedSession.layoutId,
    mode: armedSession.mode,
    laps,
    durationMinutes,
    practiceEnabled,
    practiceMinutes,
    qualifyEnabled,
    qualifyMinutes,
    raceEnabled,
    trackDayMinutes: unlimitedTrackDay ? 0 : durationMinutes,
    dragMatches,
    playerBallastKg,
    playerRestrictor,
    aiLevel: defaultAiSkill,
    aiAggression: defaultAiAggression,
    aiOpponents: aiOpponents.map((o) => ({
      ...o,
      ballastKg: o.ballastKg ?? 0,
      restrictor: o.restrictor ?? 0,
    })),
    timeOfDayMinutes,
    weatherId,
    weatherController,
    ambientTempC,
    roadTempC,
    trackGripPreset,
    ghostEnabled,
    ghostAdvantageSec,
    penaltiesEnabled,
    damageMultiplier,
    fuelRate,
    tyreWearRate,
    aiFlood,
  });

  const handleLaunch = async () => {
    if (gameRunning) {
      notify("Assetto Corsa is already running", "warning");
      return;
    }
    setLaunching(true);
    try {
      await api.launchQuickRace(buildRequest());
      notify(
        "Session launched",
        "success",
        `${armedCar?.name ?? armedSession.carName} @ ${
          armedTrack?.name ?? armedSession.trackName
        }${playerBallastKg > 0 ? ` (+${playerBallastKg}kg)` : ""}`
      );
    } catch (err) {
      notify("Failed to launch session", "error", String(err));
    } finally {
      setLaunching(false);
    }
  };

  const handleSavePreset = async () => {
    const trimmed = newPresetName.trim();
    if (!trimmed) return;
    const preset: QuickRacePreset = {
      name: trimmed,
      description: `${armedCar?.name ?? armedSession.carName} @ ${
        armedTrack?.name ?? armedSession.trackName
      }`,
      createdAt: new Date().toISOString(),
      request: buildRequest(),
    };
    try {
      await api.savePreset(preset);
      setPresets((prev) => [
        preset,
        ...prev.filter((p) => p.name !== preset.name),
      ]);
      setNewPresetName("");
      setShowSavePreset(false);
      notify("Preset saved", "success", trimmed);
    } catch (err) {
      notify("Failed to save preset", "error", String(err));
    }
  };

  const handleApplyPreset = (preset: QuickRacePreset) => {
    const r = preset.request;
    const foundCar = cars.find((c) => c.id === r.carId);
    const foundTrack = tracks.find((t) => t.id === r.trackId);
    if (foundCar) armCar(foundCar, r.skinId);
    if (foundTrack) armTrack(foundTrack, r.layoutId);
    setArmedSession({ mode: r.mode });
    setLaps(r.laps);
    setDurationMinutes(r.durationMinutes);
    if (r.practiceEnabled !== undefined) setPracticeEnabled(r.practiceEnabled);
    if (r.practiceMinutes !== undefined) setPracticeMinutes(r.practiceMinutes);
    if (r.qualifyEnabled !== undefined) setQualifyEnabled(r.qualifyEnabled);
    if (r.qualifyMinutes !== undefined) setQualifyMinutes(r.qualifyMinutes);
    if (r.raceEnabled !== undefined) setRaceEnabled(r.raceEnabled);
    if (r.trackDayMinutes !== undefined) {
      setUnlimitedTrackDay(r.trackDayMinutes === 0);
      if (r.trackDayMinutes > 0) setDurationMinutes(r.trackDayMinutes);
    }
    if (r.dragMatches !== undefined) setDragMatches(r.dragMatches);
    if (r.playerBallastKg !== undefined) setPlayerBallastKg(r.playerBallastKg);
    if (r.playerRestrictor !== undefined) setPlayerRestrictor(r.playerRestrictor);
    if (r.aiLevel !== undefined) setDefaultAiSkill(r.aiLevel);
    if (r.aiAggression !== undefined) setDefaultAiAggression(r.aiAggression);
    setAiOpponents(r.aiOpponents ?? []);
    setTimeOfDayMinutes(r.timeOfDayMinutes);
    setWeatherId(r.weatherId);
    if (r.weatherController) setWeatherController(r.weatherController);
    setAmbientTempC(r.ambientTempC);
    setRoadTempC(r.roadTempC);
    setTrackGripPreset(r.trackGripPreset);
    setGhostEnabled(r.ghostEnabled);
    setGhostAdvantageSec(r.ghostAdvantageSec);
    setPenaltiesEnabled(r.penaltiesEnabled);
    setDamageMultiplier(r.damageMultiplier);
    setFuelRate(r.fuelRate);
    setTyreWearRate(r.tyreWearRate);
    setAiFlood(Boolean(r.aiFlood));
    notify("Preset loaded", "info", preset.name);
  };

  const handleDeletePreset = async (name: string) => {
    try {
      await api.deletePreset(name);
      setPresets((prev) => prev.filter((p) => p.name !== name));
      notify("Preset deleted", "info", name);
    } catch (err) {
      notify("Failed to delete preset", "error", String(err));
    }
  };

  const handleMatchMyPace = async () => {
    setComputingAdaptive(true);
    try {
      const count = Math.max(aiOpponents.length, 6);
      const res = await api.computeAdaptiveAi(
        armedCar?.id ?? armedSession.carId,
        armedTrack?.id ?? armedSession.trackId,
        count,
        4
      );
      if (res) {
        const recLevel = Number.isFinite(res.recommendedLevel)
          ? Math.round(res.recommendedLevel)
          : defaultAiSkill;
        const minLevel = Number.isFinite(res.minLevel)
          ? Math.round(res.minLevel)
          : Math.max(70, recLevel - 2);
        const maxLevel = Number.isFinite(res.maxLevel)
          ? Math.round(res.maxLevel)
          : Math.min(100, recLevel + 2);
        const confidence = Number.isFinite(res.confidence)
          ? res.confidence
          : 0.75;
        const sampleLaps = Number.isFinite(res.sampleLaps)
          ? res.sampleLaps
          : 0;
        const safeRes: AdaptiveAiSuggestion = {
          ...res,
          recommendedLevel: recLevel,
          minLevel,
          maxLevel,
          confidence,
          sampleLaps,
        };
        setAdaptiveSuggestion(safeRes);
        setDefaultAiSkill(recLevel);
        if (safeRes.opponents && safeRes.opponents.length > 0) {
          const pool = cars.length > 0 ? cars : [];
          setAiOpponents((prev) =>
            safeRes.opponents.map((opp, i) => ({
              ...opp,
              carId:
                prev[i]?.carId ||
                opp.carId ||
                pool[i % Math.max(1, pool.length)]?.id ||
                armedSession.carId,
              skinId: prev[i]?.skinId ?? opp.skinId,
              driverName:
                prev[i]?.driverName || opp.driverName || `AI Driver ${i + 1}`,
              aiLevel: Number.isFinite(opp.aiLevel) ? opp.aiLevel : recLevel,
              aiAggression: Number.isFinite(opp.aiAggression)
                ? opp.aiAggression
                : defaultAiAggression,
            }))
          );
        } else {
          // Generate balanced opponents using recommended level
          const pool = cars.length > 0 ? cars : [];
          const generated: AiOpponent[] = Array.from({ length: count }).map(
            (_, i) => {
              const c = pool[i % Math.max(1, pool.length)];
              const offset = Math.round(((i - count / 2) / count) * 4);
              return {
                carId: c?.id ?? armedSession.carId,
                driverName: `AI Driver ${i + 1}`,
                aiLevel: Math.min(100, Math.max(70, Math.round(recLevel + offset))),
                aiAggression: defaultAiAggression,
              };
            }
          );
          setAiOpponents(generated);
        }
        notify(
          "Adaptive AI matched to your pace",
          "success",
          `Recommended skill ${recLevel}% (${sampleLaps} sample ${
            sampleLaps === 1 ? "signal" : "signals"
          })`
        );
      } else {
        notify(
          "Adaptive AI data unavailable",
          "warning",
          "No lap telemetry or backend adaptive AI data found."
        );
      }
    } catch (err) {
      notify("Adaptive AI calculation failed", "error", String(err));
    } finally {
      setComputingAdaptive(false);
    }
  };

  const handleAddOpponentCar = (car: CarEntry) => {
    setAiOpponents((prev) => [
      ...prev,
      {
        carId: car.id,
        driverName: `${car.brand} #${prev.length + 11}`,
        aiLevel: defaultAiSkill,
        aiAggression: defaultAiAggression,
      },
    ]);
  };

  const applyBatchAiStats = (skill: number, aggression: number) => {
    setAiOpponents((prev) =>
      prev.map((o) => ({ ...o, aiLevel: skill, aiAggression: aggression }))
    );
  };

  // Picker filtering
  const pickerBrands = useMemo(() => {
    if (pickerMode === "track") {
      return ["All", ...Array.from(new Set(tracks.map((t) => t.country)))];
    }
    return ["All", ...Array.from(new Set(cars.map((c) => c.brand)))];
  }, [pickerMode, cars, tracks]);

  const filteredPickerCars = useMemo(() => {
    const q = pickerQuery.trim().toLowerCase();
    return cars.filter((c) => {
      if (pickerFilter !== "All" && c.brand !== pickerFilter) return false;
      if (!q) return true;
      return (
        c.name.toLowerCase().includes(q) ||
        c.brand.toLowerCase().includes(q) ||
        c.class.toLowerCase().includes(q) ||
        c.tags.some((t) => t.toLowerCase().includes(q))
      );
    });
  }, [cars, pickerQuery, pickerFilter]);

  const filteredPickerTracks = useMemo(() => {
    const q = pickerQuery.trim().toLowerCase();
    return tracks.filter((t) => {
      if (pickerFilter !== "All" && t.country !== pickerFilter) return false;
      if (!q) return true;
      return (
        t.name.toLowerCase().includes(q) ||
        t.country.toLowerCase().includes(q) ||
        t.tags.some((tg) => tg.toLowerCase().includes(q))
      );
    });
  }, [tracks, pickerQuery, pickerFilter]);

  const carPreview = assetUrl(armedCar?.previewPath ?? armedSession.carPreviewPath);
  const activeArmedLayout = useMemo(
    () =>
      armedTrack?.layouts.find((l) => l.id === armedSession.layoutId) ??
      armedTrack?.layouts[0] ??
      null,
    [armedTrack, armedSession.layoutId]
  );
  const trackPreview = assetUrl(
    activeArmedLayout?.previewPath ??
      armedTrack?.previewPath ??
      armedSession.trackPreviewPath
  );

  return (
    <div className="h-full overflow-y-auto px-6 py-5 animate-fade-in">
      {/* Top Mode & Quick Status Bar */}
      <div className="flex flex-wrap items-center justify-between gap-3 pb-4 mb-5 border-b border-white/[0.06]">
        <div>
          <div className="text-[11px] font-mono uppercase tracking-[0.14em] text-[var(--accent)]">
            Drive · Session Cockpit
          </div>
          <h1 className="font-display text-3xl font-bold uppercase tracking-wider text-[var(--text-primary)] leading-none mt-0.5">
            Quick Race & Practice
          </h1>
        </div>
        <SegmentedControl
          options={MODE_OPTIONS}
          value={effectiveMode}
          onChange={(m) => setArmedSession({ mode: m })}
          size="sm"
        />
      </div>

      {/* 3-Column AAA Drive Cockpit */}
      <div className="grid grid-cols-1 xl:grid-cols-12 gap-5 items-start">
        {/* ================================================================= */}
        {/* LEFT COLUMN (4 cols): Armed Car, Armed Track, Presets             */}
        {/* ================================================================= */}
        <div className="xl:col-span-4 min-w-0 space-y-4">
          {/* Armed Car Card */}
          <Card variant="glass" padding="none" className="overflow-hidden">
            <div
              role="button"
              tabIndex={0}
              aria-label={`Change car: ${armedCar?.name ?? armedSession.carName}`}
              onClick={() => {
                setPickerMode("car");
                setPickerQuery("");
                setPickerFilter("All");
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setPickerMode("car");
                  setPickerQuery("");
                  setPickerFilter("All");
                }
              }}
              className="relative h-40 cursor-pointer group overflow-hidden focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--accent)]"
            >
              {carPreview ? (
                <img
                  src={carPreview}
                  alt={armedCar?.name ?? armedSession.carName}
                  className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
                />
              ) : (
                <div
                  className="w-full h-full"
                  style={{
                    background:
                      "radial-gradient(360px 180px at 80% 25%, rgba(var(--accent-rgb), 0.25), transparent 70%), linear-gradient(135deg, #131B29 0%, #090D14 100%)",
                  }}
                />
              )}
              <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-[#0D1118]/40 to-transparent" />
              <div className="absolute top-3 left-3 flex items-center gap-1.5">
                <Badge variant="accent">CAR</Badge>
                {armedCar?.class && (
                  <Badge variant="neutral">{armedCar.class}</Badge>
                )}
              </div>
              <div className="absolute top-3 right-3 px-2 py-1 rounded-md bg-black/55 border border-white/[0.12] text-[11px] font-medium text-white/90 group-hover:border-[var(--accent-border)] transition-colors">
                Change Car
              </div>
              <div className="absolute bottom-3 inset-x-4">
                <div className="text-[10px] font-mono uppercase tracking-[0.14em] text-[var(--accent)]">
                  {armedCar?.brand ?? "Selected Machine"}
                </div>
                <div className="font-display text-2xl font-bold uppercase tracking-wide text-white truncate">
                  {armedCar?.name ?? armedSession.carName}
                </div>
              </div>
            </div>

            <div className="p-3.5 space-y-3">
              {/* Car Specs */}
              <div className="grid grid-cols-3 gap-2 text-center">
                {[
                  { k: "POWER", v: armedCar?.bhp ?? "500 bhp" },
                  { k: "WEIGHT", v: armedCar?.weight ?? "1220 kg" },
                  { k: "TOP SPD", v: armedCar?.topSpeed ?? "292 km/h" },
                ].map((s) => (
                  <div
                    key={s.k}
                    className="rounded-lg bg-white/[0.03] border border-white/[0.06] py-1.5 px-2"
                  >
                    <div className="text-[9px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                      {s.k}
                    </div>
                    <div className="text-xs font-mono font-semibold text-[var(--text-primary)] tabular-nums">
                      {s.v}
                    </div>
                  </div>
                ))}
              </div>

              {/* Livery Strip */}
              {skins.length > 0 && (
                <div className="space-y-1.5">
                  <div className="text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Livery ({skins.length})
                  </div>
                  <div className="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto pr-0.5">
                    {skins.map((s) => {
                      const active = s.id === armedSession.skinId;
                      return (
                        <button
                          key={s.id}
                          type="button"
                          onClick={() => setArmedSession({ skinId: s.id })}
                          className={`px-2.5 py-1 rounded-lg text-[11px] whitespace-nowrap border transition-all cursor-pointer ${
                            active
                              ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)] font-semibold"
                              : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                          }`}
                        >
                          {s.number ? `#${s.number} ` : ""}
                          {s.name}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          </Card>

          {/* Armed Track Card */}
          <Card variant="glass" padding="none" className="overflow-hidden">
            {/* Compact Top Control Bar: 2D / 3D Rotating Preview + Change Track */}
            <div className="px-3.5 py-2 border-b border-white/[0.06] bg-black/25 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 min-w-0">
                <Badge variant="info">CIRCUIT</Badge>
                <span className="text-xs font-semibold text-white truncate">
                  {armedTrack?.name ?? armedSession.trackName}
                </span>
              </div>
              <div className="flex items-center gap-1.5 shrink-0">
                <div className="flex items-center p-0.5 rounded-lg bg-black/50 border border-white/[0.08]">
                  <button
                    type="button"
                    onClick={() => setTrackViewMode("2d")}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-medium flex items-center gap-1 transition-colors cursor-pointer ${
                      trackViewMode === "2d"
                        ? "bg-white/[0.12] text-white"
                        : "text-[var(--text-muted)] hover:text-white"
                    }`}
                  >
                    <ImageIcon className="w-3 h-3" />
                    2D
                  </button>
                  <button
                    type="button"
                    onClick={() => setTrackViewMode("3d")}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-medium flex items-center gap-1 transition-colors cursor-pointer ${
                      trackViewMode === "3d"
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
                        : "text-[var(--text-muted)] hover:text-white"
                    }`}
                  >
                    <Box className="w-3 h-3" />
                    3D
                  </button>
                </div>
                <button
                  type="button"
                  onClick={openTrackPicker}
                  className="px-2 py-1 rounded-md bg-white/[0.06] hover:bg-white/[0.12] border border-white/[0.1] hover:border-[var(--accent-border)] text-[11px] font-medium text-white/90 transition-colors cursor-pointer whitespace-nowrap"
                >
                  Change Track
                </button>
              </div>
            </div>

            {trackViewMode === "3d" ? (
              <div className="h-56 relative bg-[#090E16]">
                <Viewport3D
                  mode="track"
                  trackData={armedTrackScene}
                  accentHex={accentHex}
                  loading={loadingTrackScene}
                  label={`3D CORRIDOR · ${(armedSession.layoutId || armedTrack?.layouts[0]?.id || "default").toUpperCase()}`}
                />
              </div>
            ) : (
              <div
                role="button"
                tabIndex={0}
                aria-label={`Change track: ${armedTrack?.name ?? armedSession.trackName}`}
                onClick={openTrackPicker}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    openTrackPicker();
                  }
                }}
                className="relative h-32 cursor-pointer group overflow-hidden focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--accent)]"
              >
                {trackPreview ? (
                  <img
                    src={trackPreview}
                    alt={armedTrack?.name ?? armedSession.trackName}
                    className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
                  />
                ) : (
                  <div
                    className="w-full h-full"
                    style={{
                      background:
                        "radial-gradient(320px 150px at 20% 25%, rgba(0, 209, 255, 0.2), transparent 70%), linear-gradient(135deg, #101925 0%, #090D14 100%)",
                    }}
                  />
                )}
                <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-[#0D1118]/45 to-transparent" />
                <div className="absolute bottom-3 inset-x-4">
                  <div className="flex items-center gap-1.5 text-[10px] font-mono uppercase tracking-[0.12em] text-cyan-300">
                    <MapPin className="w-3 h-3" />
                    <span>{armedTrack?.country ?? "Circuit"}</span>
                    {(activeArmedLayout?.lengthMeters
                      ? `${(activeArmedLayout.lengthMeters / 1000).toFixed(2)} km`
                      : armedTrack?.length) && (
                      <span>
                        ·{" "}
                        {activeArmedLayout?.lengthMeters
                          ? `${(activeArmedLayout.lengthMeters / 1000).toFixed(2)} km`
                          : armedTrack?.length}
                      </span>
                    )}
                  </div>
                  <div className="font-display text-xl font-bold uppercase tracking-wide text-white truncate">
                    {armedTrack?.name ?? armedSession.trackName}
                  </div>
                </div>
              </div>
            )}

            {/* Direct Track Layout Selector */}
            {armedTrack && armedTrack.layouts.length > 0 && (
              <div className="p-3.5 space-y-1.5">
                <div className="flex items-center justify-between text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  <span>Track Layout ({armedTrack.layouts.length})</span>
                  {activeArmedLayout && (
                    <span className="text-[var(--text-secondary)] truncate max-w-[160px]">
                      {activeArmedLayout.pitboxes ? `${activeArmedLayout.pitboxes} pits` : ""}
                      {activeArmedLayout.pitboxes && activeArmedLayout.lengthMeters ? " · " : ""}
                      {activeArmedLayout.lengthMeters
                        ? `${(activeArmedLayout.lengthMeters / 1000).toFixed(2)} km`
                        : ""}
                    </span>
                  )}
                </div>
                <div className="flex flex-wrap gap-1.5 max-h-24 overflow-y-auto pr-0.5">
                  {armedTrack.layouts.map((lay) => {
                    const active = lay.id === armedSession.layoutId;
                    return (
                      <button
                        key={lay.id}
                        type="button"
                        onClick={() => armTrack(armedTrack, lay.id)}
                        className={`px-2.5 py-1 rounded-lg text-[11px] font-medium border transition-all cursor-pointer ${
                          active
                            ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                            : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                        }`}
                      >
                        {lay.name}
                      </button>
                    );
                  })}
                </div>
              </div>
            )}
          </Card>

          {/* Saved Presets Card */}
          <Card variant="glass" padding="md" className="space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Bookmark className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                  Saved Presets
                </span>
              </div>
              <Button
                variant="ghost"
                size="sm"
                icon={<Save className="w-3.5 h-3.5" />}
                onClick={() => setShowSavePreset((v) => !v)}
              >
                Save Current
              </Button>
            </div>

            {showSavePreset && (
              <div className="flex items-center gap-2 pt-1">
                <input
                  type="text"
                  value={newPresetName}
                  onChange={(e) => setNewPresetName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleSavePreset()}
                  placeholder="Preset name (e.g. Spa GT3 Sunset)"
                  className="flex-1 h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                />
                <Button variant="primary" size="sm" onClick={handleSavePreset}>
                  Save
                </Button>
              </div>
            )}

            {presets.length === 0 ? (
              <p className="text-xs text-[var(--text-muted)]">
                No saved presets yet. Save your favorite car, track, weather,
                and AI grid for 1-click recall.
              </p>
            ) : (
              <div className="space-y-1.5 max-h-40 overflow-y-auto pr-1">
                {presets.map((p) => (
                  <div
                    key={p.name}
                    className="flex items-center justify-between gap-2 p-2 rounded-lg bg-white/[0.03] border border-white/[0.06] hover:border-white/[0.14] transition-colors"
                  >
                    <button
                      type="button"
                      onClick={() => handleApplyPreset(p)}
                      className="text-left min-w-0 flex-1 cursor-pointer"
                    >
                      <div className="text-xs font-semibold text-[var(--text-primary)] truncate">
                        {p.name}
                      </div>
                      {p.description && (
                        <div className="text-[11px] text-[var(--text-muted)] truncate">
                          {p.description}
                        </div>
                      )}
                    </button>
                    <button
                      type="button"
                      onClick={() => handleDeletePreset(p.name)}
                      className="p-1 text-[var(--text-muted)] hover:text-red-400 cursor-pointer"
                      aria-label={`Delete preset ${p.name}`}
                      title="Delete preset"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </Card>
        </div>

        {/* ================================================================= */}
        {/* CENTER COLUMN (4 cols): Solar Arc, WeatherFX, Grip, Realism       */}
        {/* ================================================================= */}
        <div className="xl:col-span-4 min-w-0 space-y-4">
          {/* Solar Arc & Environment Card */}
          <Card variant="glass" padding="md" className="space-y-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Sun className="w-4 h-4 text-amber-400" />
                <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                  Time of Day & WeatherFX
                </span>
              </div>
              <Badge variant="info">{weatherController.toUpperCase()}</Badge>
            </div>

            <SolarArcDial
              minutes={timeOfDayMinutes}
              onChange={setTimeOfDayMinutes}
            />

            {/* Weather Controller */}
            <div className="space-y-1.5 pt-1">
              <div className="text-[11px] font-medium text-[var(--text-secondary)]">
                Sol / Pure Controller
              </div>
              <div className="flex flex-wrap gap-1.5">
                {(weatherControllers.length > 0
                  ? weatherControllers
                  : [
                      { id: "sol", name: "Sol 2.2" },
                      { id: "pure", name: "Pure" },
                      { id: "default", name: "Kunos Default" },
                    ]
                ).map((wc) => {
                  const active = weatherController === wc.id;
                  return (
                    <button
                      key={wc.id}
                      type="button"
                      onClick={() => setWeatherController(wc.id)}
                      className={`px-2.5 py-1 rounded-lg text-xs font-medium border transition-all cursor-pointer ${
                        active
                          ? "bg-cyan-500/15 text-cyan-300 border-cyan-500/35"
                          : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                      }`}
                    >
                      {wc.name}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Weather Condition Pills */}
            <div className="space-y-1.5">
              <div className="text-[11px] font-medium text-[var(--text-secondary)]">
                Sky & Precipitation
              </div>
              <div className="grid grid-cols-3 gap-1.5">
                {WEATHER_PRESETS.map((w) => {
                  const Icon = w.icon;
                  const active = weatherId === w.id;
                  return (
                    <button
                      key={w.id}
                      type="button"
                      onClick={() => setWeatherId(w.id)}
                      className={`flex items-center justify-center gap-1.5 h-8 px-2 rounded-lg text-[11px] font-medium border transition-all cursor-pointer ${
                        active
                          ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                          : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                      }`}
                    >
                      <Icon className="w-3.5 h-3.5 shrink-0" />
                      <span className="truncate">{w.label}</span>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Temps */}
            <div className="grid grid-cols-2 gap-3 pt-1">
              <Slider
                label="Ambient"
                value={ambientTempC}
                min={5}
                max={42}
                unit="°C"
                onChange={setAmbientTempC}
              />
              <Slider
                label="Track Temp"
                value={roadTempC}
                min={8}
                max={58}
                unit="°C"
                onChange={setRoadTempC}
              />
            </div>

            {/* Dynamic Track Grip */}
            <div className="space-y-1.5 pt-1">
              <div className="text-[11px] font-medium text-[var(--text-secondary)]">
                Dynamic Track Surface Grip
              </div>
              <div className="grid grid-cols-5 gap-1">
                {GRIP_PRESETS.map((grip) => {
                  const active = trackGripPreset === grip;
                  return (
                    <button
                      key={grip}
                      type="button"
                      onClick={() => setTrackGripPreset(grip)}
                      className={`h-7 rounded-lg text-[11px] font-medium border transition-all cursor-pointer ${
                        active
                          ? "bg-emerald-500/15 text-emerald-300 border-emerald-500/35"
                          : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                      }`}
                    >
                      {grip}
                    </button>
                  );
                })}
              </div>
            </div>
          </Card>

          {/* Mode-Adaptive Session Rules & Realism */}
          <Card variant="glass" padding="md" className="space-y-3.5">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <Flame className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                  {effectiveMode === "Weekend"
                    ? "Weekend Sessions & Rules"
                    : effectiveMode === "TrackDay"
                      ? "Track Day & CSP Traffic"
                      : effectiveMode === "Hotlap"
                        ? "Hotlap & Ghost Setup"
                        : effectiveMode === "Drift"
                          ? "Drift Session Rules"
                          : effectiveMode === "Drag"
                            ? "Drag Strip Matches"
                            : "Time Attack Rules"}
                </span>
              </div>
              <Badge variant="accent">{effectiveMode}</Badge>
            </div>

            {/* WEEKEND: Combinable Practice + Qualifying + Race */}
            {effectiveMode === "Weekend" && (
              <div className="space-y-2.5">
                <div className="text-[11px] text-[var(--text-muted)]">
                  Combine any sessions for your race weekend:
                </div>
                <div className="grid grid-cols-3 gap-2">
                  {[
                    { label: "Practice", on: practiceEnabled, toggle: () => setPracticeEnabled((v) => !v), meta: `${practiceMinutes}m` },
                    { label: "Qualifying", on: qualifyEnabled, toggle: () => setQualifyEnabled((v) => !v), meta: `${qualifyMinutes}m` },
                    { label: "Race", on: raceEnabled, toggle: () => setRaceEnabled((v) => !v), meta: `${laps} laps` },
                  ].map((s) => (
                    <button
                      key={s.label}
                      type="button"
                      onClick={s.toggle}
                      className={`px-3 py-2 rounded-xl border text-left transition-all cursor-pointer ${
                        s.on
                          ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--text-primary)]"
                          : "bg-white/[0.02] border-white/[0.07] text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-semibold">{s.label}</span>
                        <span className={`w-2 h-2 rounded-full ${s.on ? "bg-[var(--accent)]" : "bg-white/20"}`} />
                      </div>
                      <div className="text-[10px] font-mono mt-0.5 opacity-80">
                        {s.on ? s.meta : "Skipped"}
                      </div>
                    </button>
                  ))}
                </div>

                <div className="grid grid-cols-1 sm:grid-cols-3 gap-2.5 pt-1">
                  {practiceEnabled && (
                    <Slider
                      label="Practice"
                      value={practiceMinutes}
                      min={5}
                      max={120}
                      step={5}
                      unit="m"
                      onChange={setPracticeMinutes}
                    />
                  )}
                  {qualifyEnabled && (
                    <Slider
                      label="Qualifying"
                      value={qualifyMinutes}
                      min={5}
                      max={60}
                      step={5}
                      unit="m"
                      onChange={setQualifyMinutes}
                    />
                  )}
                  {raceEnabled && (
                    <Slider
                      label="Race Laps"
                      value={laps}
                      min={1}
                      max={60}
                      onChange={setLaps}
                    />
                  )}
                </div>
              </div>
            )}

            {/* TRACK DAY: Duration + CSP AI Flood */}
            {effectiveMode === "TrackDay" && (
              <div className="space-y-2.5">
                <Toggle
                  checked={unlimitedTrackDay}
                  onChange={setUnlimitedTrackDay}
                  label="Unlimited Open Pitlane"
                  description="Drive as long as you want without session timer"
                />
                {!unlimitedTrackDay && (
                  <Slider
                    label="Session Duration"
                    value={durationMinutes}
                    min={10}
                    max={180}
                    step={5}
                    unit="m"
                    onChange={setDurationMinutes}
                  />
                )}
                <Toggle
                  checked={aiFlood}
                  onChange={setAiFlood}
                  label="CSP AI Flood Traffic"
                  description="Continuous respawning highway & trackday traffic around your car"
                />
              </div>
            )}

            {/* DRAG: Best-of Matches */}
            {effectiveMode === "Drag" && (
              <Slider
                label="Best-of Matches"
                value={dragMatches}
                min={1}
                max={15}
                step={2}
                unit=" runs"
                onChange={setDragMatches}
              />
            )}

            {/* DRIFT: Session Duration */}
            {effectiveMode === "Drift" && (
              <Slider
                label="Drift Session Time"
                value={durationMinutes}
                min={5}
                max={90}
                step={5}
                unit="m"
                onChange={setDurationMinutes}
              />
            )}

            {/* HOTLAP: Ghost Car front-and-center */}
            {effectiveMode === "Hotlap" && (
              <div className="space-y-2.5">
                <Toggle
                  checked={ghostEnabled}
                  onChange={setGhostEnabled}
                  label="Personal Best Ghost Car"
                  description="Spawn your fastest lap ghost directly on track"
                />
                {ghostEnabled && (
                  <div className="pl-2 border-l-2 border-[var(--accent-border)]">
                    <Slider
                      label="Ghost Lead Offset"
                      value={ghostAdvantageSec}
                      min={-5}
                      max={5}
                      step={0.1}
                      formatValue={(v) => `${v >= 0 ? "+" : ""}${v.toFixed(1)}s`}
                      onChange={setGhostAdvantageSec}
                    />
                  </div>
                )}
              </div>
            )}

            {/* Realism Sliders */}
            <div className="grid grid-cols-3 gap-2.5 pt-1 border-t border-white/[0.06]">
              <Slider
                label="Damage"
                value={damageMultiplier}
                min={0}
                max={100}
                step={10}
                unit="%"
                onChange={setDamageMultiplier}
              />
              <Slider
                label="Fuel"
                value={fuelRate}
                min={0}
                max={200}
                step={25}
                unit="%"
                onChange={setFuelRate}
              />
              <Slider
                label="Tyre Wear"
                value={tyreWearRate}
                min={0}
                max={200}
                step={25}
                unit="%"
                onChange={setTyreWearRate}
              />
            </div>

            {(effectiveMode === "Weekend" || effectiveMode === "Hotlap" || effectiveMode === "TimeAttack") && (
              <div className="pt-1 border-t border-white/[0.06]">
                <Toggle
                  checked={penaltiesEnabled}
                  onChange={setPenaltiesEnabled}
                  label="Track Cut Penalties"
                  description="Enforce 3-wheel off-track slowdown penalties"
                />
              </div>
            )}
          </Card>
        </div>

        {/* ================================================================= */}
        {/* RIGHT COLUMN (4 cols): Mode-Specific Opponents / Solo Info, BoP   */}
        {/* ================================================================= */}
        <div className="xl:col-span-4 min-w-0 space-y-4">
          {effectiveMode === "Hotlap" || effectiveMode === "Drift" || effectiveMode === "TimeAttack" ? (
            <Card variant="glass" padding="md" className="space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Ghost className="w-4 h-4 text-[var(--accent)]" />
                  <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                    {effectiveMode === "Drift"
                      ? "Solo Drift Stage"
                      : effectiveMode === "Hotlap"
                        ? "Solo Hotlap Stage"
                        : "Solo Time Attack Stage"}
                  </span>
                </div>
                <Badge variant="info">Solo</Badge>
              </div>
              <p className="text-xs text-[var(--text-secondary)] leading-relaxed">
                {effectiveMode === "Drift"
                  ? "No AI traffic on track — full circuit clearance for chaining drift combos, angle multipliers, and clean transitions."
                  : effectiveMode === "Hotlap"
                    ? "Rolling flying start with clean track and optional personal-best ghost telemetry."
                    : "Checkpoint-based solo run against the clock. Track racing-line (fast_lane.ai) is checked before launch."}
              </p>
            </Card>
          ) : (
            <Card variant="glass" padding="md" className="space-y-3.5">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <Users className="w-4 h-4 text-[var(--accent)]" />
                  <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                    {effectiveMode === "Drag" ? "1v1 Drag Rival" : "AI Grid Builder"}
                  </span>
                  <Badge variant="neutral">
                    {effectiveMode === "Drag"
                      ? `${Math.min(1, aiOpponents.length)} / 1`
                      : `${aiOpponents.length} AI`}
                  </Badge>
                </div>
                {aiOpponents.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setAiOpponents([])}
                    className="text-[11px] text-[var(--text-muted)] hover:text-red-300 cursor-pointer"
                  >
                    Clear
                  </button>
                )}
              </div>

              <div className="grid grid-cols-2 gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<Plus className="w-3.5 h-3.5" />}
                  onClick={() => {
                    setPickerMode("opponent");
                    setPickerQuery("");
                    setPickerFilter("All");
                  }}
                >
                  {effectiveMode === "Drag" ? "Pick Rival Car" : "Add Opponent"}
                </Button>
                <Button
                  variant="secondary"
                  size="sm"
                  loading={computingAdaptive}
                  icon={<Sparkles className="w-3.5 h-3.5 text-amber-300" />}
                  onClick={handleMatchMyPace}
                  className="border-amber-500/25 hover:border-amber-400/50"
                >
                  Match My Pace
                </Button>
              </div>

              {adaptiveSuggestion && (
                <div className="rounded-lg bg-amber-500/10 border border-amber-500/25 px-3 py-2 flex items-center justify-between text-[11px]">
                  <span className="text-amber-200">
                    Adaptive Target:{" "}
                    <strong className="font-mono">
                      {Number.isFinite(adaptiveSuggestion.recommendedLevel)
                        ? Math.round(adaptiveSuggestion.recommendedLevel)
                        : defaultAiSkill}
                      %
                    </strong>{" "}
                    (
                    {Number.isFinite(adaptiveSuggestion.minLevel)
                      ? adaptiveSuggestion.minLevel
                      : defaultAiSkill}
                    –
                    {Number.isFinite(adaptiveSuggestion.maxLevel)
                      ? adaptiveSuggestion.maxLevel
                      : defaultAiSkill}
                    %)
                  </span>
                  <span className="font-mono text-amber-300/80">
                    {Number.isFinite(adaptiveSuggestion.confidence)
                      ? Math.round(adaptiveSuggestion.confidence * 100)
                      : 75}
                    % conf
                  </span>
                </div>
              )}

              <div className="grid grid-cols-2 gap-3">
                <Slider
                  label="Grid Skill"
                  value={defaultAiSkill}
                  min={70}
                  max={100}
                  unit="%"
                  onChange={(v) => {
                    setDefaultAiSkill(v);
                    applyBatchAiStats(v, defaultAiAggression);
                  }}
                />
                <Slider
                  label="Aggression"
                  value={defaultAiAggression}
                  min={0}
                  max={100}
                  unit="%"
                  onChange={(v) => {
                    setDefaultAiAggression(v);
                    applyBatchAiStats(defaultAiSkill, v);
                  }}
                />
              </div>

              {aiOpponents.length === 0 ? (
                <div className="rounded-xl border border-dashed border-white/[0.1] p-5 text-center space-y-1.5">
                  <Ghost className="w-5 h-5 text-[var(--text-muted)] mx-auto" />
                  <div className="text-xs font-medium text-[var(--text-secondary)]">
                    {effectiveMode === "Drag" ? "No Drag Rival Selected" : "Solo Track Session"}
                  </div>
                  <p className="text-[11px] text-[var(--text-muted)]">
                    {effectiveMode === "Drag"
                      ? "Pick a rival car above to race head-to-head down the strip."
                      : "Add multi-class AI opponents or click Match My Pace to build a balanced grid."}
                  </p>
                </div>
              ) : (
                <div className="space-y-2 max-h-[260px] overflow-y-auto pr-1">
                  {(effectiveMode === "Drag" ? aiOpponents.slice(0, 1) : aiOpponents).map((opp, idx) => {
                    const carInfo = cars.find((c) => c.id === opp.carId);
                    return (
                      <div
                        key={`${opp.carId}-${idx}`}
                        className="rounded-lg bg-white/[0.03] border border-white/[0.07] p-2.5 space-y-2"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <div className="min-w-0">
                            <div className="text-xs font-semibold text-[var(--text-primary)] truncate">
                              {carInfo?.name ?? opp.carId}
                            </div>
                            <div className="text-[10px] font-mono text-[var(--text-muted)]">
                              {opp.driverName ?? `Grid Slot #${idx + 1}`}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() =>
                              setAiOpponents((prev) =>
                                prev.filter((_, i) => i !== idx)
                              )
                            }
                            aria-label={`Remove opponent ${carInfo?.name ?? opp.carId}`}
                            title="Remove opponent"
                            className="p-1 text-[var(--text-muted)] hover:text-red-400 cursor-pointer"
                          >
                            <X className="w-3.5 h-3.5" />
                          </button>
                        </div>
                        <div className="grid grid-cols-2 gap-2">
                          <Slider
                            label="Skill"
                            value={opp.aiLevel}
                            min={70}
                            max={100}
                            unit="%"
                            onChange={(val) =>
                              setAiOpponents((prev) =>
                                prev.map((item, i) =>
                                  i === idx ? { ...item, aiLevel: val } : item
                                )
                              )
                            }
                          />
                          <Slider
                            label="Aggr"
                            value={opp.aiAggression}
                            min={0}
                            max={100}
                            unit="%"
                            onChange={(val) =>
                              setAiOpponents((prev) =>
                                prev.map((item, i) =>
                                  i === idx
                                    ? { ...item, aiAggression: val }
                                    : item
                                )
                              )
                            }
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </Card>
          )}

          {/* Player Ballast & Restrictor */}
          <Card variant="glass" padding="md" className="space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Gauge className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                  Player BoP & Handicap
                </span>
              </div>
              {(playerBallastKg > 0 || playerRestrictor > 0) && (
                <Badge variant="warning">BoP Active</Badge>
              )}
            </div>
            <Slider
              label="Success Ballast"
              value={playerBallastKg}
              min={0}
              max={150}
              step={5}
              unit=" kg"
              onChange={setPlayerBallastKg}
            />
            <Slider
              label="Air Restrictor"
              value={playerRestrictor}
              min={0}
              max={100}
              step={5}
              unit="%"
              onChange={setPlayerRestrictor}
            />
          </Card>

          {/* Launch Session CTA Panel */}
          <Card
            variant="elevated"
            padding="md"
            className="space-y-3 border-[var(--accent-border)] relative overflow-hidden"
          >
            <div
              className="absolute inset-x-0 top-0 h-[2px]"
              style={{
                background:
                  "linear-gradient(90deg, transparent, var(--accent), transparent)",
              }}
            />
            <div className="flex items-center justify-between text-xs">
              <span className="text-[var(--text-secondary)]">Ready to grid</span>
              <span className="font-mono text-[var(--text-primary)] tabular-nums">
                {formatMinutes(timeOfDayMinutes)} · {trackGripPreset} ·{" "}
                {aiOpponents.length + 1} cars
              </span>
            </div>
            <Button
              variant="primary"
              size="lg"
              loading={launching}
              disabled={gameRunning}
              icon={<Play className="w-4 h-4 fill-current" />}
              onClick={handleLaunch}
              className="w-full font-display text-lg uppercase tracking-wider h-12"
            >
              {gameRunning ? "Assetto Corsa Running" : "Launch Session"}
            </Button>
          </Card>
        </div>
      </div>

      {/* ================================================================= */}
      {/* Visual Searchable Car / Track / Opponent Picker Modal             */}
      {/* ================================================================= */}
      {pickerMode && (
        <div
          className="fixed inset-0 z-50 bg-black/75 backdrop-blur-md flex items-center justify-center p-6 animate-fade-in"
          onClick={() => setPickerMode(null)}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={
              pickerMode === "track"
                ? "Select Circuit"
                : pickerMode === "opponent"
                  ? "Add AI Opponent"
                  : "Select Machine"
            }
            onClick={(e) => e.stopPropagation()}
            className="glass-panel rounded-2xl w-full max-w-[min(1100px,calc(100vw-2rem))] max-h-[82vh] flex flex-col overflow-hidden border border-white/[0.14] animate-scale-in"
          >
            <div className="px-5 py-4 border-b border-white/[0.08] flex items-center justify-between gap-4 shrink-0 min-w-0">
              <div className="flex items-center gap-2.5">
                {pickerMode === "track" ? (
                  <Compass className="w-5 h-5 text-cyan-400" />
                ) : (
                  <Car className="w-5 h-5 text-[var(--accent)]" />
                )}
                <div>
                  <h2 className="font-display text-xl font-bold uppercase tracking-wider text-white leading-none">
                    {pickerMode === "track"
                      ? "Select Circuit"
                      : pickerMode === "opponent"
                      ? "Add AI Opponent"
                      : "Select Machine"}
                  </h2>
                  <p className="text-[11px] text-[var(--text-muted)] mt-0.5">
                    {pickerMode === "track"
                      ? `${filteredPickerTracks.length} circuits available`
                      : `${filteredPickerCars.length} cars indexed`}
                  </p>
                </div>
              </div>
              <div className="flex items-center gap-2">
                {pickerMode === "track" && (
                  <div className="flex items-center p-0.5 rounded-lg bg-black/50 border border-white/[0.1]">
                    <button
                      type="button"
                      onClick={() => setPickerTrackMode("2d")}
                      className={`px-2.5 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
                        pickerTrackMode === "2d"
                          ? "bg-white/[0.12] text-white"
                          : "text-[var(--text-secondary)] hover:text-white"
                      }`}
                    >
                      <ImageIcon className="w-3.5 h-3.5" />
                      2D Preview
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        if (!pickerPreviewTrack && armedTrack) {
                          setPickerPreviewTrack(armedTrack);
                          setPickerPreviewLayoutId(
                            armedSession.layoutId || armedTrack.layouts[0]?.id || "default"
                          );
                        }
                        setPickerTrackMode("3d");
                      }}
                      className={`px-2.5 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
                        pickerTrackMode === "3d"
                          ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
                          : "text-[var(--text-secondary)] hover:text-white"
                      }`}
                    >
                      <Box className="w-3.5 h-3.5" />
                      3D Preview
                    </button>
                  </div>
                )}
                <button
                  type="button"
                  onClick={() => setPickerMode(null)}
                  aria-label="Close picker"
                  title="Close"
                  className="p-1.5 rounded-lg text-[var(--text-muted)] hover:text-white hover:bg-white/[0.08] cursor-pointer"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>
            </div>

            <div className="px-5 py-3 border-b border-white/[0.06] space-y-2.5 shrink-0">
              <div className="relative">
                <Search className="w-4 h-4 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  autoFocus
                  value={pickerQuery}
                  onChange={(e) => setPickerQuery(e.target.value)}
                  placeholder={
                    pickerMode === "track"
                      ? "Search tracks by name, country, or tag..."
                      : "Search cars by name, brand, class, or tag..."
                  }
                  className="w-full h-9 pl-9 pr-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                />
              </div>
              <div className="flex items-center gap-1.5 overflow-x-auto pb-0.5">
                {pickerBrands.slice(0, 14).map((b) => (
                  <button
                    key={b}
                    type="button"
                    onClick={() => setPickerFilter(b)}
                    className={`px-2.5 py-1 rounded-lg text-[11px] font-medium whitespace-nowrap border transition-all cursor-pointer ${
                      pickerFilter === b
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                        : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                    }`}
                  >
                    {b}
                  </button>
                ))}
              </div>
            </div>

            {pickerMode === "track" && pickerTrackMode === "3d" && pickerPreviewTrack && (
              <div className="px-5 py-3 border-b border-white/[0.08] bg-black/30 shrink-0 grid grid-cols-1 md:grid-cols-12 gap-3 items-center min-w-0">
                <div className="md:col-span-7 min-w-0 h-44 rounded-xl overflow-hidden border border-white/[0.1] relative">
                  <Viewport3D
                    mode="track"
                    trackData={pickerTrackScene}
                    accentHex={accentHex}
                    loading={loadingPickerScene}
                    label={`${pickerPreviewTrack.name.toUpperCase()} · ${pickerPreviewLayoutId.toUpperCase()}`}
                  />
                </div>
                <div className="md:col-span-5 flex flex-col justify-between gap-2.5 min-w-0">
                  <div>
                    <div className="text-[10px] font-mono uppercase tracking-[0.12em] text-cyan-300">
                      {pickerPreviewTrack.country} ·{" "}
                      {pickerPreviewTrack.length ?? "Circuit"}
                    </div>
                    <div className="font-display text-lg font-bold uppercase text-white truncate">
                      {pickerPreviewTrack.name}
                    </div>
                  </div>
                  {pickerPreviewTrack.layouts.length > 0 && (
                    <div className="space-y-1">
                      <div className="text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                        Choose Layout ({pickerPreviewTrack.layouts.length})
                      </div>
                      <div className="flex flex-wrap gap-1.5 max-h-20 overflow-y-auto">
                        {pickerPreviewTrack.layouts.map((lay) => {
                          const activeLay = lay.id === pickerPreviewLayoutId;
                          return (
                            <button
                              key={lay.id}
                              type="button"
                              onClick={() => setPickerPreviewLayoutId(lay.id)}
                              className={`px-2 py-1 rounded-md text-[11px] font-medium border transition-colors cursor-pointer ${
                                activeLay
                                  ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                                  : "bg-white/[0.04] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                              }`}
                            >
                              {lay.name}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  )}
                  <div className="flex items-center gap-2">
                    <Button
                      variant="primary"
                      size="sm"
                      onClick={() => {
                        armTrack(pickerPreviewTrack, pickerPreviewLayoutId);
                        setPickerMode(null);
                      }}
                    >
                      Select Circuit & Layout
                    </Button>
                  </div>
                </div>
              </div>
            )}

            <div className="flex-1 min-h-0 overflow-y-auto p-5">
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3 auto-rows-max">
                {pickerMode === "track"
                  ? filteredPickerTracks.map((t) => {
                      const selected = t.id === armedSession.trackId;
                      const previewing =
                        pickerTrackMode === "3d" && pickerPreviewTrack?.id === t.id;
                      const activeLayoutId =
                        previewing
                          ? pickerPreviewLayoutId
                          : selected
                            ? armedSession.layoutId
                            : t.layouts[0]?.id ?? "default";
                      const activeLayoutObj =
                        t.layouts.find((l) => l.id === activeLayoutId) ?? t.layouts[0];
                      const img = assetUrl(activeLayoutObj?.previewPath ?? t.previewPath);
                      return (
                        <div
                          key={t.id}
                          role="button"
                          tabIndex={0}
                          onClick={() => {
                            armTrack(t, activeLayoutId);
                            setPickerMode(null);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              armTrack(t, activeLayoutId);
                              setPickerMode(null);
                            }
                          }}
                          className={`flex flex-col text-left rounded-xl overflow-hidden border transition-all cursor-pointer group ${
                            previewing || selected
                              ? "border-[var(--accent)] bg-[var(--accent-soft)]"
                              : "border-white/[0.08] bg-white/[0.03] hover:border-white/[0.2]"
                          }`}
                        >
                          <div className="w-full h-24 shrink-0 bg-black/50 relative overflow-hidden">
                            {img ? (
                              <img
                                src={img}
                                alt={t.name}
                                className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                              />
                            ) : (
                              <div className="w-full h-full bg-gradient-to-br from-cyan-500/15 to-transparent flex items-center justify-center">
                                <Flag className="w-6 h-6 text-white/20" />
                              </div>
                            )}
                            <button
                              type="button"
                              title="Preview in 3D"
                              onClick={(e) => {
                                e.stopPropagation();
                                setPickerPreviewTrack(t);
                                setPickerPreviewLayoutId(activeLayoutId);
                                setPickerTrackMode("3d");
                              }}
                              className="absolute top-1.5 right-1.5 px-1.5 py-0.5 rounded bg-black/75 hover:bg-black border border-white/[0.14] text-[10px] font-mono text-cyan-300 flex items-center gap-1 cursor-pointer"
                            >
                              <Box className="w-2.5 h-2.5" />
                              3D
                            </button>
                            <div className="absolute bottom-1.5 right-1.5 px-1.5 py-0.5 rounded bg-black/70 text-[10px] font-mono text-white/85">
                              {t.length ?? "Circuit"}
                            </div>
                          </div>
                          <div className="p-2.5 shrink-0 space-y-1.5">
                            <div>
                              <div className="text-xs font-semibold text-[var(--text-primary)] truncate">
                                {t.name}
                              </div>
                              <div className="text-[11px] text-[var(--text-muted)]">
                                {t.country} · {t.layouts.length} layout
                                {t.layouts.length === 1 ? "" : "s"}
                              </div>
                            </div>
                            {t.layouts.length > 1 && (
                              <div
                                className="flex flex-wrap gap-1 pt-0.5"
                                onClick={(e) => e.stopPropagation()}
                              >
                                {t.layouts.slice(0, 4).map((lay) => {
                                  const laySelected = selected && lay.id === armedSession.layoutId;
                                  return (
                                    <button
                                      key={lay.id}
                                      type="button"
                                      onClick={() => {
                                        armTrack(t, lay.id);
                                        setPickerMode(null);
                                      }}
                                      className={`px-1.5 py-0.5 rounded text-[10px] font-mono border transition-colors cursor-pointer ${
                                        laySelected
                                          ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                                          : "bg-white/[0.04] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                                      }`}
                                    >
                                      {lay.name}
                                    </button>
                                  );
                                })}
                                {t.layouts.length > 4 && (
                                  <button
                                    type="button"
                                    onClick={() => {
                                      setPickerPreviewTrack(t);
                                      setPickerPreviewLayoutId(activeLayoutId);
                                      setPickerTrackMode("3d");
                                    }}
                                    className="px-1.5 py-0.5 rounded text-[10px] font-mono text-cyan-300 bg-cyan-500/10 border border-cyan-500/25 cursor-pointer"
                                  >
                                    +{t.layouts.length - 4}
                                  </button>
                                )}
                              </div>
                            )}
                          </div>
                        </div>
                      );
                    })
                  : filteredPickerCars.map((c) => {
                      const img = assetUrl(c.previewPath);
                      const selected =
                        pickerMode === "car" && c.id === armedSession.carId;
                      return (
                        <button
                          key={c.id}
                          type="button"
                          onClick={() => {
                            if (pickerMode === "opponent") {
                              handleAddOpponentCar(c);
                              notify("Opponent added", "info", c.name);
                            } else {
                              armCar(c);
                              setPickerMode(null);
                            }
                          }}
                          className={`flex flex-col w-full text-left rounded-xl overflow-hidden border transition-all cursor-pointer group ${
                            selected
                              ? "border-[var(--accent)] bg-[var(--accent-soft)]"
                              : "border-white/[0.08] bg-white/[0.03] hover:border-white/[0.2]"
                          }`}
                        >
                          <div className="w-full h-24 shrink-0 bg-black/50 relative overflow-hidden">
                            {img ? (
                              <img
                                src={img}
                                alt={c.name}
                                loading="lazy"
                                className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                              />
                            ) : (
                              <div className="w-full h-full bg-gradient-to-br from-[var(--accent)]/15 to-transparent flex items-center justify-center">
                                <Car className="w-6 h-6 text-white/20" />
                              </div>
                            )}
                            <div className="absolute bottom-1.5 right-1.5 px-1.5 py-0.5 rounded bg-black/70 text-[10px] font-mono text-white/85">
                              {c.bhp ?? c.class}
                            </div>
                          </div>
                          <div className="p-2.5 w-full shrink-0">
                            <div className="text-[10px] font-mono uppercase text-[var(--accent)]">
                              {c.brand}
                            </div>
                            <div className="text-xs font-semibold text-[var(--text-primary)] truncate">
                              {c.name}
                            </div>
                          </div>
                        </button>
                      );
                    })}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default DrivePage;

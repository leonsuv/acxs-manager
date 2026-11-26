import { useEffect, useMemo, useState, useCallback } from "react";
import {
  Box,
  ChevronLeft,
  ChevronRight,
  FolderOpen,
  Heart,
  Image as ImageIcon,
  Play,
  Sparkles,
  Gauge,
  Weight,
  Zap,
  Flame,
  Palette,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import { ACCENT_PRESETS } from "../lib/types";
import type { CarEntry, Scene3DMeshData, SkinEntry } from "../lib/types";
import { useAppStore } from "../store/appStore";
import { Badge, Button, SegmentedControl, Skeleton } from "../components/Primitives";
import { Viewport3D } from "../components/Viewport3D";

type StageMode = "3d" | "2d";

const FAVORITES_KEY = "acxs_favorite_cars_v1";

function loadFavoriteIds(): Set<string> {
  try {
    const raw = localStorage.getItem(FAVORITES_KEY);
    if (!raw) return new Set();
    return new Set(JSON.parse(raw) as string[]);
  } catch {
    return new Set();
  }
}

function saveFavoriteIds(ids: Set<string>) {
  try {
    localStorage.setItem(FAVORITES_KEY, JSON.stringify(Array.from(ids)));
  } catch {
    // ignore storage errors
  }
}

function parseNumeric(val?: string | null): number {
  if (!val) return 0;
  const m = val.replace(/,/g, "").match(/[\d.]+/);
  return m ? parseFloat(m[0]) : 0;
}

function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, v));
}

function computeRadarScores(car: CarEntry) {
  const bhp = parseNumeric(car.bhp) || 420;
  const torque = parseNumeric(car.torque) || 440;
  const weight = parseNumeric(car.weight) || 1300;
  const topSpeed = parseNumeric(car.topSpeed) || 285;
  const pw = parseNumeric(car.pwRatio) || (weight > 0 && bhp > 0 ? weight / bhp : 3.0);

  const isRace =
    car.class.toLowerCase().includes("race") ||
    car.tags.some((t) => /gt3|gt2|lmp|groupc|f1|dtm|race/i.test(t));

  return [
    { label: "PWR", full: "Power", score: clamp((bhp / 950) * 100, 18, 100) },
    { label: "TRQ", full: "Torque", score: clamp((torque / 900) * 100, 18, 100) },
    { label: "SPD", full: "Top Speed", score: clamp(((topSpeed - 160) / 210) * 100, 20, 100) },
    { label: "LGT", full: "Lightness", score: clamp(((2000 - weight) / 1250) * 100, 20, 100) },
    { label: "P/W", full: "Power/Weight", score: clamp(((5.2 - pw) / 4.2) * 100, 18, 100) },
    { label: "AERO", full: "Aero Grip", score: isRace ? clamp(76 + (bhp / 1000) * 20, 72, 98) : clamp(46 + (topSpeed / 360) * 28, 42, 78) },
  ];
}

function PerformanceRadar({
  car,
  accentHex,
}: {
  car: CarEntry;
  accentHex: string;
}) {
  const axes = useMemo(() => computeRadarScores(car), [car]);
  const cx = 100;
  const cy = 96;
  const radius = 66;
  const levels = [0.25, 0.5, 0.75, 1];

  const pointAt = (idx: number, frac: number) => {
    const angle = (Math.PI * 2 * idx) / axes.length - Math.PI / 2;
    return {
      x: cx + Math.cos(angle) * radius * frac,
      y: cy + Math.sin(angle) * radius * frac,
    };
  };

  const dataPoints = axes
    .map((a, i) => {
      const p = pointAt(i, a.score / 100);
      return `${p.x.toFixed(1)},${p.y.toFixed(1)}`;
    })
    .join(" ");

  const avgScore = Math.round(
    axes.reduce((acc, a) => acc + a.score, 0) / axes.length
  );

  return (
    <div className="relative flex flex-col items-center">
      <svg viewBox="0 0 200 192" className="w-full max-w-[200px] h-auto overflow-visible">
        <defs>
          <radialGradient id="acxs-radar-fill" cx="50%" cy="50%" r="55%">
            <stop offset="0%" stopColor={accentHex} stopOpacity="0.48" />
            <stop offset="100%" stopColor={accentHex} stopOpacity="0.12" />
          </radialGradient>
        </defs>

        {/* Concentric hexagons */}
        {levels.map((lvl) => {
          const pts = axes
            .map((_, i) => {
              const p = pointAt(i, lvl);
              return `${p.x.toFixed(1)},${p.y.toFixed(1)}`;
            })
            .join(" ");
          return (
            <polygon
              key={lvl}
              points={pts}
              fill="none"
              stroke="rgba(255,255,255,0.08)"
              strokeWidth={lvl === 1 ? "1.1" : "0.7"}
            />
          );
        })}

        {/* Radial spokes */}
        {axes.map((a, i) => {
          const outer = pointAt(i, 1);
          const lbl = pointAt(i, 1.24);
          return (
            <g key={a.label}>
              <line
                x1={cx}
                y1={cy}
                x2={outer.x}
                y2={outer.y}
                stroke="rgba(255,255,255,0.08)"
                strokeWidth="0.7"
              />
              <text
                x={lbl.x}
                y={lbl.y}
                textAnchor="middle"
                dominantBaseline="middle"
                fill="rgba(152,164,184,0.9)"
                fontSize="8.5"
                fontFamily="JetBrains Mono, monospace"
                fontWeight="600"
              >
                {a.label}
              </text>
            </g>
          );
        })}

        {/* Data polygon */}
        <polygon
          points={dataPoints}
          fill="url(#acxs-radar-fill)"
          stroke={accentHex}
          strokeWidth="1.8"
          strokeLinejoin="round"
        />

        {/* Vertex dots */}
        {axes.map((a, i) => {
          const p = pointAt(i, a.score / 100);
          return (
            <circle
              key={a.label}
              cx={p.x}
              cy={p.y}
              r="2.6"
              fill="#F8FAFC"
              stroke={accentHex}
              strokeWidth="1.4"
            />
          );
        })}

        {/* Center PI Score */}
        <circle cx={cx} cy={cy} r="14" fill="rgba(7,9,13,0.82)" stroke="rgba(255,255,255,0.1)" />
        <text
          x={cx}
          y={cy + 0.5}
          textAnchor="middle"
          dominantBaseline="middle"
          fill="#F3F6FA"
          fontSize="10"
          fontFamily="JetBrains Mono, monospace"
          fontWeight="700"
        >
          {avgScore}
        </text>
      </svg>
    </div>
  );
}

function SpecBar({
  label,
  value,
  pct,
  icon,
}: {
  label: string;
  value: string;
  pct: number;
  icon: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-[11px]">
        <span className="flex items-center gap-1.5 text-[var(--text-secondary)]">
          <span className="text-[var(--text-muted)]">{icon}</span>
          {label}
        </span>
        <span className="font-mono font-semibold text-[var(--text-primary)] tabular-nums">
          {value}
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-white/[0.07] overflow-hidden">
        <div
          className="h-full rounded-full transition-all duration-500 ease-out"
          style={{
            width: `${clamp(pct, 6, 100)}%`,
            background:
              "linear-gradient(90deg, rgba(var(--accent-rgb), 0.55) 0%, var(--accent) 100%)",
            boxShadow: "0 0 10px var(--accent-glow)",
          }}
        />
      </div>
    </div>
  );
}

export function ShowroomPage() {
  const { cars, armedSession, accent, armCar, navigate, notify } = useAppStore();

  const accentHex = useMemo(
    () => ACCENT_PRESETS.find((p) => p.id === accent)?.hex ?? "#FF4D2E",
    [accent]
  );

  const [selectedIndex, setSelectedIndex] = useState<number>(() => {
    const idx = cars.findIndex((c) => c.id === armedSession.carId);
    return idx >= 0 ? idx : 0;
  });
  const [stageMode, setStageMode] = useState<StageMode>("3d");
  const [skins, setSkins] = useState<SkinEntry[]>([]);
  const [activeSkinId, setActiveSkinId] = useState<string>(armedSession.skinId || "");
  const [meshData, setMeshData] = useState<Scene3DMeshData | null>(null);
  const [loadingMesh, setLoadingMesh] = useState(false);
  const [favorites, setFavorites] = useState<Set<string>>(() => loadFavoriteIds());

  // Sync selected index if armed car changes externally
  useEffect(() => {
    if (cars.length === 0) return;
    const idx = cars.findIndex((c) => c.id === armedSession.carId);
    if (idx >= 0) {
      setSelectedIndex(idx);
    }
  }, [armedSession.carId, cars]);

  const currentCar: CarEntry | undefined = cars[selectedIndex] ?? cars[0];

  const stepCar = useCallback(
    (delta: number) => {
      if (cars.length === 0) return;
      setSelectedIndex((prev) => (prev + delta + cars.length) % cars.length);
    },
    [cars.length]
  );

  const toggleFavorite = useCallback(
    (carId: string) => {
      setFavorites((prev) => {
        const next = new Set(prev);
        if (next.has(carId)) {
          next.delete(carId);
          notify("Removed from favorites", "info");
        } else {
          next.add(carId);
          notify("Added to favorites", "success");
        }
        saveFavoriteIds(next);
        return next;
      });
    },
    [notify]
  );

  // Keyboard shortcuts: Left / Right / F / 2 / 3
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable)
      ) {
        return;
      }
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        stepCar(-1);
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        stepCar(1);
      } else if (e.key.toLowerCase() === "f" && currentCar) {
        toggleFavorite(currentCar.id);
      } else if (e.key === "3") {
        setStageMode("3d");
      } else if (e.key === "2") {
        setStageMode("2d");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [stepCar, toggleFavorite, currentCar]);

  // Load skins for current car
  useEffect(() => {
    if (!currentCar) return;
    let cancelled = false;
    api
      .getCarSkins(currentCar.path)
      .then((list) => {
        if (cancelled) return;
        const resolved = list ?? [];
        setSkins(resolved);
        if (resolved.length > 0) {
          setActiveSkinId((prev) =>
            resolved.some((s) => s.id === prev) ? prev : resolved[0].id
          );
        } else {
          setActiveSkinId("");
        }
      })
      .catch(() => {
        if (!cancelled) setSkins([]);
      });
    return () => {
      cancelled = true;
    };
  }, [currentCar]);

  // Load real KN5 3D scene when in 3D mode
  useEffect(() => {
    if (!currentCar || stageMode !== "3d") return;
    let cancelled = false;
    setLoadingMesh(true);
    api
      .loadCar3DScene(currentCar.path, activeSkinId)
      .then((data) => {
        if (!cancelled) setMeshData(data ?? null);
      })
      .catch(() => {
        if (!cancelled) setMeshData(null);
      })
      .finally(() => {
        if (!cancelled) setLoadingMesh(false);
      });
    return () => {
      cancelled = true;
    };
  }, [currentCar, activeSkinId, stageMode]);

  if (!currentCar) {
    return (
      <div className="h-full flex items-center justify-center p-8">
        <div className="glass-panel rounded-2xl p-10 text-center max-w-md space-y-3">
          <Sparkles className="w-8 h-8 text-[var(--accent)] mx-auto" />
          <h2 className="font-display text-2xl font-bold uppercase tracking-wider">
            Showroom Empty
          </h2>
          <p className="text-xs text-[var(--text-secondary)]">
            No vehicles indexed yet. Configure your Assetto Corsa root folder in Setup to load your garage.
          </p>
          <Button variant="primary" onClick={() => navigate("setup", "app")}>
            Configure AC Root
          </Button>
        </div>
      </div>
    );
  }

  const activeSkin = skins.find((s) => s.id === activeSkinId) ?? skins[0];
  const previewSrc = assetUrl(activeSkin?.previewPath ?? currentCar.previewPath);
  const isFav = favorites.has(currentCar.id);

  const bhpVal = parseNumeric(currentCar.bhp);
  const torqueVal = parseNumeric(currentCar.torque);
  const weightVal = parseNumeric(currentCar.weight);
  const pwVal = parseNumeric(currentCar.pwRatio);

  const handleRaceThisCar = () => {
    armCar(currentCar, activeSkinId || undefined);
    notify(
      "Car Armed for Session",
      "success",
      `${currentCar.name}${activeSkin ? ` · ${activeSkin.name}` : ""}`
    );
    navigate("drive", "quick-race");
  };

  const handleOpenFolder = async () => {
    try {
      await api.openPathInExplorer(currentCar.path);
    } catch (err) {
      notify("Could not open folder", "error", String(err));
    }
  };

  return (
    <div className="h-full flex flex-col gap-3 p-4 overflow-hidden animate-fade-in">
      {/* Main Full-Bleed Showcase Stage */}
      <div className="relative flex-1 min-h-0 rounded-2xl overflow-hidden border border-white/[0.08] bg-[#06080D] shadow-2xl">
        {/* Stage Canvas: 3D or 2D */}
        {stageMode === "3d" ? (
          <div className="w-full h-full relative">
            <Viewport3D
              mode="car"
              meshData={meshData}
              accentHex={accentHex}
              label={`${currentCar.brand.toUpperCase()} · ${currentCar.name.toUpperCase()}`}
              className="rounded-none border-0"
            />
            {loadingMesh && (
              <div className="absolute bottom-3 right-3 px-3 py-1.5 rounded-lg bg-black/70 backdrop-blur-md border border-white/[0.1] text-[11px] font-mono text-[var(--text-secondary)] flex items-center gap-2">
                <span className="w-2 h-2 rounded-full bg-[var(--accent)] animate-pulse" />
                Loading KN5 geometry...
              </div>
            )}
          </div>
        ) : (
          <div className="w-full h-full relative flex items-center justify-center overflow-hidden bg-[radial-gradient(ellipse_at_50%_40%,rgba(var(--accent-rgb),0.16),rgba(7,9,13,0.98)_70%)]">
            {/* Subtle studio floor reflection line */}
            <div className="absolute inset-x-0 bottom-[22%] h-px bg-gradient-to-r from-transparent via-white/15 to-transparent" />
            {previewSrc ? (
              <img
                src={previewSrc}
                alt={currentCar.name}
                className="max-h-[82%] max-w-[76%] object-contain drop-shadow-[0_32px_48px_rgba(0,0,0,0.85)] transition-all duration-300"
              />
            ) : (
              <div className="flex flex-col items-center gap-3 text-center px-8">
                <div className="w-20 h-20 rounded-2xl bg-white/[0.04] border border-white/[0.1] flex items-center justify-center">
                  <ImageIcon className="w-8 h-8 text-[var(--text-muted)]" />
                </div>
                <div className="font-display text-4xl font-bold uppercase tracking-widest text-white/85">
                  {currentCar.brand} {currentCar.name}
                </div>
                <p className="text-xs font-mono text-[var(--text-muted)]">
                  2D preview image not found on disk — switch to 3D Showroom for live WebGL inspection
                </p>
              </div>
            )}
          </div>
        )}

        {/* Top Floating Controls Bar */}
        <div className="absolute top-3.5 left-1/2 -translate-x-1/2 z-20 flex items-center gap-2.5 px-2 py-1.5 rounded-2xl bg-black/65 backdrop-blur-xl border border-white/[0.1] shadow-xl">
          <button
            type="button"
            onClick={() => stepCar(-1)}
            title="Previous Car (←)"
            className="h-8 w-8 rounded-lg flex items-center justify-center text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.08] transition-colors cursor-pointer"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>

          <span className="font-mono text-[11px] text-[var(--text-secondary)] tabular-nums px-1">
            {String(selectedIndex + 1).padStart(2, "0")} / {String(cars.length).padStart(2, "0")}
          </span>

          <button
            type="button"
            onClick={() => stepCar(1)}
            title="Next Car (→)"
            className="h-8 w-8 rounded-lg flex items-center justify-center text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.08] transition-colors cursor-pointer"
          >
            <ChevronRight className="w-4 h-4" />
          </button>

          <div className="w-px h-5 bg-white/[0.1]" />

          <SegmentedControl
            size="sm"
            value={stageMode}
            onChange={setStageMode}
            options={[
              { value: "3d", label: "3D Showroom", icon: <Box className="w-3.5 h-3.5" /> },
              { value: "2d", label: "2D Studio", icon: <ImageIcon className="w-3.5 h-3.5" /> },
            ]}
          />

          <div className="w-px h-5 bg-white/[0.1]" />

          <button
            type="button"
            onClick={() => toggleFavorite(currentCar.id)}
            title="Toggle Favorite (F)"
            className={`h-8 w-8 rounded-lg flex items-center justify-center transition-colors cursor-pointer ${
              isFav
                ? "bg-red-500/20 text-red-400 border border-red-500/35"
                : "text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.08]"
            }`}
          >
            <Heart className={`w-4 h-4 ${isFav ? "fill-current" : ""}`} />
          </button>

          <button
            type="button"
            onClick={handleOpenFolder}
            title="Open Car Folder"
            className="h-8 w-8 rounded-lg flex items-center justify-center text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.08] transition-colors cursor-pointer"
          >
            <FolderOpen className="w-4 h-4" />
          </button>

          <Button
            variant="primary"
            size="sm"
            icon={<Play className="w-3.5 h-3.5 fill-current" />}
            onClick={handleRaceThisCar}
          >
            Race This Car
          </Button>
        </div>

        {/* Left/Right Stage Prev/Next Chevrons */}
        <button
          type="button"
          onClick={() => stepCar(-1)}
          aria-label="Previous car"
          className="absolute left-3.5 top-1/2 -translate-y-1/2 z-20 h-11 w-11 rounded-full bg-black/50 hover:bg-black/75 backdrop-blur-md border border-white/[0.1] flex items-center justify-center text-white/75 hover:text-white transition-all cursor-pointer"
        >
          <ChevronLeft className="w-5 h-5" />
        </button>

        {/* Floating Glass Spec Overlay Card (Right) */}
        <div className="absolute right-4 top-16 bottom-4 z-20 w-[300px] glass-panel rounded-2xl p-4 flex flex-col justify-between gap-3 overflow-y-auto pointer-events-auto">
          <div className="space-y-3">
            {/* Identity Header */}
            <div>
              <div className="flex items-center justify-between gap-2 mb-1">
                <span className="text-[11px] font-mono uppercase tracking-[0.14em] text-[var(--accent)] font-semibold">
                  {currentCar.brand}
                </span>
                <div className="flex items-center gap-1.5">
                  {currentCar.year && <Badge variant="neutral">{currentCar.year}</Badge>}
                  <Badge variant="accent" className="uppercase">
                    {currentCar.class}
                  </Badge>
                </div>
              </div>
              <h2 className="font-display text-2xl font-bold uppercase tracking-wide text-[var(--text-primary)] leading-tight">
                {currentCar.name}
              </h2>
              {currentCar.country && (
                <p className="text-[11px] text-[var(--text-muted)] mt-0.5">
                  {currentCar.country} · {currentCar.skinsCount ?? skins.length} Liveries
                </p>
              )}
            </div>

            {/* 6-Axis Performance Radar */}
            <div className="rounded-xl bg-black/35 border border-white/[0.06] py-2 px-3">
              <div className="flex items-center justify-between text-[10px] font-mono uppercase tracking-wider text-[var(--text-muted)] mb-1">
                <span>Performance Index</span>
                <span className="text-[var(--accent)]">6-Axis Telemetry</span>
              </div>
              <PerformanceRadar car={currentCar} accentHex={accentHex} />
            </div>

            {/* Animated Spec Bars */}
            <div className="space-y-2.5">
              <SpecBar
                label="Power"
                value={currentCar.bhp || "—"}
                pct={bhpVal ? (bhpVal / 1000) * 100 : 45}
                icon={<Flame className="w-3 h-3" />}
              />
              <SpecBar
                label="Torque"
                value={currentCar.torque || "—"}
                pct={torqueVal ? (torqueVal / 950) * 100 : 45}
                icon={<Zap className="w-3 h-3" />}
              />
              <SpecBar
                label="Curb Weight"
                value={currentCar.weight || "—"}
                pct={weightVal ? ((2100 - weightVal) / 1300) * 100 : 50}
                icon={<Weight className="w-3 h-3" />}
              />
              <SpecBar
                label="Power / Weight"
                value={currentCar.pwRatio || "—"}
                pct={pwVal ? ((5.5 - pwVal) / 4.5) * 100 : 55}
                icon={<Gauge className="w-3 h-3" />}
              />
            </div>
          </div>

          {/* Tags & Keyboard Hint */}
          <div className="space-y-2.5 pt-2 border-t border-white/[0.06]">
            {currentCar.tags.length > 0 && (
              <div className="flex flex-wrap gap-1">
                {currentCar.tags.slice(0, 6).map((tag) => (
                  <span
                    key={tag}
                    className="px-2 py-0.5 rounded-md bg-white/[0.04] border border-white/[0.07] text-[10px] font-mono text-[var(--text-secondary)]"
                  >
                    {tag}
                  </span>
                ))}
              </div>
            )}
            <div className="flex items-center justify-between text-[10px] font-mono text-[var(--text-muted)]">
              <span>←/→ Switch Car</span>
              <span>2/3 Stage · F Fav</span>
            </div>
          </div>
        </div>
      </div>

      {/* Bottom Horizontal Skin / Livery Filmstrip */}
      <div className="h-[96px] shrink-0 glass-panel rounded-2xl px-3.5 py-2.5 flex items-center gap-3">
        <div className="shrink-0 pr-3 border-r border-white/[0.07] flex flex-col justify-center min-w-[118px]">
          <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-wider text-[var(--accent)]">
            <Palette className="w-3.5 h-3.5" />
            <span>Liveries</span>
          </div>
          <div className="text-xs text-[var(--text-secondary)] mt-0.5 truncate max-w-[125px]">
            {activeSkin?.name ?? "Default"}
          </div>
          <div className="text-[10px] font-mono text-[var(--text-muted)]">
            {skins.length} available
          </div>
        </div>

        <div className="flex-1 flex items-center gap-2.5 overflow-x-auto pb-0.5">
          {skins.length === 0 ? (
            <div className="flex items-center gap-2">
              <Skeleton className="h-16 w-36 rounded-xl" />
              <Skeleton className="h-16 w-36 rounded-xl" />
              <Skeleton className="h-16 w-36 rounded-xl" />
            </div>
          ) : (
            skins.map((skin, idx) => {
              const selected = skin.id === activeSkinId;
              const thumb = assetUrl(skin.previewPath ?? skin.liveryPath);
              const hue = (idx * 57 + 12) % 360;
              return (
                <button
                  key={skin.id}
                  type="button"
                  onClick={() => {
                    setActiveSkinId(skin.id);
                    armCar(currentCar, skin.id);
                  }}
                  className={`group relative h-[72px] w-[168px] shrink-0 rounded-xl overflow-hidden border text-left transition-all duration-150 cursor-pointer ${
                    selected
                      ? "border-[var(--accent)] ring-1 ring-[var(--accent)] bg-white/[0.06]"
                      : "border-white/[0.08] bg-white/[0.02] hover:border-white/[0.2]"
                  }`}
                >
                  {thumb ? (
                    <img
                      src={thumb}
                      alt={skin.name}
                      className="absolute inset-0 w-full h-full object-cover opacity-70 group-hover:opacity-90 transition-opacity"
                    />
                  ) : (
                    <div
                      className="absolute inset-0 opacity-35"
                      style={{
                        background: `linear-gradient(135deg, hsl(${hue}, 65%, 24%) 0%, #090D14 75%)`,
                      }}
                    />
                  )}
                  <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/30 to-transparent" />
                  <div className="relative z-10 h-full p-2 flex flex-col justify-between">
                    <div className="flex items-center justify-between">
                      {skin.number ? (
                        <span className="px-1.5 py-0.5 rounded bg-black/65 border border-white/[0.12] font-mono text-[10px] font-bold text-white">
                          #{skin.number}
                        </span>
                      ) : (
                        <span />
                      )}
                      {selected && (
                        <span className="w-2 h-2 rounded-full bg-[var(--accent)] shadow-[0_0_8px_var(--accent)]" />
                      )}
                    </div>
                    <div>
                      <div className="text-[11px] font-semibold text-white truncate">
                        {skin.name}
                      </div>
                      {skin.team && (
                        <div className="text-[10px] text-white/60 truncate">
                          {skin.team}
                        </div>
                      )}
                    </div>
                  </div>
                </button>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

export default ShowroomPage;

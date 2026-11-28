import React, { useMemo, useState } from "react";
import {
  Car,
  ChevronRight,
  Compass,
  FolderOpen,
  Gauge,
  Globe,
  Layers,
  Play,
  RefreshCw,
  Settings,
  Shuffle,
  Sparkles,
  Timer,
  Trophy,
  Wrench,
  Activity,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { assetUrl, isTauri } from "../lib/api";
import type { Pillar, SessionMode, SubTab } from "../lib/types";
import { useAppStore } from "../store/appStore";
import { Badge, Button, Card, KpiTile } from "../components/Primitives";

const SURPRISE_MODES: SessionMode[] = [
  "QuickRace",
  "TrackDay",
  "Hotlap",
  "Weekend",
  "TimeAttack",
  "Drift",
];

function formatNumber(n: number | undefined | null, decimals = 0): string {
  const safe = typeof n === "number" && Number.isFinite(n) ? n : 0;
  return safe.toLocaleString("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

export function PaddockPage() {
  const {
    armedSession,
    cars,
    tracks,
    drivingStats,
    settings,
    cspBuild,
    gameRunning,
    launching,
    scanning,
    navigate,
    armCar,
    armTrack,
    setArmedSession,
    updateSettings,
    rescan,
    launchArmed,
    notify,
  } = useAppStore();

  const [acPathInput, setAcPathInput] = useState(settings.acRoot || "");
  const [savingPath, setSavingPath] = useState(false);

  const armedCar = useMemo(
    () => cars.find((c) => c.id === armedSession.carId) ?? cars[0] ?? null,
    [cars, armedSession.carId]
  );

  const armedTrack = useMemo(
    () => tracks.find((t) => t.id === armedSession.trackId) ?? tracks[0] ?? null,
    [tracks, armedSession.trackId]
  );

  const heroImageUrl = assetUrl(
    armedCar?.previewPath ?? armedSession.carPreviewPath
  );
  const trackImageUrl = assetUrl(
    armedTrack?.previewPath ?? armedSession.trackPreviewPath
  );

  const handleSurpriseMe = () => {
    if (cars.length === 0 || tracks.length === 0) {
      notify("No cars or tracks indexed yet", "warning");
      return;
    }
    const randomCar = cars[Math.floor(Math.random() * cars.length)];
    const randomTrack = tracks[Math.floor(Math.random() * tracks.length)];
    const randomLayout =
      randomTrack.layouts.length > 0
        ? randomTrack.layouts[
            Math.floor(Math.random() * randomTrack.layouts.length)
          ].id
        : "default";
    const randomMode =
      SURPRISE_MODES[Math.floor(Math.random() * SURPRISE_MODES.length)];

    armCar(randomCar);
    armTrack(randomTrack, randomLayout);
    setArmedSession({ mode: randomMode });
    notify(
      "Random combo armed",
      "info",
      `${randomCar.name} @ ${randomTrack.name} (${randomMode})`
    );
  };

  const handleBrowseAcFolder = async () => {
    if (!isTauri()) {
      notify(
        "Folder picker requires desktop app",
        "info",
        "Type a path directly in browser preview mode."
      );
      return;
    }
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Assetto Corsa Installation Folder",
      });
      if (typeof selected === "string" && selected.trim()) {
        setAcPathInput(selected);
        setSavingPath(true);
        await updateSettings({ acRoot: selected });
        await rescan(false);
        setSavingPath(false);
      }
    } catch (err) {
      setSavingPath(false);
      notify("Could not open folder dialog", "error", String(err));
    }
  };

  const handleSaveAcPath = async () => {
    const trimmed = acPathInput.trim();
    if (!trimmed) {
      notify("Enter your Assetto Corsa folder path first", "warning");
      return;
    }
    setSavingPath(true);
    try {
      await updateSettings({ acRoot: trimmed });
      await rescan(false);
    } finally {
      setSavingPath(false);
    }
  };

  const quickJumps: Array<{
    title: string;
    subtitle: string;
    meta: string;
    pillar: Pillar;
    subTab: SubTab;
    icon: React.ReactNode;
    accentColor: string;
  }> = [
    {
      title: "Drive Cockpit",
      subtitle: "Solar dial, WeatherFX, Adaptive AI grid & presets",
      meta: armedSession.mode,
      pillar: "drive",
      subTab: "quick-race",
      icon: <Gauge className="w-5 h-5" />,
      accentColor: "from-[var(--accent)]/20 to-transparent",
    },
    {
      title: "Online Servers",
      subtitle: "Live directory, slot inspector, booking & watchlist",
      meta: "Live Lobby",
      pillar: "online",
      subTab: "servers",
      icon: <Globe className="w-5 h-5" />,
      accentColor: "from-cyan-500/20 to-transparent",
    },
    {
      title: "3D Showroom",
      subtitle: "Inspect liveries, aero & studio lighting in WebGL",
      meta: `${cars.length} Cars`,
      pillar: "garage",
      subTab: "showroom",
      icon: <Compass className="w-5 h-5" />,
      accentColor: "from-amber-500/20 to-transparent",
    },
    {
      title: "Telemetry & Pit-Wall",
      subtitle: "Corner coach, lap traces, live UDP & race strategy",
      meta: `${drivingStats.totalLaps} Laps`,
      pillar: "telemetry",
      subTab: "sessions",
      icon: <Activity className="w-5 h-5" />,
      accentColor: "from-emerald-500/20 to-transparent",
    },
    {
      title: "Championships",
      subtitle: "Multi-round seasons, scoring presets & standings",
      meta: "Season Mode",
      pillar: "drive",
      subTab: "championships",
      icon: <Trophy className="w-5 h-5" />,
      accentColor: "from-violet-500/20 to-transparent",
    },
    {
      title: "CSP & Game Setup",
      subtitle: "Custom Shaders Patch, VR/Triple, FFB & snapshots",
      meta: cspBuild ? `CSP ${cspBuild}` : "Configure",
      pillar: "setup",
      subTab: "csp",
      icon: <Wrench className="w-5 h-5" />,
      accentColor: "from-sky-500/20 to-transparent",
    },
  ];

  return (
    <div className="h-full overflow-y-auto px-6 py-6 pb-10 space-y-6 animate-fade-in">
      {/* First-Run AC Folder Onboarding Banner */}
      {!settings.acRoot && (
        <div className="glass-panel rounded-2xl p-4 sm:p-5 border border-[var(--accent-border)] relative overflow-hidden">
          <div
            className="absolute inset-0 pointer-events-none opacity-40"
            style={{
              background:
                "radial-gradient(600px 180px at 0% 50%, rgba(var(--accent-rgb), 0.22), transparent 70%)",
            }}
          />
          <div className="relative flex flex-col lg:flex-row lg:items-center justify-between gap-4">
            <div className="flex items-start gap-3.5">
              <div className="w-10 h-10 rounded-xl bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center text-[var(--accent)] shrink-0 mt-0.5">
                <FolderOpen className="w-5 h-5" />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <span className="text-[11px] font-mono uppercase tracking-[0.14em] text-[var(--accent)]">
                    First-Run Setup
                  </span>
                  <Badge variant="warning">AC Folder Not Configured</Badge>
                </div>
                <h2 className="text-sm font-semibold text-[var(--text-primary)] mt-1">
                  Connect your Assetto Corsa installation directory
                </h2>
                <p className="text-xs text-[var(--text-secondary)] mt-0.5 max-w-2xl">
                  Point ACXS Manager to your{" "}
                  <span className="font-mono text-[var(--text-primary)]">
                    assettocorsa
                  </span>{" "}
                  root folder to index cars, tracks, skins, replays, and CSP
                  configurations.
                </p>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2 flex-1 lg:max-w-xl">
              <input
                type="text"
                value={acPathInput}
                onChange={(e) => setAcPathInput(e.target.value)}
                placeholder="/path/to/steamapps/common/assettocorsa"
                className="h-9 flex-1 min-w-[260px] max-w-md px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-[var(--accent-border)] outline-none"
              />
              <Button
                variant="secondary"
                size="sm"
                icon={<FolderOpen className="w-3.5 h-3.5" />}
                onClick={handleBrowseAcFolder}
              >
                Browse
              </Button>
              <Button
                variant="primary"
                size="sm"
                loading={savingPath}
                onClick={handleSaveAcPath}
              >
                Save & Scan
              </Button>
              <Button
                variant="ghost"
                size="sm"
                icon={<Settings className="w-3.5 h-3.5" />}
                onClick={() => navigate("setup", "app")}
              >
                Settings
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Cinematic Full-Bleed Hero Showcase */}
      <section className="relative rounded-2xl overflow-hidden border border-white/[0.09] min-h-[370px] flex flex-col justify-between shadow-2xl">
        {/* Background Layer */}
        <div className="absolute inset-0 bg-[#090D14]">
          {heroImageUrl ? (
            <img
              src={heroImageUrl}
              alt={armedCar?.name ?? armedSession.carName}
              className="w-full h-full object-cover object-center scale-[1.03] opacity-75 transition-transform duration-700"
            />
          ) : (
            <div
              className="w-full h-full"
              style={{
                background:
                  "radial-gradient(900px 420px at 78% 35%, rgba(var(--accent-rgb), 0.24), transparent 60%), radial-gradient(640px 360px at 22% 85%, rgba(0, 209, 255, 0.12), transparent 65%), linear-gradient(135deg, #0D131F 0%, #07090E 55%, #121722 100%)",
              }}
            />
          )}
          {/* Layered Cinematic Scrims */}
          <div className="absolute inset-0 bg-gradient-to-r from-[#07090D] via-[#07090D]/80 to-[#07090D]/25" />
          <div className="absolute inset-0 bg-gradient-to-t from-[#07090D] via-transparent to-[#07090D]/40" />
          {/* Subtle Motorsport Grid Overlay */}
          <div
            className="absolute inset-0 opacity-[0.04] pointer-events-none"
            style={{
              backgroundImage:
                "linear-gradient(rgba(255,255,255,0.6) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.6) 1px, transparent 1px)",
              backgroundSize: "48px 48px",
            }}
          />
        </div>

        {/* Top Bar inside Hero */}
        <div className="relative z-10 flex flex-wrap items-center justify-between gap-3 px-7 pt-6">
          <div className="flex items-center gap-2.5">
            <Badge variant="accent" dot>
              ARMED SESSION
            </Badge>
            <Badge variant="neutral">{armedSession.mode}</Badge>
            {cspBuild && <Badge variant="info">CSP {cspBuild}</Badge>}
            {gameRunning && (
              <Badge variant="success" dot>
                AC RUNNING
              </Badge>
            )}
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              loading={scanning}
              icon={<RefreshCw className="w-3.5 h-3.5" />}
              onClick={() => rescan(false)}
            >
              Rescan Library
            </Button>
          </div>
        </div>

        {/* Main Hero Content */}
        <div className="relative z-10 px-7 pb-7 pt-10 flex flex-col xl:flex-row xl:items-end justify-between gap-8">
          <div className="max-w-2xl space-y-4">
            <div className="flex items-center gap-2 text-xs font-mono uppercase tracking-[0.16em] text-[var(--accent)]">
              <span>{armedCar?.brand ?? "Motorsport"}</span>
              {armedCar?.class && (
                <>
                  <span className="text-white/25">·</span>
                  <span className="text-[var(--text-secondary)]">
                    {armedCar.class}
                  </span>
                </>
              )}
              {armedCar?.year && (
                <>
                  <span className="text-white/25">·</span>
                  <span className="text-[var(--text-secondary)]">
                    {armedCar.year}
                  </span>
                </>
              )}
              {armedCar?.country && (
                <>
                  <span className="text-white/25">·</span>
                  <span className="text-[var(--text-muted)]">
                    {armedCar.country}
                  </span>
                </>
              )}
            </div>

            <h1 className="font-display text-4xl sm:text-5xl lg:text-6xl font-bold uppercase tracking-wide text-white leading-[0.95] drop-shadow-sm">
              {armedCar?.name ?? armedSession.carName}
            </h1>

            {/* Tabular Spec Strip */}
            <div className="flex flex-wrap items-center gap-x-6 gap-y-2 pt-1">
              {[
                { label: "POWER", value: armedCar?.bhp ?? "500 bhp" },
                { label: "TORQUE", value: armedCar?.torque ?? "460 Nm" },
                { label: "WEIGHT", value: armedCar?.weight ?? "1220 kg" },
                { label: "TOP SPEED", value: armedCar?.topSpeed ?? "292 km/h" },
                { label: "P/W RATIO", value: armedCar?.pwRatio ?? "2.44 kg/hp" },
              ].map((spec) => (
                <div key={spec.label} className="flex flex-col">
                  <span className="text-[10px] font-mono uppercase tracking-[0.14em] text-[var(--text-muted)]">
                    {spec.label}
                  </span>
                  <span className="font-mono text-sm font-semibold text-[var(--text-primary)] tabular-nums">
                    {spec.value}
                  </span>
                </div>
              ))}
            </div>

            {/* Hero Primary CTA Row */}
            <div className="flex flex-wrap items-center gap-3 pt-2">
              <Button
                variant="primary"
                size="lg"
                loading={launching}
                disabled={gameRunning}
                icon={<Play className="w-4 h-4 fill-current" />}
                onClick={launchArmed}
                className="px-7 tracking-wider uppercase font-display text-base"
              >
                {gameRunning ? "Session Active" : "Drive Now"}
              </Button>
              <Button
                variant="secondary"
                size="lg"
                icon={<Shuffle className="w-4 h-4" />}
                onClick={handleSurpriseMe}
              >
                Surprise Me
              </Button>
              <Button
                variant="secondary"
                size="lg"
                icon={<Sparkles className="w-4 h-4" />}
                onClick={() => navigate("garage", "showroom")}
              >
                Open Showroom
              </Button>
            </div>
          </div>

          {/* Armed Circuit Glass Pill Card */}
          <div
            role="button"
            tabIndex={0}
            aria-label={`Armed circuit: ${armedTrack?.name ?? armedSession.trackName}. Activate to change circuit`}
            onClick={() => navigate("drive", "quick-race")}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                navigate("drive", "quick-race");
              }
            }}
            className="glass-panel rounded-xl p-3.5 w-full xl:w-[360px] shrink-0 cursor-pointer group hover:border-white/[0.18] transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
          >
            <div className="flex items-center gap-3.5">
              <div className="w-20 h-14 rounded-lg overflow-hidden bg-black/50 border border-white/[0.08] shrink-0 relative">
                {trackImageUrl ? (
                  <img
                    src={trackImageUrl}
                    alt={armedTrack?.name ?? armedSession.trackName}
                    className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                  />
                ) : (
                  <div className="w-full h-full bg-gradient-to-br from-white/[0.08] to-transparent flex items-center justify-center text-[var(--text-muted)]">
                    <Compass className="w-5 h-5" />
                  </div>
                )}
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[10px] font-mono uppercase tracking-[0.14em] text-[var(--accent)]">
                  Armed Circuit
                </div>
                <div className="text-sm font-semibold text-[var(--text-primary)] truncate">
                  {armedTrack?.name ?? armedSession.trackName}
                </div>
                <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] font-mono text-[var(--text-muted)] mt-0.5 tabular-nums">
                  <span className="whitespace-nowrap">{armedTrack?.country ?? "International"}</span>
                  {armedTrack?.length && (
                    <>
                      <span>·</span>
                      <span className="whitespace-nowrap">{armedTrack.length}</span>
                    </>
                  )}
                  {armedTrack?.pitboxes && (
                    <>
                      <span>·</span>
                      <span className="whitespace-nowrap">{armedTrack.pitboxes} pits</span>
                    </>
                  )}
                </div>
              </div>
              <ChevronRight className="w-4 h-4 text-[var(--text-muted)] group-hover:text-[var(--text-primary)] group-hover:translate-x-0.5 transition-all shrink-0" />
            </div>
          </div>
        </div>
      </section>

      {/* 4-Tile Motorsport KPI Strip */}
      <section className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-4">
        <KpiTile
          label="Drive Time"
          value={formatNumber(drivingStats.totalHours, 1)}
          unit="hrs"
          sublabel={
            drivingStats.favoriteCar
              ? `Top car: ${drivingStats.favoriteCar}`
              : "Track time logged"
          }
          icon={<Timer className="w-4 h-4" />}
          accent
        />
        <KpiTile
          label="Distance Driven"
          value={formatNumber(drivingStats.totalKm, 1)}
          unit="km"
          sublabel={
            drivingStats.favoriteTrack
              ? `Favorite: ${drivingStats.favoriteTrack}`
              : "Total odometer"
          }
          icon={<Gauge className="w-4 h-4" />}
        />
        <KpiTile
          label="Sessions & Laps"
          value={formatNumber(drivingStats.sessionsCount)}
          unit="runs"
          sublabel={`${formatNumber(drivingStats.totalLaps)} completed laps`}
          icon={<Activity className="w-4 h-4" />}
        />
        <KpiTile
          label="Garage Library"
          value={formatNumber(cars.length)}
          unit="cars"
          sublabel={`${formatNumber(tracks.length)} circuits · ${
            cspBuild ? `CSP ${cspBuild}` : "Vanilla AC"
          }`}
          icon={<Layers className="w-4 h-4" />}
        />
      </section>

      {/* Visual Quick-Jump Grid */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-[11px] font-mono uppercase tracking-[0.14em] text-[var(--text-muted)]">
              Quick Access
            </div>
            <h2 className="font-display text-xl font-bold uppercase tracking-wider text-[var(--text-primary)]">
              Paddock Modules
            </h2>
          </div>
          <Button
            variant="ghost"
            size="sm"
            icon={<Car className="w-3.5 h-3.5" />}
            onClick={() => navigate("garage", "cars")}
          >
            Browse All Cars
          </Button>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {quickJumps.map((item) => (
            <Card
              key={item.title}
              variant="interactive"
              padding="md"
              onClick={() => navigate(item.pillar, item.subTab)}
              className="group relative overflow-hidden"
            >
              <div
                className={`absolute inset-0 bg-gradient-to-br ${item.accentColor} opacity-0 group-hover:opacity-100 transition-opacity duration-200 pointer-events-none`}
              />
              <div className="relative flex items-start justify-between gap-3">
                <div className="flex items-start gap-3.5 min-w-0">
                  <div className="w-10 h-10 rounded-xl bg-white/[0.04] border border-white/[0.08] flex items-center justify-center text-[var(--text-secondary)] group-hover:text-[var(--accent)] group-hover:border-[var(--accent-border)] transition-colors shrink-0">
                    {item.icon}
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <h3 className="text-sm font-semibold text-[var(--text-primary)] truncate">
                        {item.title}
                      </h3>
                    </div>
                    <p className="text-xs text-[var(--text-muted)] mt-0.5 line-clamp-2 leading-relaxed">
                      {item.subtitle}
                    </p>
                  </div>
                </div>
                <div className="flex flex-col items-end gap-2 shrink-0">
                  <Badge variant="neutral">{item.meta}</Badge>
                  <ChevronRight className="w-4 h-4 text-[var(--text-muted)] group-hover:text-[var(--text-primary)] group-hover:translate-x-0.5 transition-all" />
                </div>
              </div>
            </Card>
          ))}
        </div>
      </section>
    </div>
  );
}

export default PaddockPage;

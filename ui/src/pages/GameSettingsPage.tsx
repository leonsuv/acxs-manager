import { useEffect, useMemo, useState } from "react";
import {
  Camera,
  Check,
  Gamepad2,
  Glasses,
  History,
  Layers,
  Monitor,
  RotateCcw,
  Save,
  Sliders,
  Volume2,
  Wrench,
} from "lucide-react";
import { api } from "../lib/api";
import type {
  ConfigSnapshot,
  FfbConfig,
  RigProfile,
  VideoSettings,
  VrTripleSettings,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  PageHeader,
  Slider,
  Toggle,
} from "../components/Primitives";

type CategoryId =
  | "display"
  | "vr-rigs"
  | "controls-audio"
  | "gameplay-snapshots";

const LOCAL_PREFS_KEY = "acxs_game_local_prefs_v1";

interface LocalGamePrefs {
  fovDeg: number;
  replayQuality: "Medium" | "High" | "Ultra";
  masterVol: number;
  engineVol: number;
  tyreVol: number;
  autoBlip: boolean;
  idealLine: boolean;
}

const DEFAULT_LOCAL_PREFS: LocalGamePrefs = {
  fovDeg: 54,
  replayQuality: "High",
  masterVol: 85,
  engineVol: 92,
  tyreVol: 78,
  autoBlip: false,
  idealLine: false,
};

function loadLocalPrefs(): LocalGamePrefs {
  try {
    const raw = localStorage.getItem(LOCAL_PREFS_KEY);
    if (!raw) return DEFAULT_LOCAL_PREFS;
    return { ...DEFAULT_LOCAL_PREFS, ...(JSON.parse(raw) as Partial<LocalGamePrefs>) };
  } catch {
    return DEFAULT_LOCAL_PREFS;
  }
}

const NAV_GROUPS: Array<{
  title: string;
  items: Array<{ id: CategoryId; label: string; desc: string; icon: typeof Monitor }>;
}> = [
  {
    title: "VISUALS & RIG",
    items: [
      { id: "display", label: "Display, Video & Camera", desc: "video.ini · FOV · Replay", icon: Monitor },
      { id: "vr-rigs", label: "VR, Triple & Rig Profiles", desc: "Atomic rig & optics", icon: Glasses },
    ],
  },
  {
    title: "DRIVING & SYSTEM",
    items: [
      { id: "controls-audio", label: "Controls, FFB & Audio", desc: "Force feedback · LUT · Mix", icon: Sliders },
      { id: "gameplay-snapshots", label: "Gameplay & Snapshots", desc: "Assists · INI backups", icon: History },
    ],
  },
];

const DEFAULT_VIDEO: VideoSettings = {
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
};

const DEFAULT_VR: VrTripleSettings = {
  renderMode: "SINGLE",
  tripleFovDeg: 54,
  screenWidthMm: 600,
  distanceMm: 650,
  bezelMm: 10,
  sideAngleDeg: 45,
  vrPixelDensity: 1.2,
};

const DEFAULT_FFB: FfbConfig = {
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
};

export default function GameSettingsPage() {
  const { notify } = useAppStore();
  const [category, setCategory] = useState<CategoryId>("display");
  const [saving, setSaving] = useState(false);

  const [video, setVideo] = useState<VideoSettings>(DEFAULT_VIDEO);
  const [savedVideo, setSavedVideo] = useState<VideoSettings>(DEFAULT_VIDEO);

  const [vr, setVr] = useState<VrTripleSettings>(DEFAULT_VR);
  const [savedVr, setSavedVr] = useState<VrTripleSettings>(DEFAULT_VR);

  const [ffb, setFfb] = useState<FfbConfig>(DEFAULT_FFB);
  const [savedFfb, setSavedFfb] = useState<FfbConfig>(DEFAULT_FFB);

  const [rigs, setRigs] = useState<RigProfile[]>([]);
  const [snapshots, setSnapshots] = useState<ConfigSnapshot[]>([]);
  const [snapshotLabel, setSnapshotLabel] = useState("");

  // Local-only preview preferences (persisted in localStorage)
  const [localPrefs, setLocalPrefs] = useState<LocalGamePrefs>(() => loadLocalPrefs());
  const updateLocalPref = <K extends keyof LocalGamePrefs>(
    key: K,
    val: LocalGamePrefs[K]
  ) => {
    setLocalPrefs((prev) => {
      const next = { ...prev, [key]: val };
      try {
        localStorage.setItem(LOCAL_PREFS_KEY, JSON.stringify(next));
      } catch {
        // ignore storage errors
      }
      return next;
    });
  };

  useEffect(() => {
    void Promise.all([
      api.getVideoSettings().catch(() => null),
      api.getVrTripleSettings().catch(() => null),
      api.getFfbConfig().catch(() => null),
      api.listRigProfiles().catch(() => []),
      api.listConfigSnapshots().catch(() => []),
    ]).then(([v, vrRes, ffbRes, rigList, snapList]) => {
      if (v) {
        setVideo(v);
        setSavedVideo(v);
      }
      if (vrRes) {
        const modeFromVideo: VrTripleSettings["renderMode"] =
          v?.cameraMode === "Triple"
            ? "TRIPLE"
            : v?.cameraMode === "OpenVr"
              ? "OPENVR"
              : v?.cameraMode === "Oculus"
                ? "OCULUS"
                : vrRes.renderMode ?? DEFAULT_VR.renderMode;
        const normalizedVr: VrTripleSettings = {
          ...vrRes,
          renderMode: modeFromVideo,
        };
        setVr(normalizedVr);
        setSavedVr(normalizedVr);
      }
      if (ffbRes) {
        setFfb(ffbRes);
        setSavedFfb(ffbRes);
      }
      setRigs(
        rigList && rigList.length > 0
          ? rigList
          : [
              { name: "Flat 1440p Desk", videoMode: "SINGLE", ffbProfile: "DD 12Nm GT3", description: "2560×1440 @ 144Hz single screen", active: true },
              { name: "Triple 32\" Surround", videoMode: "TRIPLE", ffbProfile: "DD 15Nm Endurance", description: "7680×1440 bezel-corrected 45° side monitors" },
              { name: "OpenVR Headset", videoMode: "OPENVR", ffbProfile: "DD 12Nm VR Smooth", description: "90Hz locked reprojection-free VR profile" },
            ]
      );
      setSnapshots(
        snapList && snapList.length > 0
          ? snapList
          : [
              { id: "snap-1", label: "Pre-CSP 0.2.5 Baseline", createdAt: "2 days ago", filesCount: 6 },
              { id: "snap-2", label: "VR 90Hz Stable", createdAt: "Last week", filesCount: 6 },
            ]
      );
    });
  }, []);

  const dirty = useMemo(() => {
    return (
      JSON.stringify(video) !== JSON.stringify(savedVideo) ||
      JSON.stringify(vr) !== JSON.stringify(savedVr) ||
      JSON.stringify(ffb) !== JSON.stringify(savedFfb)
    );
  }, [video, savedVideo, vr, savedVr, ffb, savedFfb]);

  const visualPerf = useMemo(() => {
    const quality = Math.min(
      100,
      Math.round(
        (video.worldDetail / 5) * 30 +
          (Math.min(video.msaa, 8) / 8) * 25 +
          (video.shadowMapSize / 4096) * 25 +
          (video.postProcessing ? 20 : 0)
      )
    );
    const performance = Math.max(15, 115 - Math.round(quality * 0.85));
    return { quality, performance };
  }, [video]);

  const handleSaveAll = async () => {
    setSaving(true);
    try {
      const cameraMode =
        vr.renderMode === "TRIPLE"
          ? "Triple"
          : vr.renderMode === "OPENVR"
            ? "OpenVr"
            : vr.renderMode === "OCULUS"
              ? "Oculus"
              : "Default";
      const videoToSave: VideoSettings = { ...video, cameraMode };
      await Promise.all([
        api.saveVideoSettings(videoToSave),
        api.saveVrTripleSettings(vr),
        api.saveFfbConfig(ffb),
      ]);
      setVideo(videoToSave);
      setSavedVideo(videoToSave);
      setSavedVr(vr);
      setSavedFfb(ffb);
      notify("Game settings saved", "success", "video.ini, triple_screen.ini & ff_post_process.ini updated");
    } catch (err) {
      notify("Failed to save settings", "error", String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleRevert = () => {
    setVideo(savedVideo);
    setVr(savedVr);
    setFfb(savedFfb);
    notify("Reverted pending changes", "info");
  };

  const handleAtomicRigSwitch = async (mode: VrTripleSettings["renderMode"]) => {
    const next = { ...vr, renderMode: mode };
    const cameraMode: VideoSettings["cameraMode"] =
      mode === "TRIPLE"
        ? "Triple"
        : mode === "OPENVR"
          ? "OpenVr"
          : mode === "OCULUS"
            ? "Oculus"
            : "Default";
    const nextVideo: VideoSettings = { ...video, cameraMode };
    setVr(next);
    setVideo(nextVideo);
    try {
      await Promise.all([
        api.saveVrTripleSettings(next),
        api.saveVideoSettings(nextVideo),
      ]);
      setSavedVr(next);
      setSavedVideo(nextVideo);
      notify("Rig render mode switched", "success", `Active mode: ${mode}`);
    } catch (err) {
      notify("Rig switch failed", "error", String(err));
    }
  };

  const handleApplyRig = async (name: string) => {
    try {
      await api.applyRigProfile(name);
      setRigs((prev) => prev.map((r) => ({ ...r, active: r.name === name })));
      notify("Rig profile applied", "success", name);
    } catch (err) {
      notify("Could not apply rig profile", "error", String(err));
    }
  };

  const handleCreateSnapshot = async () => {
    const label = snapshotLabel.trim() || `Snapshot ${new Date().toLocaleTimeString()}`;
    try {
      const created = await api.createConfigSnapshot(label);
      setSnapshots((prev) => [
        created ?? { id: `snap-${Date.now()}`, label, createdAt: "Just now", filesCount: 6 },
        ...prev,
      ]);
      setSnapshotLabel("");
      notify("Config snapshot created", "success", label);
    } catch (err) {
      notify("Snapshot failed", "error", String(err));
    }
  };

  return (
    <div className="p-6 max-w-[1620px] mx-auto pb-24 space-y-5 overflow-x-hidden animate-fade-in">
      <PageHeader
        eyebrow="SETUP · ASSETTO CORSA CONFIGURATION"
        title="Game Settings"
        subtitle="Configure video, atomic VR/Triple switching, force feedback & LUTs, and atomic config snapshots."
        actions={
          dirty ? (
            <Badge variant="warning" dot>
              Unsaved Changes
            </Badge>
          ) : (
            <Badge variant="success" dot>
              Synced with AC cfg/
            </Badge>
          )
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
        {/* Master-Detail Left Category Rail */}
        <Card className="lg:col-span-3 min-w-0 space-y-5" padding="sm">
          {NAV_GROUPS.map((grp) => (
            <div key={grp.title} className="space-y-1">
              <div className="px-2.5 py-1 text-[10px] font-mono uppercase tracking-[0.14em] text-[var(--text-muted)]">
                {grp.title}
              </div>
              {grp.items.map((item) => {
                const Icon = item.icon;
                const active = category === item.id;
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => setCategory(item.id)}
                    className={`w-full flex items-start gap-2.5 px-3 py-2.5 rounded-lg text-left transition-all cursor-pointer ${
                      active
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
                        : "text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.04] border border-transparent"
                    }`}
                  >
                    <Icon className="w-4 h-4 shrink-0 mt-0.5" />
                    <div className="min-w-0">
                      <div className="text-[13px] font-medium leading-tight">{item.label}</div>
                      <div className="text-[10px] font-mono text-[var(--text-muted)] mt-0.5 truncate">
                        {item.desc}
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          ))}
        </Card>

        {/* Full-Width 2-Column Content Area */}
        <div className="lg:col-span-9 min-w-0 space-y-5">
          {/* GROUP 1: DISPLAY, VIDEO & CAMERA */}
          {category === "display" && (
            <div className="space-y-5">
              <div className="grid grid-cols-1 xl:grid-cols-12 gap-5">
              <Card className="xl:col-span-7 min-w-0 space-y-4" padding="md">
                <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Display & Rendering Pipeline
                </div>
                <div className="grid grid-cols-3 gap-3">
                  <div>
                    <label className="text-[11px] text-[var(--text-secondary)] block mb-1">
                      Width
                    </label>
                    <input
                      type="number"
                      value={video.width}
                      onChange={(e) => setVideo({ ...video, width: Number(e.target.value) })}
                      className="w-full rounded-lg bg-black/40 border border-white/[0.09] px-2.5 py-1.5 text-xs font-mono"
                    />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-secondary)] block mb-1">
                      Height
                    </label>
                    <input
                      type="number"
                      value={video.height}
                      onChange={(e) => setVideo({ ...video, height: Number(e.target.value) })}
                      className="w-full rounded-lg bg-black/40 border border-white/[0.09] px-2.5 py-1.5 text-xs font-mono"
                    />
                  </div>
                  <div>
                    <label className="text-[11px] text-[var(--text-secondary)] block mb-1">
                      Refresh (Hz)
                    </label>
                    <input
                      type="number"
                      value={video.refreshHz}
                      onChange={(e) => setVideo({ ...video, refreshHz: Number(e.target.value) })}
                      className="w-full rounded-lg bg-black/40 border border-white/[0.09] px-2.5 py-1.5 text-xs font-mono"
                    />
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-4 pt-2">
                  <Toggle
                    checked={video.fullscreen}
                    onChange={(v) => setVideo({ ...video, fullscreen: v })}
                    label="Fullscreen Exclusive"
                  />
                  <Toggle
                    checked={video.vsync}
                    onChange={(v) => setVideo({ ...video, vsync: v })}
                    label="Vertical Sync"
                  />
                  <Toggle
                    checked={video.postProcessing}
                    onChange={(v) => setVideo({ ...video, postProcessing: v })}
                    label="Post-Processing Filter"
                  />
                </div>

                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-[var(--text-secondary)] font-medium">
                      MSAA Anti-Aliasing
                    </span>
                    <span className="font-mono text-[var(--text-primary)]">
                      {video.msaa <= 1 ? "Off" : `${video.msaa}x`}
                    </span>
                  </div>
                  <div className="grid grid-cols-4 gap-2">
                    {[
                      { label: "Off", value: 0 },
                      { label: "2x", value: 2 },
                      { label: "4x", value: 4 },
                      { label: "8x", value: 8 },
                    ].map((opt) => {
                      const currentAa = video.msaa <= 1 ? 0 : video.msaa;
                      const active = currentAa === opt.value;
                      return (
                        <button
                          key={opt.value}
                          type="button"
                          onClick={() => setVideo({ ...video, msaa: opt.value })}
                          className={`h-8 rounded-lg border text-xs font-mono transition-colors cursor-pointer ${
                            active
                              ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent)] font-semibold"
                              : "bg-white/[0.04] border-white/[0.08] text-[var(--text-secondary)] hover:text-white"
                          }`}
                        >
                          {opt.label}
                        </button>
                      );
                    })}
                  </div>
                </div>
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-[var(--text-secondary)] font-medium">
                      Anisotropic Filtering
                    </span>
                    <span className="font-mono text-[var(--text-primary)]">
                      {video.anisotropic === 0 ? "Off" : `${video.anisotropic}x`}
                    </span>
                  </div>
                  <div className="grid grid-cols-5 gap-1.5">
                    {[0, 2, 4, 8, 16].map((val) => {
                      const active = video.anisotropic === val;
                      return (
                        <button
                          key={val}
                          type="button"
                          onClick={() => setVideo({ ...video, anisotropic: val })}
                          className={`h-8 rounded-lg border text-xs font-mono transition-colors cursor-pointer ${
                            active
                              ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent)] font-semibold"
                              : "bg-white/[0.04] border-white/[0.08] text-[var(--text-secondary)] hover:text-white"
                          }`}
                        >
                          {val === 0 ? "Off" : `${val}x`}
                        </button>
                      );
                    })}
                  </div>
                </div>
                <Slider
                  label="World Detail"
                  value={video.worldDetail}
                  min={0}
                  max={5}
                  step={1}
                  formatValue={(v) => ["Minimum", "Very Low", "Low", "Medium", "High", "Maximum"][v] ?? String(v)}
                  onChange={(v) => setVideo({ ...video, worldDetail: v })}
                />
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-[var(--text-secondary)] font-medium">
                      Shadow Map Resolution
                    </span>
                    <span className="font-mono text-[var(--text-primary)]">
                      {video.shadowMapSize <= 0 ? "Off" : `${video.shadowMapSize} px`}
                    </span>
                  </div>
                  <div className="grid grid-cols-5 gap-1.5">
                    {[
                      { label: "Off", value: 0 },
                      { label: "512", value: 512 },
                      { label: "1024", value: 1024 },
                      { label: "2048", value: 2048 },
                      { label: "4096", value: 4096 },
                    ].map((opt) => {
                      const currentShadow = video.shadowMapSize <= 0 ? 0 : video.shadowMapSize;
                      const active = currentShadow === opt.value;
                      return (
                        <button
                          key={opt.value}
                          type="button"
                          onClick={() => setVideo({ ...video, shadowMapSize: opt.value })}
                          className={`h-8 rounded-lg border text-xs font-mono transition-colors cursor-pointer ${
                            active
                              ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent)] font-semibold"
                              : "bg-white/[0.04] border-white/[0.08] text-[var(--text-secondary)] hover:text-white"
                          }`}
                        >
                          {opt.label}
                        </button>
                      );
                    })}
                  </div>
                </div>
              </Card>

              {/* Right: Live Visual Quality vs Performance Bars */}
              <Card className="xl:col-span-5 min-w-0 space-y-5" padding="md">
                <div>
                  <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Live Telemetry Estimator
                  </div>
                  <div className="text-base font-semibold text-[var(--text-primary)] mt-0.5">
                    Visual Quality vs Performance
                  </div>
                </div>

                <div className="space-y-4">
                  <div className="space-y-1.5">
                    <div className="flex justify-between text-xs font-mono">
                      <span className="text-[var(--text-secondary)]">VISUAL FIDELITY</span>
                      <span className="text-cyan-300 font-semibold">{visualPerf.quality}%</span>
                    </div>
                    <div className="h-3 rounded-full bg-white/[0.07] overflow-hidden">
                      <div
                        className="h-full rounded-full transition-all duration-300"
                        style={{
                          width: `${visualPerf.quality}%`,
                          background: "linear-gradient(90deg, #00D1FF, #38BDF8)",
                        }}
                      />
                    </div>
                  </div>

                  <div className="space-y-1.5">
                    <div className="flex justify-between text-xs font-mono">
                      <span className="text-[var(--text-secondary)]">FRAME-RATE HEADROOM</span>
                      <span className="text-emerald-300 font-semibold">{visualPerf.performance}%</span>
                    </div>
                    <div className="h-3 rounded-full bg-white/[0.07] overflow-hidden">
                      <div
                        className="h-full rounded-full transition-all duration-300"
                        style={{
                          width: `${visualPerf.performance}%`,
                          background: "linear-gradient(90deg, #10B981, #34D399)",
                        }}
                      />
                    </div>
                  </div>
                </div>

                <div className="rounded-xl bg-black/35 border border-white/[0.07] p-3.5 space-y-2 text-xs">
                  <div className="flex justify-between">
                    <span className="text-[var(--text-muted)]">PP Filter</span>
                    <input
                      value={video.ppFilter}
                      onChange={(e) => setVideo({ ...video, ppFilter: e.target.value })}
                      className="bg-transparent text-right font-mono text-[var(--text-primary)] focus:outline-none border-b border-white/10 w-36"
                    />
                  </div>
                  <div className="flex justify-between">
                    <span className="text-[var(--text-muted)]">FPS Cap</span>
                    <span className="font-mono text-[var(--text-primary)]">
                      {video.fpsCap === 0 ? "Unlimited" : `${video.fpsCap} FPS`}
                    </span>
                  </div>
                </div>
              </Card>
              </div>

              {/* Camera & Replay Card */}
              <Card padding="md" className="space-y-4">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2 text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    <Camera className="w-3.5 h-3.5 text-[var(--accent)]" />
                    <span>Cockpit Camera &amp; Replay Quality</span>
                  </div>
                  <Badge variant="info">Local Preview</Badge>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-6 items-center">
                  <Slider
                    label="Vertical FOV"
                    value={localPrefs.fovDeg}
                    min={32}
                    max={80}
                    unit="°"
                    onChange={(v) => updateLocalPref("fovDeg", v)}
                  />
                  <div className="space-y-2">
                    <div className="text-[11px] font-medium text-[var(--text-secondary)]">
                      Replay Recording Quality
                    </div>
                    <div className="flex gap-2">
                      {(["Medium", "High", "Ultra"] as const).map((q) => (
                        <Button
                          key={q}
                          size="sm"
                          variant={localPrefs.replayQuality === q ? "primary" : "secondary"}
                          onClick={() => updateLocalPref("replayQuality", q)}
                        >
                          {q}
                        </Button>
                      ))}
                    </div>
                  </div>
                </div>
              </Card>
            </div>
          )}

          {/* GROUP 2: VR, TRIPLE SCREEN & RIG PROFILES */}
          {category === "vr-rigs" && (
            <div className="space-y-5">
              <Card padding="md" className="space-y-3">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                      1-Click Atomic Rig Switcher
                    </div>
                    <div className="text-sm font-semibold text-[var(--text-primary)]">
                      Switch VR ↔ Triple Screen ↔ Flat Display atomically
                    </div>
                  </div>
                  <Badge variant="accent">ACTIVE: {vr.renderMode}</Badge>
                </div>

                <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                  {(
                    [
                      { id: "SINGLE", label: "Flat Single", desc: "Standard monitor" },
                      { id: "TRIPLE", label: "Triple Screen", desc: "Bezel-corrected surround" },
                      { id: "OPENVR", label: "OpenVR / SteamVR", desc: "Index · Bigscreen · Quest" },
                      { id: "OCULUS", label: "Oculus Rift", desc: "Native Oculus SDK" },
                    ] as const
                  ).map((m) => {
                    const active = vr.renderMode === m.id;
                    return (
                      <button
                        key={m.id}
                        type="button"
                        onClick={() => void handleAtomicRigSwitch(m.id)}
                        className={`p-3.5 rounded-xl border text-left transition-all cursor-pointer ${
                          active
                            ? "bg-[var(--accent-soft)] border-[var(--accent-border)]"
                            : "bg-white/[0.03] border-white/[0.07] hover:bg-white/[0.06]"
                        }`}
                      >
                        <div className="flex items-center justify-between">
                          <span className="text-sm font-semibold text-[var(--text-primary)]">
                            {m.label}
                          </span>
                          {active && <Check className="w-4 h-4 text-[var(--accent)]" />}
                        </div>
                        <div className="text-[11px] text-[var(--text-secondary)] mt-1">
                          {m.desc}
                        </div>
                      </button>
                    );
                  })}
                </div>
              </Card>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <Card padding="md" className="space-y-4">
                  <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Triple-Screen Geometry
                  </div>
                  <Slider
                    label="Screen Width"
                    value={vr.screenWidthMm}
                    min={400}
                    max={1100}
                    step={5}
                    unit=" mm"
                    onChange={(v) => setVr({ ...vr, screenWidthMm: v })}
                  />
                  <Slider
                    label="Eye Distance"
                    value={vr.distanceMm}
                    min={400}
                    max={1200}
                    step={5}
                    unit=" mm"
                    onChange={(v) => setVr({ ...vr, distanceMm: v })}
                  />
                  <Slider
                    label="Side Monitor Angle"
                    value={vr.sideAngleDeg}
                    min={20}
                    max={75}
                    step={1}
                    unit="°"
                    onChange={(v) => setVr({ ...vr, sideAngleDeg: v })}
                  />
                </Card>

                <Card padding="md" className="space-y-4">
                  <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    VR Supersampling & Optics
                  </div>
                  <Slider
                    label="VR Pixel Density"
                    value={vr.vrPixelDensity}
                    min={0.8}
                    max={2.0}
                    step={0.05}
                    formatValue={(v) => `${v.toFixed(2)}x`}
                    onChange={(v) => setVr({ ...vr, vrPixelDensity: v })}
                  />
                  <Slider
                    label="Bezel Width"
                    value={vr.bezelMm}
                    min={0}
                    max={30}
                    step={1}
                    unit=" mm"
                    onChange={(v) => setVr({ ...vr, bezelMm: v })}
                  />
                </Card>
              </div>

              {/* Rig Profiles */}
              <div className="space-y-3">
                <div className="flex items-center gap-2 text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  <Layers className="w-3.5 h-3.5 text-[var(--accent)]" />
                  <span>Saved Rig Profiles</span>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                  {rigs.map((rig) => (
                    <Card
                      key={rig.name}
                      variant="elevated"
                      padding="md"
                      className={rig.active ? "border-[var(--accent-border)]" : ""}
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="text-sm font-semibold text-[var(--text-primary)]">
                          {rig.name}
                        </div>
                        {rig.active && <Badge variant="accent">ACTIVE</Badge>}
                      </div>
                      <div className="text-xs text-[var(--text-secondary)] mt-1.5">
                        {rig.description ?? "Saved video & FFB rig configuration"}
                      </div>
                      <div className="mt-3 pt-3 border-t border-white/[0.06] flex items-center justify-between text-[11px] font-mono">
                        <span className="text-[var(--text-muted)]">{rig.videoMode}</span>
                        <Button
                          variant={rig.active ? "secondary" : "primary"}
                          size="sm"
                          onClick={() => void handleApplyRig(rig.name)}
                        >
                          {rig.active ? "Re-Apply" : "Switch Rig"}
                        </Button>
                      </div>
                    </Card>
                  ))}
                </div>
              </div>
            </div>
          )}

          {/* GROUP 3: CONTROLS, FFB & AUDIO */}
          {category === "controls-audio" && (
            <div className="space-y-5">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
                <Card padding="md" className="space-y-4">
                  <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Force Feedback Core Gain &amp; Effects
                  </div>
                  <Slider
                    label="Master Gain"
                    value={Math.round(ffb.gain * 100)}
                    min={10}
                    max={150}
                    unit="%"
                    onChange={(v) => setFfb({ ...ffb, gain: v / 100 })}
                  />
                  <Slider
                    label="Minimum Force"
                    value={Math.round(ffb.minForce * 100)}
                    min={0}
                    max={25}
                    unit="%"
                    onChange={(v) => setFfb({ ...ffb, minForce: v / 100 })}
                  />
                  <Slider
                    label="Kerb Effect"
                    value={Math.round(ffb.kerbEffect * 100)}
                    min={0}
                    max={100}
                    unit="%"
                    onChange={(v) => setFfb({ ...ffb, kerbEffect: v / 100 })}
                  />
                  <Slider
                    label="Road Effect"
                    value={Math.round(ffb.roadEffect * 100)}
                    min={0}
                    max={100}
                    unit="%"
                    onChange={(v) => setFfb({ ...ffb, roadEffect: v / 100 })}
                  />
                  <Slider
                    label="Slip Effect"
                    value={Math.round(ffb.slipEffect * 100)}
                    min={0}
                    max={100}
                    unit="%"
                    onChange={(v) => setFfb({ ...ffb, slipEffect: v / 100 })}
                  />
                </Card>

                <Card padding="md" className="space-y-4">
                  <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Post-Processing &amp; Gamma / LUT Curve
                  </div>
                  <Toggle
                    checked={ffb.enhancedUndersteer}
                    onChange={(v) => setFfb({ ...ffb, enhancedUndersteer: v })}
                    label="Enhanced Understeer Effect"
                    description="Drops steering torque progressively when front tyres exceed peak slip angle."
                  />
                  <Toggle
                    checked={ffb.gammaEnabled}
                    onChange={(v) => setFfb({ ...ffb, gammaEnabled: v })}
                    label="Enable Gamma Linearity Curve"
                  />
                  <Slider
                    label="Gamma Value"
                    value={ffb.gammaValue}
                    min={0.5}
                    max={1.5}
                    step={0.05}
                    disabled={!ffb.gammaEnabled}
                    formatValue={(v) => v.toFixed(2)}
                    onChange={(v) => setFfb({ ...ffb, gammaValue: v })}
                  />
                  <div className="rounded-xl bg-[#070A10] border border-white/[0.06] p-3">
                    <div className="text-[10px] font-mono text-[var(--text-muted)] mb-1">
                      TORQUE RESPONSE CURVE
                    </div>
                    <svg viewBox="0 0 220 96" className="w-full h-[96px]">
                      <line x1="10" y1="86" x2="210" y2="10" stroke="rgba(255,255,255,0.15)" strokeDasharray="3 3" />
                      <path
                        d={Array.from({ length: 24 }, (_, i) => {
                          const x = i / 23;
                          const y = Math.pow(x, ffb.gammaEnabled ? ffb.gammaValue : 1);
                          return `${i === 0 ? "M" : "L"} ${(10 + x * 200).toFixed(1)} ${(86 - y * 76).toFixed(1)}`;
                        }).join(" ")}
                        fill="none"
                        stroke="var(--accent)"
                        strokeWidth="2.5"
                      />
                    </svg>
                  </div>
                </Card>
              </div>

              {/* DirectInput + Audio Mix */}
              <div className="grid grid-cols-1 md:grid-cols-12 gap-5">
                <Card padding="md" className="md:col-span-5 min-w-0 space-y-2.5">
                  <div className="flex items-center gap-2 text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    <Gamepad2 className="w-3.5 h-3.5 text-cyan-400" />
                    <span>Wheel &amp; Pedal Bindings</span>
                  </div>
                  <div className="text-sm font-semibold text-[var(--text-primary)]">
                    DirectInput / controls.ini Synced
                  </div>
                  <p className="text-xs text-[var(--text-secondary)] leading-relaxed">
                    Bind steering axes, shift paddles, DRS, and pit limiter inside Assetto Corsa or switch between saved Rig Profiles.
                  </p>
                </Card>

                <Card padding="md" className="md:col-span-7 min-w-0 space-y-4">
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                      <Volume2 className="w-3.5 h-3.5 text-[var(--accent)]" />
                      <span>Audio Mix Levels</span>
                    </div>
                    <Badge variant="info">Local Preview</Badge>
                  </div>
                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
                    <Slider
                      label="Master Volume"
                      value={localPrefs.masterVol}
                      min={0}
                      max={100}
                      unit="%"
                      onChange={(v) => updateLocalPref("masterVol", v)}
                    />
                    <Slider
                      label="Engine & Exhaust"
                      value={localPrefs.engineVol}
                      min={0}
                      max={100}
                      unit="%"
                      onChange={(v) => updateLocalPref("engineVol", v)}
                    />
                    <Slider
                      label="Tyre Scrub & Slip"
                      value={localPrefs.tyreVol}
                      min={0}
                      max={100}
                      unit="%"
                      onChange={(v) => updateLocalPref("tyreVol", v)}
                    />
                  </div>
                </Card>
              </div>
            </div>
          )}

          {/* GROUP 4: GAMEPLAY & CONFIG SNAPSHOTS */}
          {category === "gameplay-snapshots" && (
            <div className="space-y-5">
              <Card padding="md" className="space-y-4">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2 text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    <Wrench className="w-3.5 h-3.5 text-[var(--accent)]" />
                    <span>Gameplay &amp; Driving Aids</span>
                  </div>
                  <Badge variant="info">Local Preview</Badge>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <Toggle
                    checked={localPrefs.autoBlip}
                    onChange={(v) => updateLocalPref("autoBlip", v)}
                    label="Auto-Blip on Downshift"
                    description="Automatically rev-match H-pattern manual gearboxes."
                  />
                  <Toggle
                    checked={localPrefs.idealLine}
                    onChange={(v) => updateLocalPref("idealLine", v)}
                    label="Show On-Track Ideal Line"
                    description="Display dynamic braking & racing line overlay in-sim."
                  />
                </div>
              </Card>

              <Card padding="md" className="space-y-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <div className="text-sm font-semibold text-[var(--text-primary)]">
                      Atomic Configuration Snapshots
                    </div>
                    <div className="text-xs text-[var(--text-secondary)]">
                      Back up and restore video.ini, controls.ini, and CSP configs in one click.
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <input
                      value={snapshotLabel}
                      onChange={(e) => setSnapshotLabel(e.target.value)}
                      placeholder="Snapshot label..."
                      className="h-8 rounded-lg bg-black/40 border border-white/[0.1] px-2.5 text-xs text-[var(--text-primary)] focus:outline-none"
                    />
                    <Button variant="primary" size="sm" onClick={() => void handleCreateSnapshot()}>
                      Create Snapshot
                    </Button>
                  </div>
                </div>

                <div className="space-y-2">
                  {snapshots.map((s) => (
                    <div
                      key={s.id}
                      className="flex items-center justify-between p-3 rounded-xl bg-white/[0.03] border border-white/[0.06]"
                    >
                      <div>
                        <div className="text-[13px] font-semibold text-[var(--text-primary)]">
                          {s.label}
                        </div>
                        <div className="text-[11px] font-mono text-[var(--text-muted)]">
                          {s.createdAt} · {s.filesCount} ini files
                        </div>
                      </div>
                      <Button
                        variant="secondary"
                        size="sm"
                        icon={<RotateCcw className="w-3.5 h-3.5" />}
                        onClick={() => {
                          void api.restoreConfigSnapshot(s.id).catch(() => {});
                          notify("Snapshot restored", "success", s.label);
                        }}
                      >
                        Restore
                      </Button>
                    </div>
                  ))}
                </div>
              </Card>
            </div>
          )}
        </div>
      </div>

      {/* Sticky Bottom Pending Changes Bar */}
      {dirty && (
        <div className="fixed bottom-20 left-1/2 -translate-x-1/2 z-40 glass-panel border-[var(--accent-border)] rounded-2xl px-4 py-2.5 flex items-center gap-4 shadow-2xl animate-scale-in">
          <Badge variant="warning" dot>
            Pending Changes
          </Badge>
          <span className="text-xs text-[var(--text-secondary)] hidden sm:inline">
            You have unsaved Assetto Corsa configuration edits.
          </span>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              icon={<RotateCcw className="w-3.5 h-3.5" />}
              onClick={handleRevert}
            >
              Revert
            </Button>
            <Button
              variant="primary"
              size="sm"
              loading={saving}
              icon={<Save className="w-3.5 h-3.5" />}
              onClick={() => void handleSaveAll()}
            >
              Save Changes
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

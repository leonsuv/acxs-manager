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
  | "video"
  | "vr-triple"
  | "camera"
  | "controls"
  | "ffb"
  | "rigs"
  | "audio"
  | "gameplay"
  | "snapshots";

const NAV_GROUPS: Array<{
  title: string;
  items: Array<{ id: CategoryId; label: string; icon: typeof Monitor }>;
}> = [
  {
    title: "DISPLAY",
    items: [
      { id: "video", label: "Video", icon: Monitor },
      { id: "vr-triple", label: "VR & Triple", icon: Glasses },
      { id: "camera", label: "Camera & Replay", icon: Camera },
    ],
  },
  {
    title: "CONTROLS",
    items: [
      { id: "controls", label: "Controls", icon: Gamepad2 },
      { id: "ffb", label: "FFB & LUT", icon: Sliders },
      { id: "rigs", label: "Rig Profiles", icon: Layers },
    ],
  },
  {
    title: "SIMULATION",
    items: [
      { id: "audio", label: "Audio", icon: Volume2 },
      { id: "gameplay", label: "Gameplay", icon: Wrench },
      { id: "snapshots", label: "Snapshots", icon: History },
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
  const [category, setCategory] = useState<CategoryId>("video");
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

  // Local camera / audio / gameplay states
  const [fovDeg, setFovDeg] = useState(54);
  const [replayQuality, setReplayQuality] = useState<"Medium" | "High" | "Ultra">("High");
  const [masterVol, setMasterVol] = useState(85);
  const [engineVol, setEngineVol] = useState(92);
  const [tyreVol, setTyreVol] = useState(78);
  const [autoBlip, setAutoBlip] = useState(false);
  const [idealLine, setIdealLine] = useState(false);

  useEffect(() => {
    void Promise.all([
      api.getVideoSettings().catch(() => null),
      api.getVrTripleSettings().catch(() => null),
      api.getFfbConfig().catch(() => null),
      api.listRigProfiles().catch(() => []),
      api.listConfigSnapshots().catch(() => []),
    ]).then(([v, vrRes, ffbRes, rigList, snapList]) => {
      if (v) {
        const raw = v as unknown as Record<string, unknown>;
        const normalizedVideo: VideoSettings = {
          ...DEFAULT_VIDEO,
          width: Number(raw.width ?? DEFAULT_VIDEO.width),
          height: Number(raw.height ?? DEFAULT_VIDEO.height),
          refreshHz: Number(raw.refreshHz ?? raw.refresh ?? DEFAULT_VIDEO.refreshHz),
          fullscreen: Boolean(raw.fullscreen ?? DEFAULT_VIDEO.fullscreen),
          vsync: Boolean(raw.vsync ?? DEFAULT_VIDEO.vsync),
          fpsCap: Number(raw.fpsCap ?? raw.fps_cap ?? DEFAULT_VIDEO.fpsCap),
          msaa: Number(raw.msaa ?? raw.aa_samples ?? DEFAULT_VIDEO.msaa),
          anisotropic: Number(raw.anisotropic ?? DEFAULT_VIDEO.anisotropic),
          shadowMapSize: Number(raw.shadowMapSize ?? raw.shadow_map_size ?? DEFAULT_VIDEO.shadowMapSize),
          worldDetail: Number(raw.worldDetail ?? raw.world_detail ?? DEFAULT_VIDEO.worldDetail),
          postProcessing: Boolean(raw.postProcessing ?? raw.post_process ?? DEFAULT_VIDEO.postProcessing),
          ppFilter: String(raw.ppFilter ?? raw.pp_filter ?? DEFAULT_VIDEO.ppFilter),
        };
        setVideo(normalizedVideo);
        setSavedVideo(normalizedVideo);
      }
      if (vrRes) {
        const raw = vrRes as unknown as Record<string, unknown>;
        const triple = (raw.triple_screen ?? raw.tripleScreen ?? {}) as Record<string, unknown>;
        const openvr = (raw.openvr ?? raw.oculus ?? {}) as Record<string, unknown>;
        const normalizedVr: VrTripleSettings = {
          ...DEFAULT_VR,
          renderMode: (raw.renderMode as VrTripleSettings["renderMode"]) ?? DEFAULT_VR.renderMode,
          tripleFovDeg: Number(raw.tripleFovDeg ?? triple.fov_deg ?? DEFAULT_VR.tripleFovDeg),
          screenWidthMm: Number(raw.screenWidthMm ?? triple.width_mm ?? DEFAULT_VR.screenWidthMm),
          distanceMm: Number(raw.distanceMm ?? triple.distance_mm ?? DEFAULT_VR.distanceMm),
          bezelMm: Number(raw.bezelMm ?? triple.bezel_mm ?? DEFAULT_VR.bezelMm),
          sideAngleDeg: Number(raw.sideAngleDeg ?? triple.angle_deg ?? DEFAULT_VR.sideAngleDeg),
          vrPixelDensity: Number(raw.vrPixelDensity ?? openvr.pixel_per_display ?? DEFAULT_VR.vrPixelDensity),
        };
        setVr(normalizedVr);
        setSavedVr(normalizedVr);
      }
      if (ffbRes) {
        const raw = ffbRes as unknown as Record<string, unknown>;
        const pp = (raw.post_process ?? raw.postProcess ?? {}) as Record<string, unknown>;
        const normalizedFfb: FfbConfig = {
          ...DEFAULT_FFB,
          gain: Number(raw.gain ?? DEFAULT_FFB.gain),
          filter: Number(raw.filter ?? DEFAULT_FFB.filter),
          minForce: Number(raw.minForce ?? DEFAULT_FFB.minForce),
          kerbEffect: Number(raw.kerbEffect ?? DEFAULT_FFB.kerbEffect),
          roadEffect: Number(raw.roadEffect ?? DEFAULT_FFB.roadEffect),
          slipEffect: Number(raw.slipEffect ?? DEFAULT_FFB.slipEffect),
          absEffect: Number(raw.absEffect ?? DEFAULT_FFB.absEffect),
          enhancedUndersteer: Boolean(raw.enhancedUndersteer ?? DEFAULT_FFB.enhancedUndersteer),
          gammaEnabled: Boolean(raw.gammaEnabled ?? pp.enabled ?? DEFAULT_FFB.gammaEnabled),
          gammaValue: Number(raw.gammaValue ?? pp.gamma ?? DEFAULT_FFB.gammaValue),
        };
        setFfb(normalizedFfb);
        setSavedFfb(normalizedFfb);
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
      await Promise.all([
        api.saveVideoSettings(video),
        api.saveVrTripleSettings(vr),
        api.saveFfbConfig(ffb),
      ]);
      setSavedVideo(video);
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
    setVr(next);
    try {
      await api.saveVrTripleSettings(next);
      setSavedVr(next);
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
    <div className="p-6 max-w-[1620px] mx-auto pb-24 space-y-5 animate-fade-in">
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
        <Card className="lg:col-span-3 space-y-5" padding="sm">
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
                    className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-[13px] font-medium transition-all cursor-pointer ${
                      active
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
                        : "text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.04] border border-transparent"
                    }`}
                  >
                    <Icon className="w-4 h-4 shrink-0" />
                    <span>{item.label}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </Card>

        {/* Full-Width 2-Column Content Area */}
        <div className="lg:col-span-9 space-y-5">
          {/* VIDEO */}
          {category === "video" && (
            <div className="grid grid-cols-1 xl:grid-cols-12 gap-5">
              <Card className="xl:col-span-7 space-y-4" padding="md">
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

                <Slider
                  label="MSAA Samples"
                  value={video.msaa}
                  min={0}
                  max={8}
                  step={2}
                  formatValue={(v) => (v === 0 ? "Off" : `${v}x`)}
                  onChange={(v) => setVideo({ ...video, msaa: v })}
                />
                <Slider
                  label="Anisotropic Filtering"
                  value={video.anisotropic}
                  min={0}
                  max={16}
                  step={4}
                  formatValue={(v) => (v === 0 ? "Off" : `${v}x`)}
                  onChange={(v) => setVideo({ ...video, anisotropic: v })}
                />
                <Slider
                  label="World Detail"
                  value={video.worldDetail}
                  min={1}
                  max={5}
                  step={1}
                  formatValue={(v) => ["Very Low", "Low", "Medium", "High", "Maximum"][v - 1]}
                  onChange={(v) => setVideo({ ...video, worldDetail: v })}
                />
                <Slider
                  label="Shadow Map Resolution"
                  value={video.shadowMapSize}
                  min={512}
                  max={4096}
                  step={512}
                  unit=" px"
                  onChange={(v) => setVideo({ ...video, shadowMapSize: v })}
                />
              </Card>

              {/* Right: Live Visual Quality vs Performance Bars */}
              <Card className="xl:col-span-5 space-y-5" padding="md">
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
          )}

          {/* VR & TRIPLE */}
          {category === "vr-triple" && (
            <div className="space-y-4">
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
            </div>
          )}

          {/* FFB & LUT */}
          {category === "ffb" && (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
              <Card padding="md" className="space-y-4">
                <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Force Feedback Core Gain & Effects
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
                  Post-Processing & Gamma / LUT Curve
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
          )}

          {/* RIG PROFILES */}
          {category === "rigs" && (
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
          )}

          {/* SNAPSHOTS */}
          {category === "snapshots" && (
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
          )}

          {/* CAMERA / CONTROLS / AUDIO / GAMEPLAY */}
          {(category === "camera" ||
            category === "controls" ||
            category === "audio" ||
            category === "gameplay") && (
            <Card padding="md" className="grid grid-cols-1 md:grid-cols-2 gap-6">
              {category === "camera" && (
                <>
                  <div className="space-y-4">
                    <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                      Cockpit Camera
                    </div>
                    <Slider label="Vertical FOV" value={fovDeg} min={32} max={80} unit="°" onChange={setFovDeg} />
                  </div>
                  <div className="space-y-3">
                    <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                      Replay Recording Quality
                    </div>
                    <div className="flex gap-2">
                      {(["Medium", "High", "Ultra"] as const).map((q) => (
                        <Button
                          key={q}
                          size="sm"
                          variant={replayQuality === q ? "primary" : "secondary"}
                          onClick={() => setReplayQuality(q)}
                        >
                          {q}
                        </Button>
                      ))}
                    </div>
                  </div>
                </>
              )}

              {category === "controls" && (
                <>
                  <div className="space-y-2">
                    <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                      Active Wheel & Pedal Bindings
                    </div>
                    <div className="text-sm font-semibold text-[var(--text-primary)]">
                      DirectInput / Wheel Profile Synced
                    </div>
                    <p className="text-xs text-[var(--text-secondary)]">
                      Bind axes, shift paddles, DRS, and pit limiter directly inside Assetto Corsa or switch saved Rig Profiles.
                    </p>
                  </div>
                  <div className="flex items-center justify-end">
                    <Button variant="secondary" size="sm" onClick={() => setCategory("ffb")}>
                      Tune FFB & LUT Curve
                    </Button>
                  </div>
                </>
              )}

              {category === "audio" && (
                <>
                  <div className="space-y-4">
                    <Slider label="Master Volume" value={masterVol} min={0} max={100} unit="%" onChange={setMasterVol} />
                    <Slider label="Engine & Exhaust" value={engineVol} min={0} max={100} unit="%" onChange={setEngineVol} />
                  </div>
                  <div className="space-y-4">
                    <Slider label="Tyre Scrub & Slip" value={tyreVol} min={0} max={100} unit="%" onChange={setTyreVol} />
                  </div>
                </>
              )}

              {category === "gameplay" && (
                <>
                  <Toggle
                    checked={autoBlip}
                    onChange={setAutoBlip}
                    label="Auto-Blip on Downshift"
                    description="Automatically rev-match H-pattern manual gearboxes."
                  />
                  <Toggle
                    checked={idealLine}
                    onChange={setIdealLine}
                    label="Show On-Track Ideal Line"
                    description="Display dynamic braking & racing line overlay in-sim."
                  />
                </>
              )}
            </Card>
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

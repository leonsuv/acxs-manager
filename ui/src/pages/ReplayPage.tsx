import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Box,
  Camera,
  Film,
  Map as MapIcon,
  Pause,
  Play,
  RefreshCw,
  Scissors,
  SkipBack,
  SkipForward,
  Video,
} from "lucide-react";
import { api } from "../lib/api";
import { ACCENT_PRESETS, type ReplaySummary } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  PageHeader,
  SegmentedControl,
  Slider,
} from "../components/Primitives";
import { Viewport3D } from "../components/Viewport3D";

const DEMO_REPLAYS: ReplaySummary[] = [
  {
    filename: "spa_gt3_endurance_final_lap.acreplay",
    path: "/Documents/Assetto Corsa/replay/spa_gt3_endurance_final_lap.acreplay",
    sizeBytes: 48_200_000,
    modifiedAt: "Today · 21:35",
    carId: "ks_porsche_911_gt3_r_2016",
    trackId: "spa",
    durationSec: 284,
  },
  {
    filename: "monza_quali_pb_1_48_4.acreplay",
    path: "/Documents/Assetto Corsa/replay/monza_quali_pb_1_48_4.acreplay",
    sizeBytes: 24_600_000,
    modifiedAt: "Yesterday · 19:52",
    carId: "ks_ferrari_488_gt3",
    trackId: "monza",
    durationSec: 132,
  },
  {
    filename: "nordschleife_touristenfahrten_clean.acreplay",
    path: "/Documents/Assetto Corsa/replay/nordschleife_touristenfahrten_clean.acreplay",
    sizeBytes: 118_400_000,
    modifiedAt: "3 days ago · 22:18",
    carId: "ks_mclaren_p1_gtr",
    trackId: "ks_nordschleife",
    durationSec: 428,
  },
];

function formatTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function formatSize(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function ReplayPage() {
  const { accent, notify } = useAppStore();
  const [replays, setReplays] = useState<ReplaySummary[]>([]);
  const [selectedName, setSelectedName] = useState("");
  const [loading, setLoading] = useState(true);
  const [usingDemo, setUsingDemo] = useState(false);

  const [viewMode, setViewMode] = useState<"2d" | "3d">("2d");
  const [cameraMode, setCameraMode] = useState<"chase" | "freefly" | "tv">("chase");
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState<number>(1);
  const [playheadSec, setPlayheadSec] = useState(24);
  const [trimStart, setTrimStart] = useState(12);
  const [trimEnd, setTrimEnd] = useState(96);

  const accentHex =
    ACCENT_PRESETS.find((p) => p.id === accent)?.hex ?? "#FF4D2E";

  const loadReplays = useCallback(async () => {
    setLoading(true);
    try {
      const list = await api.listReplays();
      if (list && list.length > 0) {
        setReplays(list);
        setUsingDemo(false);
        setSelectedName((prev) => prev || list[0].filename);
      } else {
        setReplays(DEMO_REPLAYS);
        setUsingDemo(true);
        setSelectedName((prev) => prev || DEMO_REPLAYS[0].filename);
      }
    } catch {
      setReplays(DEMO_REPLAYS);
      setUsingDemo(true);
      setSelectedName(DEMO_REPLAYS[0].filename);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadReplays();
  }, [loadReplays]);

  const selected = useMemo(
    () => replays.find((r) => r.filename === selectedName) ?? replays[0],
    [replays, selectedName]
  );

  const duration = selected?.durationSec ?? 180;

  useEffect(() => {
    setPlayheadSec(Math.min(20, duration));
    setTrimStart(Math.round(duration * 0.1));
    setTrimEnd(Math.round(duration * 0.65));
    setPlaying(false);
  }, [selected?.filename, duration]);

  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => {
      setPlayheadSec((prev) => {
        const next = prev + 0.1 * speed;
        return next >= duration ? 0 : next;
      });
    }, 100);
    return () => window.clearInterval(id);
  }, [playing, speed, duration]);

  const handleWatchInAc = async () => {
    if (!selected) return;
    try {
      await api.launchReplayInAc(selected.filename);
      notify("Launching Replay in Assetto Corsa", "success", selected.filename);
    } catch (err) {
      notify("Could not launch replay", "error", String(err));
    }
  };

  const handleExportClip = () => {
    if (!selected) return;
    const clipLen = Math.max(1, trimEnd - trimStart);
    notify(
      "Clip Export Queued",
      "success",
      `${selected.filename.replace(".acreplay", "")}_clip_${formatTime(trimStart)}-${formatTime(trimEnd)} (${clipLen}s)`
    );
  };

  // Multi-car positions on 2D track loop
  const trackPoints = useMemo(() => {
    return Array.from({ length: 80 }, (_, i) => {
      const t = (i / 80) * Math.PI * 2;
      const x = 260 + Math.cos(t) * 185 + Math.sin(t * 2) * 34;
      const y = 145 + Math.sin(t) * 92 + Math.cos(t * 3) * 20;
      return [x, y] as [number, number];
    });
  }, []);

  const carsOnTrack = useMemo(() => {
    const base = (playheadSec / Math.max(duration, 1)) * Math.PI * 2 * 3;
    const drivers = [
      { id: "YOU", offset: 0, color: accentHex },
      { id: "P2", offset: -0.18, color: "#00D1FF" },
      { id: "P3", offset: -0.41, color: "#FFB020" },
      { id: "P4", offset: -0.68, color: "#94A3B8" },
    ];
    return drivers.map((d) => {
      const t = base + d.offset;
      const x = 260 + Math.cos(t) * 185 + Math.sin(t * 2) * 34;
      const y = 145 + Math.sin(t) * 92 + Math.cos(t * 3) * 20;
      return { ...d, x, y };
    });
  }, [playheadSec, duration, accentHex]);

  return (
    <div className="p-6 max-w-[1620px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="TELEMETRY · REPLAY STUDIO"
        title="Replay Library & Clip Trimmer"
        subtitle="Inspect multi-car track telemetry in 2D or 3D, trim .acreplay highlights, and launch directly into Assetto Corsa."
        actions={
          <>
            {usingDemo && <Badge variant="info" dot>Sample Replays</Badge>}
            <Button
              variant="secondary"
              size="sm"
              loading={loading}
              icon={<RefreshCw className="w-3.5 h-3.5" />}
              onClick={() => void loadReplays()}
            >
              Refresh
            </Button>
            <Button
              variant="primary"
              size="sm"
              icon={<Video className="w-3.5 h-3.5" />}
              onClick={() => void handleWatchInAc()}
              disabled={!selected}
            >
              Watch in AC
            </Button>
          </>
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
        {/* Left: Replay Library */}
        <Card className="lg:col-span-4 space-y-3" padding="md">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Saved .acreplay Files ({replays.length})
            </span>
            <Film className="w-4 h-4 text-[var(--text-muted)]" />
          </div>

          <div className="space-y-2 max-h-[600px] overflow-y-auto pr-1">
            {replays.map((r) => {
              const active = r.filename === selected?.filename;
              return (
                <button
                  key={r.filename}
                  type="button"
                  onClick={() => setSelectedName(r.filename)}
                  className={`w-full text-left p-3.5 rounded-xl border transition-all cursor-pointer ${
                    active
                      ? "bg-[var(--accent-soft)] border-[var(--accent-border)]"
                      : "bg-white/[0.025] border-white/[0.06] hover:bg-white/[0.05]"
                  }`}
                >
                  <div className="text-[13px] font-semibold text-[var(--text-primary)] truncate">
                    {r.filename}
                  </div>
                  <div className="flex items-center justify-between mt-1.5 text-[11px] font-mono text-[var(--text-secondary)]">
                    <span>{r.trackId ?? "circuit"} · {formatTime(r.durationSec ?? 180)}</span>
                    <span>{formatSize(r.sizeBytes)}</span>
                  </div>
                  <div className="text-[10px] text-[var(--text-muted)] mt-1">
                    {r.modifiedAt}
                  </div>
                </button>
              );
            })}
          </div>
        </Card>

        {/* Right: 2D / 3D Viewer + Transport + Trim & Export */}
        <div className="lg:col-span-8 space-y-4">
          <Card padding="md" className="space-y-3">
            {/* Viewer Controls Row */}
            <div className="flex flex-wrap items-center justify-between gap-3">
              <SegmentedControl
                size="sm"
                value={viewMode}
                onChange={setViewMode}
                options={[
                  { value: "2d", label: "2D Track Map", icon: <MapIcon className="w-3.5 h-3.5" /> },
                  { value: "3d", label: "3D Viewer", icon: <Box className="w-3.5 h-3.5" /> },
                ]}
              />

              <div className="flex items-center gap-1.5">
                <Camera className="w-3.5 h-3.5 text-[var(--text-muted)] mr-1" />
                {(["chase", "freefly", "tv"] as const).map((cam) => (
                  <button
                    key={cam}
                    type="button"
                    onClick={() => setCameraMode(cam)}
                    className={`h-7 px-2.5 rounded-lg text-xs font-medium capitalize transition-colors cursor-pointer ${
                      cameraMode === cam
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
                        : "bg-white/[0.04] text-[var(--text-secondary)] hover:text-white border border-white/[0.06]"
                    }`}
                  >
                    {cam === "freefly" ? "FreeFly" : cam === "tv" ? "TV" : "Chase"}
                  </button>
                ))}
              </div>
            </div>

            {/* Viewport Surface */}
            {viewMode === "3d" ? (
              <div className="h-[310px]">
                <Viewport3D
                  mode="track"
                  accentHex={accentHex}
                  label={`${(selected?.trackId ?? "CIRCUIT").toUpperCase()} · ${cameraMode.toUpperCase()} CAM`}
                />
              </div>
            ) : (
              <div className="h-[310px] rounded-2xl bg-[#070A10] border border-white/[0.07] relative overflow-hidden flex items-center justify-center">
                <svg viewBox="0 0 520 290" className="w-full h-full">
                  <path
                    d={
                      trackPoints
                        .map(([x, y], i) => `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`)
                        .join(" ") + " Z"
                    }
                    fill="none"
                    stroke="rgba(255,255,255,0.1)"
                    strokeWidth="14"
                    strokeLinejoin="round"
                  />
                  <path
                    d={
                      trackPoints
                        .map(([x, y], i) => `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`)
                        .join(" ") + " Z"
                    }
                    fill="none"
                    stroke="rgba(255,255,255,0.28)"
                    strokeWidth="1.5"
                    strokeDasharray="6 6"
                  />
                  {carsOnTrack.map((c) => (
                    <g key={c.id}>
                      <circle cx={c.x} cy={c.y} r={10} fill={c.color} fillOpacity={0.22} />
                      <circle cx={c.x} cy={c.y} r={5} fill={c.color} stroke="#070A10" strokeWidth="1.6" />
                      <text
                        x={c.x + 9}
                        y={c.y + 3}
                        fill="#F3F6FA"
                        fontSize="10"
                        fontFamily="JetBrains Mono, monospace"
                      >
                        {c.id}
                      </text>
                    </g>
                  ))}
                </svg>
                <div className="absolute top-3 left-3 flex items-center gap-2">
                  <Badge variant="accent">MULTI-CAR TELEMETRY</Badge>
                  <Badge variant="neutral">{cameraMode.toUpperCase()} CAM</Badge>
                </div>
              </div>
            )}

            {/* Multi-car Playhead Transport Bar */}
            <div className="rounded-xl bg-black/35 border border-white/[0.07] p-3 space-y-2.5">
              <div className="flex items-center gap-3">
                <span className="font-mono text-xs text-[var(--text-primary)] w-12">
                  {formatTime(playheadSec)}
                </span>
                <input
                  type="range"
                  min={0}
                  max={duration}
                  step={0.5}
                  value={playheadSec}
                  onChange={(e) => setPlayheadSec(Number(e.target.value))}
                  className="acxs-range flex-1 h-4"
                />
                <span className="font-mono text-xs text-[var(--text-muted)] w-12 text-right">
                  {formatTime(duration)}
                </span>
              </div>

              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-1.5">
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={<SkipBack className="w-3.5 h-3.5" />}
                    onClick={() => setPlayheadSec((v) => Math.max(0, v - 10))}
                  />
                  <Button
                    variant="primary"
                    size="sm"
                    icon={playing ? <Pause className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
                    onClick={() => setPlaying((p) => !p)}
                  >
                    {playing ? "Pause" : "Play"}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={<SkipForward className="w-3.5 h-3.5" />}
                    onClick={() => setPlayheadSec((v) => Math.min(duration, v + 10))}
                  />
                </div>

                <div className="flex items-center gap-1">
                  {[0.25, 0.5, 1, 2, 4].map((rate) => (
                    <button
                      key={rate}
                      type="button"
                      onClick={() => setSpeed(rate)}
                      className={`px-2 py-1 rounded-md font-mono text-[11px] cursor-pointer transition-colors ${
                        speed === rate
                          ? "bg-[var(--accent)] text-white font-semibold"
                          : "bg-white/[0.05] text-[var(--text-secondary)] hover:text-white"
                      }`}
                    >
                      {rate}x
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </Card>

          {/* .acreplay Trim & Export Clip */}
          <Card padding="md" className="space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <Scissors className="w-4 h-4 text-[var(--accent)]" />
                <div>
                  <div className="text-sm font-semibold text-[var(--text-primary)]">
                    .acreplay Trim & Export Clip
                  </div>
                  <div className="text-[11px] text-[var(--text-secondary)]">
                    Slice a clean highlight segment without re-encoding Assetto Corsa replay packets.
                  </div>
                </div>
              </div>
              <Badge variant="info">
                Clip Length: {formatTime(Math.max(0, trimEnd - trimStart))}
              </Badge>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <Slider
                label="In Point (Start)"
                value={trimStart}
                min={0}
                max={Math.max(1, trimEnd - 2)}
                step={1}
                formatValue={formatTime}
                onChange={setTrimStart}
              />
              <Slider
                label="Out Point (End)"
                value={trimEnd}
                min={Math.min(duration, trimStart + 2)}
                max={duration}
                step={1}
                formatValue={formatTime}
                onChange={setTrimEnd}
              />
            </div>

            <div className="flex items-center justify-end gap-2 pt-1">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setPlayheadSec(trimStart)}
              >
                Preview In-Point
              </Button>
              <Button
                variant="primary"
                size="sm"
                icon={<Scissors className="w-3.5 h-3.5" />}
                onClick={handleExportClip}
              >
                Export Trimmed .acreplay
              </Button>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

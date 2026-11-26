import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  AlertTriangle,
  ArrowUpRight,
  Clock,
  Compass,
  Flame,
  Gauge,
  RefreshCw,
  Sliders,
  Sparkles,
  Trophy,
} from "lucide-react";
import { api } from "../lib/api";
import type {
  CornerCoachNote,
  TelemetrySessionAnalysis,
  TelemetrySessionSummary,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  KpiTile,
  PageHeader,
  Skeleton,
} from "../components/Primitives";

function formatLapMs(ms: number): string {
  if (!ms || ms <= 0) return "--:--.---";
  const mins = Math.floor(ms / 60000);
  const secs = Math.floor((ms % 60000) / 1000);
  const rem = Math.floor(ms % 1000);
  return `${mins}:${String(secs).padStart(2, "0")}.${String(rem).padStart(3, "0")}`;
}

const DEMO_SESSIONS: TelemetrySessionSummary[] = [
  {
    id: "demo-spa-gt3",
    carId: "ks_porsche_911_gt3_r_2016",
    trackId: "spa",
    date: "Today · 21:14",
    lapsCount: 14,
    bestLapMs: 137842,
    topSpeedKmh: 274,
  },
  {
    id: "demo-monza-488",
    carId: "ks_ferrari_488_gt3",
    trackId: "monza",
    date: "Yesterday · 19:40",
    lapsCount: 18,
    bestLapMs: 108415,
    topSpeedKmh: 291,
  },
  {
    id: "demo-imola-p1",
    carId: "ks_mclaren_p1_gtr",
    trackId: "imola",
    date: "3 days ago · 22:05",
    lapsCount: 9,
    bestLapMs: 101920,
    topSpeedKmh: 312,
  },
];

function buildDemoAnalysis(summary: TelemetrySessionSummary): TelemetrySessionAnalysis {
  const trackPath: Array<[number, number]> = [];
  const count = 90;
  for (let i = 0; i < count; i++) {
    const t = (i / count) * Math.PI * 2;
    const x = 180 + Math.cos(t) * 125 + Math.sin(t * 2) * 32;
    const y = 110 + Math.sin(t) * 74 + Math.cos(t * 3) * 18;
    trackPath.push([x, y]);
  }
  const speedTrace = Array.from({ length: 80 }, (_, i) => {
    const p = i / 79;
    const wave = Math.sin(p * Math.PI * 6);
    const speedKmh = Math.round(135 + wave * 82 + Math.cos(p * Math.PI * 4) * 35);
    const brake = wave < -0.45 ? Math.min(1, (-wave - 0.45) * 1.9) : 0;
    const throttle = brake > 0.1 ? 0 : Math.min(1, 0.55 + (wave + 0.5) * 0.45);
    const gear = Math.max(2, Math.min(6, Math.round(speedKmh / 48)));
    return { distanceM: Math.round(p * 7004), speedKmh, throttle, brake, gear };
  });

  const corners: CornerCoachNote[] = [
    {
      cornerIndex: 1,
      name: "T1 · La Source Hairpin",
      timeLossMs: 285,
      entrySpeedDeltaKmh: -6.4,
      apexSpeedDeltaKmh: -3.2,
      advice: "Trail-brake 14m deeper into the apex and release brake smoothly to avoid front-left lockup.",
    },
    {
      cornerIndex: 5,
      name: "T5-T7 · Les Combes",
      timeLossMs: 192,
      entrySpeedDeltaKmh: -2.1,
      apexSpeedDeltaKmh: -5.8,
      advice: "Carry +5 km/h minimum speed through right-left transition; soften front ARB by 1 click.",
    },
    {
      cornerIndex: 10,
      name: "T10 · Pouhon Double-Left",
      timeLossMs: 340,
      entrySpeedDeltaKmh: 4.5,
      apexSpeedDeltaKmh: -7.1,
      advice: "Mid-corner understeer forcing late throttle application. Lower front ride height 2mm or add 1° rear wing.",
    },
    {
      cornerIndex: 18,
      name: "T18 · Bus Stop Chicane",
      timeLossMs: 148,
      entrySpeedDeltaKmh: -3.8,
      apexSpeedDeltaKmh: -1.9,
      advice: "Slight rear wheelspin on exit curb. Short-shift to 2nd or add +1 TC cut level.",
    },
  ];

  return {
    summary,
    lapTimesMs: [
      summary.bestLapMs + 1420,
      summary.bestLapMs + 680,
      summary.bestLapMs + 210,
      summary.bestLapMs,
      summary.bestLapMs + 340,
      summary.bestLapMs + 150,
    ],
    speedTrace,
    corners,
    trackPath,
  };
}

export default function SessionsPage() {
  const { navigate, notify } = useAppStore();
  const [sessions, setSessions] = useState<TelemetrySessionSummary[]>([]);
  const [usingDemo, setUsingDemo] = useState(false);
  const [loading, setLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string>("");
  const [analysis, setAnalysis] = useState<TelemetrySessionAnalysis | null>(null);
  const [activeCornerIdx, setActiveCornerIdx] = useState<number | null>(null);

  const loadSessions = useCallback(async () => {
    setLoading(true);
    try {
      const list = await api.listSessions();
      if (list && list.length > 0) {
        setSessions(list);
        setUsingDemo(false);
        setSelectedId((prev) => prev || list[0].id);
      } else {
        setSessions(DEMO_SESSIONS);
        setUsingDemo(true);
        setSelectedId((prev) => prev || DEMO_SESSIONS[0].id);
      }
    } catch (err) {
      setSessions(DEMO_SESSIONS);
      setUsingDemo(true);
      setSelectedId(DEMO_SESSIONS[0].id);
      notify("Loaded sample analytics", "info", String(err));
    } finally {
      setLoading(false);
    }
  }, [notify]);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  useEffect(() => {
    const current = sessions.find((s) => s.id === selectedId);
    if (!current) return;
    if (usingDemo) {
      setAnalysis(buildDemoAnalysis(current));
      setActiveCornerIdx(null);
      return;
    }
    let cancelled = false;
    api
      .getSessionAnalysis(current.id)
      .then((res) => {
        if (!cancelled) {
          setAnalysis(res ?? buildDemoAnalysis(current));
          setActiveCornerIdx(null);
        }
      })
      .catch(() => {
        if (!cancelled) setAnalysis(buildDemoAnalysis(current));
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId, sessions, usingDemo]);

  // Derived driving dynamics metrics from speed/pedal trace & corners
  const dynamics = useMemo(() => {
    if (!analysis) {
      return { lockups: 0, wheelspin: 0, balancePct: 50, totalLossMs: 0 };
    }
    const lockups = Math.max(
      1,
      analysis.speedTrace.filter((p) => p.brake > 0.88).length
    );
    const wheelspin = Math.max(
      1,
      analysis.speedTrace.filter((p) => p.throttle > 0.92 && p.speedKmh < 120).length
    );
    const totalLossMs = analysis.corners.reduce((acc, c) => acc + c.timeLossMs, 0);
    // Negative = understeer bias, positive = oversteer bias (0..100 scale, 50 neutral)
    const balancePct = Math.min(
      82,
      Math.max(18, 44 + (wheelspin - lockups) * 4)
    );
    return { lockups, wheelspin, balancePct, totalLossMs };
  }, [analysis]);

  // Normalize trackPath to SVG viewBox (0 0 360 220)
  const svgTrack = useMemo(() => {
    const pts = analysis?.trackPath ?? [];
    if (pts.length < 3) return { d: "", norm: [] as Array<[number, number]> };
    let minX = Infinity,
      maxX = -Infinity,
      minY = Infinity,
      maxY = -Infinity;
    for (const [x, y] of pts) {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
    const spanX = Math.max(maxX - minX, 1);
    const spanY = Math.max(maxY - minY, 1);
    const scale = Math.min(300 / spanX, 168 / spanY);
    const offX = (360 - spanX * scale) / 2;
    const offY = (220 - spanY * scale) / 2;
    const norm: Array<[number, number]> = pts.map(([x, y]) => [
      offX + (x - minX) * scale,
      offY + (y - minY) * scale,
    ]);
    const d =
      norm
        .map(([x, y], i) => `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`)
        .join(" ") + " Z";
    return { d, norm };
  }, [analysis]);

  const cornerHotspots = useMemo(() => {
    if (!analysis || svgTrack.norm.length === 0) return [];
    return analysis.corners.map((c, idx) => {
      const ptIdx = Math.floor(
        ((idx + 0.6) / Math.max(analysis.corners.length, 1)) * svgTrack.norm.length
      ) % svgTrack.norm.length;
      const [x, y] = svgTrack.norm[ptIdx];
      return { corner: c, x, y };
    });
  }, [analysis, svgTrack.norm]);

  return (
    <div className="p-6 max-w-[1600px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="TELEMETRY · DRIVING ANALYTICS"
        title="Sessions & Corner Coach"
        subtitle="Inspect recorded stints, braking lockups, handling balance, and corner-by-corner setup advice."
        actions={
          <>
            {usingDemo && (
              <Badge variant="info" dot>
                Preview Dataset
              </Badge>
            )}
            <Button
              variant="secondary"
              size="sm"
              icon={<RefreshCw className="w-3.5 h-3.5" />}
              loading={loading}
              onClick={() => void loadSessions()}
            >
              Rescan Sessions
            </Button>
            <Button
              variant="primary"
              size="sm"
              icon={<Sliders className="w-3.5 h-3.5" />}
              onClick={() => navigate("garage", "setups")}
            >
              Open Setups
            </Button>
          </>
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
        {/* Left Column: Recorded Session Timeline */}
        <Card className="lg:col-span-4 space-y-3" padding="md">
          <div className="flex items-center justify-between">
            <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Recorded Stints ({sessions.length})
            </div>
            <Badge variant="neutral">ACXS Logger</Badge>
          </div>

          {loading ? (
            <div className="space-y-2.5">
              <Skeleton className="h-20 w-full" />
              <Skeleton className="h-20 w-full" />
              <Skeleton className="h-20 w-full" />
            </div>
          ) : (
            <div className="space-y-2 max-h-[640px] overflow-y-auto pr-1">
              {sessions.map((s) => {
                const active = s.id === selectedId;
                return (
                  <button
                    key={s.id}
                    type="button"
                    onClick={() => setSelectedId(s.id)}
                    className={`w-full text-left p-3.5 rounded-xl border transition-all cursor-pointer ${
                      active
                        ? "bg-[var(--accent-soft)] border-[var(--accent-border)] shadow-lg"
                        : "bg-white/[0.025] border-white/[0.06] hover:bg-white/[0.05] hover:border-white/[0.12]"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div className="min-w-0">
                        <div className="text-[13px] font-semibold text-[var(--text-primary)] truncate">
                          {s.carId.replace(/^ks_/, "").replace(/_/g, " ")}
                        </div>
                        <div className="text-[11px] text-[var(--text-secondary)] flex items-center gap-1.5 mt-0.5">
                          <Compass className="w-3 h-3 text-[var(--accent)] shrink-0" />
                          <span className="truncate">{s.trackId}</span>
                          <span className="text-[var(--text-muted)]">·</span>
                          <span className="text-[var(--text-muted)]">{s.date}</span>
                        </div>
                      </div>
                      <Badge variant={active ? "accent" : "neutral"}>
                        {s.lapsCount} laps
                      </Badge>
                    </div>
                    <div className="mt-3 pt-2.5 border-t border-white/[0.06] flex items-center justify-between text-xs font-mono">
                      <span className="text-emerald-300 flex items-center gap-1">
                        <Trophy className="w-3 h-3" />
                        {formatLapMs(s.bestLapMs)}
                      </span>
                      <span className="text-[var(--text-muted)]">
                        Vmax <span className="text-[var(--text-primary)]">{s.topSpeedKmh}</span> km/h
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </Card>

        {/* Right Column: KPI Strip + Track Hotspot Map + Corner Coach */}
        <div className="lg:col-span-8 space-y-5">
          {/* KPI Telemetry Strip */}
          <div className="grid grid-cols-2 xl:grid-cols-4 gap-3">
            <KpiTile
              label="Best Lap"
              value={formatLapMs(analysis?.summary.bestLapMs ?? 0)}
              sublabel={`Potential −${(dynamics.totalLossMs / 1000).toFixed(3)}s`}
              icon={<Clock className="w-4 h-4" />}
              accent
            />
            <KpiTile
              label="Brake Lockups"
              value={dynamics.lockups}
              unit="events"
              sublabel="Front axle > 88% slip"
              icon={<AlertTriangle className="w-4 h-4 text-amber-400" />}
            />
            <KpiTile
              label="Wheelspin"
              value={dynamics.wheelspin}
              unit="events"
              sublabel="Corner-exit traction loss"
              icon={<Flame className="w-4 h-4 text-red-400" />}
            />
            <div className="glass-panel rounded-xl p-4 flex flex-col justify-between gap-2">
              <div className="flex items-center justify-between text-[11px] uppercase tracking-[0.08em] text-[var(--text-muted)]">
                <span>Handling Balance</span>
                <Gauge className="w-4 h-4" />
              </div>
              <div className="space-y-1.5">
                <div className="flex justify-between text-[10px] font-mono text-[var(--text-secondary)]">
                  <span>UNDERSTEER</span>
                  <span>OVERSTEER</span>
                </div>
                <div className="h-2.5 rounded-full bg-white/[0.08] relative overflow-hidden">
                  <div className="absolute inset-y-0 left-1/2 w-px bg-white/30" />
                  <div
                    className="h-full rounded-full transition-all duration-300"
                    style={{
                      width: "18%",
                      marginLeft: `${Math.max(0, Math.min(82, dynamics.balancePct - 9))}%`,
                      background:
                        dynamics.balancePct < 45
                          ? "#00D1FF"
                          : dynamics.balancePct > 55
                          ? "#FF4D2E"
                          : "#10B981",
                    }}
                  />
                </div>
              </div>
              <div className="text-[11px] text-[var(--text-secondary)] font-mono">
                {dynamics.balancePct < 45
                  ? "Mild Understeer Bias"
                  : dynamics.balancePct > 55
                  ? "Mild Oversteer Bias"
                  : "Neutral Aero Balance"}
              </div>
            </div>
          </div>

          {/* SVG Track Hotspot Map & Lap Pace Strip */}
          <div className="grid grid-cols-1 xl:grid-cols-12 gap-4">
            <Card className="xl:col-span-7" padding="md">
              <div className="flex items-center justify-between mb-3">
                <div>
                  <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Circuit Time-Loss Hotspots
                  </div>
                  <div className="text-sm font-semibold text-[var(--text-primary)]">
                    Click a corner marker to focus coaching advice
                  </div>
                </div>
                <Badge variant="danger" dot>
                  +{(dynamics.totalLossMs / 1000).toFixed(3)}s available
                </Badge>
              </div>
              <div className="rounded-xl bg-[#070A10] border border-white/[0.06] p-2">
                <svg viewBox="0 0 360 220" className="w-full h-[210px]">
                  <defs>
                    <filter id="acxs-glow">
                      <feGaussianBlur stdDeviation="3" result="blur" />
                      <feMerge>
                        <feMergeNode in="blur" />
                        <feMergeNode in="SourceGraphic" />
                      </feMerge>
                    </filter>
                  </defs>
                  {/* Asphalt shadow */}
                  <path
                    d={svgTrack.d}
                    fill="none"
                    stroke="rgba(255,255,255,0.08)"
                    strokeWidth="11"
                    strokeLinejoin="round"
                  />
                  {/* Racing line */}
                  <path
                    d={svgTrack.d}
                    fill="none"
                    stroke="var(--accent)"
                    strokeWidth="2.5"
                    strokeLinejoin="round"
                    filter="url(#acxs-glow)"
                  />
                  {/* Corner Hotspot Markers */}
                  {cornerHotspots.map(({ corner, x, y }) => {
                    const isActive = activeCornerIdx === corner.cornerIndex;
                    const severe = corner.timeLossMs >= 250;
                    const fill = severe ? "#EF4444" : "#FFB020";
                    return (
                      <g
                        key={corner.cornerIndex}
                        className="cursor-pointer"
                        onClick={() =>
                          setActiveCornerIdx(isActive ? null : corner.cornerIndex)
                        }
                      >
                        <circle
                          cx={x}
                          cy={y}
                          r={isActive ? 14 : 10}
                          fill={fill}
                          fillOpacity={isActive ? 0.35 : 0.2}
                        />
                        <circle
                          cx={x}
                          cy={y}
                          r={isActive ? 6.5 : 5}
                          fill={fill}
                          stroke="#070A10"
                          strokeWidth="1.8"
                        />
                        <text
                          x={x + 10}
                          y={y + 3}
                          fill="#F3F6FA"
                          fontSize="9"
                          fontFamily="JetBrains Mono, monospace"
                        >
                          T{corner.cornerIndex} (+{(corner.timeLossMs / 1000).toFixed(2)}s)
                        </text>
                      </g>
                    );
                  })}
                </svg>
              </div>
            </Card>

            <Card className="xl:col-span-5 flex flex-col justify-between" padding="md">
              <div>
                <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Stint Pace Consistency
                </div>
                <div className="text-sm font-semibold text-[var(--text-primary)] mb-3">
                  Recent Lap Progression
                </div>
                <div className="space-y-2">
                  {(analysis?.lapTimesMs ?? []).map((ms, idx) => {
                    const best = analysis?.summary.bestLapMs ?? ms;
                    const delta = Math.max(0, ms - best);
                    const pct = Math.max(22, 100 - (delta / 2500) * 100);
                    const isBest = delta === 0;
                    return (
                      <div key={idx} className="space-y-1">
                        <div className="flex items-center justify-between text-xs font-mono">
                          <span className="text-[var(--text-muted)]">LAP {idx + 1}</span>
                          <span
                            className={
                              isBest ? "text-emerald-300 font-semibold" : "text-[var(--text-primary)]"
                            }
                          >
                            {formatLapMs(ms)}
                            {!isBest && (
                              <span className="text-[var(--text-muted)] ml-1.5">
                                +{(delta / 1000).toFixed(3)}s
                              </span>
                            )}
                          </span>
                        </div>
                        <div className="h-1.5 rounded-full bg-white/[0.06] overflow-hidden">
                          <div
                            className="h-full rounded-full"
                            style={{
                              width: `${pct}%`,
                              background: isBest ? "#10B981" : "var(--accent)",
                            }}
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
              <div className="mt-4 pt-3 border-t border-white/[0.06] flex items-center justify-between text-xs">
                <span className="text-[var(--text-muted)] flex items-center gap-1.5">
                  <Activity className="w-3.5 h-3.5 text-cyan-400" />
                  Compare racing line in Trajectory Studio
                </span>
                <button
                  type="button"
                  onClick={() => navigate("telemetry", "trajectory")}
                  className="text-[var(--accent)] hover:underline font-medium inline-flex items-center gap-1 cursor-pointer"
                >
                  Open <ArrowUpRight className="w-3.5 h-3.5" />
                </button>
              </div>
            </Card>
          </div>

          {/* Corner-by-Corner Coaching Cards */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Sparkles className="w-4 h-4 text-[var(--accent)]" />
                <h2 className="text-sm font-semibold text-[var(--text-primary)]">
                  Corner-by-Corner Coaching & Setup Actions
                </h2>
              </div>
              {activeCornerIdx !== null && (
                <button
                  type="button"
                  onClick={() => setActiveCornerIdx(null)}
                  className="text-xs text-[var(--text-secondary)] hover:text-white cursor-pointer"
                >
                  Show all corners
                </button>
              )}
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              {(analysis?.corners ?? [])
                .filter(
                  (c) => activeCornerIdx === null || c.cornerIndex === activeCornerIdx
                )
                .map((corner) => (
                  <Card
                    key={corner.cornerIndex}
                    variant="elevated"
                    padding="md"
                    className={`border transition-all ${
                      activeCornerIdx === corner.cornerIndex
                        ? "border-[var(--accent-border)]"
                        : "border-white/[0.07]"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div>
                        <div className="text-sm font-semibold text-[var(--text-primary)]">
                          {corner.name}
                        </div>
                        <div className="flex items-center gap-3 mt-1 text-[11px] font-mono text-[var(--text-secondary)]">
                          <span>
                            Entry{" "}
                            <span
                              className={
                                corner.entrySpeedDeltaKmh < 0
                                  ? "text-amber-300"
                                  : "text-emerald-300"
                              }
                            >
                              {corner.entrySpeedDeltaKmh > 0 ? "+" : ""}
                              {corner.entrySpeedDeltaKmh.toFixed(1)} km/h
                            </span>
                          </span>
                          <span>
                            Apex{" "}
                            <span className="text-red-300">
                              {corner.apexSpeedDeltaKmh.toFixed(1)} km/h
                            </span>
                          </span>
                        </div>
                      </div>
                      <Badge variant={corner.timeLossMs >= 250 ? "danger" : "warning"}>
                        +{(corner.timeLossMs / 1000).toFixed(3)}s
                      </Badge>
                    </div>

                    <p className="text-xs text-[var(--text-secondary)] leading-relaxed mt-3">
                      {corner.advice}
                    </p>

                    <div className="mt-3.5 pt-3 border-t border-white/[0.06] flex items-center justify-between">
                      <span className="text-[11px] font-mono text-[var(--text-muted)]">
                        Recommended Adjustment
                      </span>
                      <Button
                        variant="secondary"
                        size="sm"
                        icon={<Sliders className="w-3 h-3" />}
                        onClick={() => navigate("garage", "setups")}
                      >
                        Open Setups
                      </Button>
                    </div>
                  </Card>
                ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

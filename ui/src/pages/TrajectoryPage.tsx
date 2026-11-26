import { useMemo, useState } from "react";
import { Box, GitCompare, Map as MapIcon, TrendingDown } from "lucide-react";
import { ACCENT_PRESETS } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Card,
  PageHeader,
  SegmentedControl,
} from "../components/Primitives";
import { Viewport3D } from "../components/Viewport3D";

interface LapOption {
  id: string;
  label: string;
  timeStr: string;
  timeMs: number;
  offsetSeed: number;
}

const LAP_OPTIONS: LapOption[] = [
  { id: "lap-4", label: "Lap 4 (PB)", timeStr: "2:17.842", timeMs: 137842, offsetSeed: 0 },
  { id: "lap-6", label: "Lap 6 (Race)", timeStr: "2:18.194", timeMs: 138194, offsetSeed: 1 },
  { id: "lap-2", label: "Lap 2 (Outlap+1)", timeStr: "2:18.610", timeMs: 138610, offsetSeed: 2 },
  { id: "ai-ref", label: "fast_lane.ai Reference", timeStr: "2:17.310", timeMs: 137310, offsetSeed: -1 },
];

const CORNERS = [
  { name: "T1 · La Source", entryDelta: "+1.8", apexDelta: "-2.4", exitDelta: "-3.1", lossMs: 118, note: "Slightly early turn-in pinches exit" },
  { name: "T3-T4 · Eau Rouge / Raidillon", entryDelta: "+0.4", apexDelta: "+1.2", exitDelta: "+2.0", lossMs: -42, note: "Flat-out line gained 0.04s" },
  { name: "T5-T7 · Les Combes", entryDelta: "-3.2", apexDelta: "-4.1", exitDelta: "-1.8", lossMs: 164, note: "Late brake release costs mid-corner rotation" },
  { name: "T10 · Pouhon", entryDelta: "+2.5", apexDelta: "-5.3", exitDelta: "-4.0", lossMs: 215, note: "Drifted 1.4m wide of ideal fast_lane.ai apex" },
  { name: "T18 · Bus Stop Chicane", entryDelta: "-1.1", apexDelta: "-0.8", exitDelta: "-2.2", lossMs: 77, note: "Hesitation on final throttle pick-up" },
];

export default function TrajectoryPage() {
  const { accent, armedSession } = useAppStore();
  const [viewMode, setViewMode] = useState<"svg" | "3d">("svg");
  const [colorMode, setColorMode] = useState<"pedals" | "speed" | "delta">("pedals");
  const [lapAId, setLapAId] = useState("lap-4");
  const [lapBId, setLapBId] = useState("ai-ref");
  const [scrubPct, setScrubPct] = useState(32);

  const accentHex =
    ACCENT_PRESETS.find((p) => p.id === accent)?.hex ?? "#FF4D2E";

  const lapA = LAP_OPTIONS.find((l) => l.id === lapAId) ?? LAP_OPTIONS[0];
  const lapB = LAP_OPTIONS.find((l) => l.id === lapBId) ?? LAP_OPTIONS[3];
  const totalDeltaMs = lapA.timeMs - lapB.timeMs;

  // Build corridor & racing-line points
  const samples = useMemo(() => {
    const count = 96;
    return Array.from({ length: count }, (_, i) => {
      const p = i / count;
      const t = p * Math.PI * 2;
      const cx = 270 + Math.cos(t) * 195 + Math.sin(t * 2) * 34;
      const cy = 148 + Math.sin(t) * 92 + Math.cos(t * 3) * 18;
      const speedA = 140 + Math.sin(p * Math.PI * 6) * 76 + Math.cos(p * Math.PI * 4) * 34;
      const speedB = speedA + Math.sin(p * Math.PI * 5 + lapB.offsetSeed) * 9;
      const brakeA = Math.sin(p * Math.PI * 6) < -0.55 ? 0.9 : 0;
      const throttleA = brakeA > 0 ? 0 : Math.min(1, 0.55 + Math.sin(p * Math.PI * 6) * 0.45);
      const delta = (speedA - speedB) * 0.04;
      return { p, cx, cy, speedA, speedB, throttleA, brakeA, delta };
    });
  }, [lapB.offsetSeed]);

  const corridorPath = useMemo(
    () =>
      samples
        .map((s, i) => `${i === 0 ? "M" : "L"} ${s.cx.toFixed(1)} ${s.cy.toFixed(1)}`)
        .join(" ") + " Z",
    [samples]
  );

  const scrubIdx = Math.min(
    samples.length - 1,
    Math.floor((scrubPct / 100) * samples.length)
  );
  const activeSample = samples[scrubIdx];

  const segmentColor = (s: (typeof samples)[number]) => {
    if (colorMode === "pedals") {
      if (s.brakeA > 0.2) return "#EF4444";
      if (s.throttleA > 0.7) return "#10B981";
      return "#FFB020";
    }
    if (colorMode === "speed") {
      if (s.speedA > 215) return "#00D1FF";
      if (s.speedA > 145) return "#38BDF8";
      return "#F59E0B";
    }
    return s.delta >= 0 ? "#10B981" : "#EF4444";
  };

  return (
    <div className="p-6 max-w-[1640px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="TELEMETRY · RACING-LINE STUDIO"
        title="Trajectory & Racing-Line Studio"
        subtitle={`${armedSession.trackName} · Compare fast_lane.ai corridor against recorded laps in 2D or 3D.`}
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <SegmentedControl
              size="sm"
              value={viewMode}
              onChange={setViewMode}
              options={[
                { value: "svg", label: "Corridor SVG", icon: <MapIcon className="w-3.5 h-3.5" /> },
                { value: "3d", label: "3D Ribbon", icon: <Box className="w-3.5 h-3.5" /> },
              ]}
            />
            <SegmentedControl
              size="sm"
              value={colorMode}
              onChange={setColorMode}
              options={[
                { value: "pedals", label: "Pedals" },
                { value: "speed", label: "Speed" },
                { value: "delta", label: "Delta" },
              ]}
            />
          </div>
        }
      />

      {/* Lap A vs Lap B Comparison Bar */}
      <Card padding="sm" className="flex flex-wrap items-center justify-between gap-4 px-4">
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="w-2.5 h-2.5 rounded-full bg-cyan-400" />
            <span className="text-xs font-mono text-[var(--text-muted)]">LAP A</span>
            <select
              value={lapAId}
              onChange={(e) => setLapAId(e.target.value)}
              className="bg-black/40 border border-white/[0.1] rounded-lg px-2.5 py-1.5 text-xs font-mono text-[var(--text-primary)]"
            >
              {LAP_OPTIONS.map((l) => (
                <option key={l.id} value={l.id} className="bg-[#0D1118]">
                  {l.label} — {l.timeStr}
                </option>
              ))}
            </select>
          </div>

          <GitCompare className="w-4 h-4 text-[var(--text-muted)]" />

          <div className="flex items-center gap-2">
            <span className="w-2.5 h-2.5 rounded-full bg-amber-400" />
            <span className="text-xs font-mono text-[var(--text-muted)]">LAP B</span>
            <select
              value={lapBId}
              onChange={(e) => setLapBId(e.target.value)}
              className="bg-black/40 border border-white/[0.1] rounded-lg px-2.5 py-1.5 text-xs font-mono text-[var(--text-primary)]"
            >
              {LAP_OPTIONS.map((l) => (
                <option key={l.id} value={l.id} className="bg-[#0D1118]">
                  {l.label} — {l.timeStr}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="flex items-center gap-3 font-mono text-xs">
          <span className="text-[var(--text-muted)]">LAP DELTA:</span>
          <Badge variant={totalDeltaMs <= 0 ? "success" : "danger"}>
            {totalDeltaMs > 0 ? "+" : ""}
            {(totalDeltaMs / 1000).toFixed(3)}s
          </Badge>
        </div>
      </Card>

      {/* Main Corridor / 3D Ribbon + Synced Trace */}
      <div className="grid grid-cols-1 xl:grid-cols-12 gap-5">
        <Card className="xl:col-span-7 space-y-3" padding="md">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              fast_lane.ai Corridor & Racing Line ({colorMode.toUpperCase()})
            </span>
            <span className="text-xs font-mono text-[var(--text-secondary)]">
              Cursor: {Math.round((scrubPct / 100) * 7004)} m · {Math.round(activeSample.speedA)} km/h
            </span>
          </div>

          {viewMode === "3d" ? (
            <div className="h-[320px]">
              <Viewport3D
                mode="track"
                accentHex={accentHex}
                label={`3D RACING RIBBON · ${colorMode.toUpperCase()}`}
              />
            </div>
          ) : (
            <div className="h-[320px] rounded-2xl bg-[#070A10] border border-white/[0.07] p-2">
              <svg viewBox="0 0 540 296" className="w-full h-full">
                {/* Track asphalt corridor */}
                <path
                  d={corridorPath}
                  fill="none"
                  stroke="rgba(255,255,255,0.09)"
                  strokeWidth="22"
                  strokeLinejoin="round"
                />
                {/* fast_lane.ai dashed reference */}
                <path
                  d={corridorPath}
                  fill="none"
                  stroke="rgba(255,255,255,0.22)"
                  strokeWidth="1.2"
                  strokeDasharray="4 4"
                />
                {/* Colored trajectory segments */}
                {samples.map((s, i) => {
                  const next = samples[(i + 1) % samples.length];
                  return (
                    <line
                      key={i}
                      x1={s.cx}
                      y1={s.cy}
                      x2={next.cx}
                      y2={next.cy}
                      stroke={segmentColor(s)}
                      strokeWidth="3.5"
                      strokeLinecap="round"
                    />
                  );
                })}
                {/* Synced scrub cursor */}
                <circle
                  cx={activeSample.cx}
                  cy={activeSample.cy}
                  r={10}
                  fill="var(--accent)"
                  fillOpacity={0.28}
                />
                <circle
                  cx={activeSample.cx}
                  cy={activeSample.cy}
                  r={4.5}
                  fill="#fff"
                  stroke="var(--accent)"
                  strokeWidth="2"
                />
              </svg>
            </div>
          )}

          {/* Synced Distance Scrubber */}
          <div className="flex items-center gap-3 pt-1">
            <span className="text-[11px] font-mono text-[var(--text-muted)]">SCRUB LAP</span>
            <input
              type="range"
              min={0}
              max={99}
              value={scrubPct}
              onChange={(e) => setScrubPct(Number(e.target.value))}
              className="acxs-range flex-1 h-4"
            />
            <span className="text-[11px] font-mono text-[var(--text-primary)] w-12 text-right">
              {scrubPct}%
            </span>
          </div>
        </Card>

        {/* Synced Speed & Pedal SVG Traces */}
        <Card className="xl:col-span-5 flex flex-col justify-between gap-3" padding="md">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Synced Speed & Pedal Traces
            </span>
            <div className="flex items-center gap-3 text-[10px] font-mono">
              <span className="text-cyan-300">— Lap A</span>
              <span className="text-amber-300">- - Lap B</span>
            </div>
          </div>

          {/* Speed Comparison Trace */}
          <div className="rounded-xl bg-[#070A10] border border-white/[0.06] p-2">
            <div className="text-[10px] font-mono text-[var(--text-muted)] mb-1">
              SPEED (KM/H)
            </div>
            <svg viewBox="0 0 400 120" preserveAspectRatio="none" className="w-full h-[120px]">
              <path
                d={samples
                  .map((s, i) => {
                    const x = (i / (samples.length - 1)) * 400;
                    const y = 112 - (s.speedB / 300) * 102;
                    return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
                  })
                  .join(" ")}
                fill="none"
                stroke="#FFB020"
                strokeWidth="1.6"
                strokeDasharray="4 3"
              />
              <path
                d={samples
                  .map((s, i) => {
                    const x = (i / (samples.length - 1)) * 400;
                    const y = 112 - (s.speedA / 300) * 102;
                    return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
                  })
                  .join(" ")}
                fill="none"
                stroke="#00D1FF"
                strokeWidth="2.2"
              />
              <line
                x1={(scrubPct / 100) * 400}
                y1="0"
                x2={(scrubPct / 100) * 400}
                y2="120"
                stroke="var(--accent)"
                strokeWidth="1.5"
              />
            </svg>
          </div>

          {/* Throttle / Brake Trace */}
          <div className="rounded-xl bg-[#070A10] border border-white/[0.06] p-2">
            <div className="text-[10px] font-mono text-[var(--text-muted)] mb-1">
              THROTTLE (GREEN) / BRAKE (RED)
            </div>
            <svg viewBox="0 0 400 100" preserveAspectRatio="none" className="w-full h-[100px]">
              <path
                d={samples
                  .map((s, i) => {
                    const x = (i / (samples.length - 1)) * 400;
                    const y = 92 - s.throttleA * 82;
                    return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
                  })
                  .join(" ")}
                fill="none"
                stroke="#10B981"
                strokeWidth="2"
              />
              <path
                d={samples
                  .map((s, i) => {
                    const x = (i / (samples.length - 1)) * 400;
                    const y = 92 - s.brakeA * 82;
                    return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
                  })
                  .join(" ")}
                fill="none"
                stroke="#EF4444"
                strokeWidth="2"
              />
              <line
                x1={(scrubPct / 100) * 400}
                y1="0"
                x2={(scrubPct / 100) * 400}
                y2="100"
                stroke="var(--accent)"
                strokeWidth="1.5"
              />
            </svg>
          </div>
        </Card>
      </div>

      {/* Corner-by-Corner Time-Loss Breakdown Table */}
      <Card padding="none" className="overflow-hidden">
        <div className="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between">
          <div className="flex items-center gap-2">
            <TrendingDown className="w-4 h-4 text-[var(--accent)]" />
            <span className="text-sm font-semibold text-[var(--text-primary)]">
              Corner-by-Corner Time-Loss Breakdown
            </span>
          </div>
          <Badge variant="neutral">{CORNERS.length} Key Sectors</Badge>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-left border-collapse text-xs">
            <thead>
              <tr className="border-b border-white/[0.06] text-[10px] font-mono uppercase tracking-[0.1em] text-[var(--text-muted)] bg-white/[0.02]">
                <th className="py-2.5 px-4">Corner</th>
                <th className="py-2.5 px-3">Entry Δ</th>
                <th className="py-2.5 px-3">Apex Δ</th>
                <th className="py-2.5 px-3">Exit Δ</th>
                <th className="py-2.5 px-3">Time Δ</th>
                <th className="py-2.5 px-4">Line Analysis</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/[0.05]">
              {CORNERS.map((c) => (
                <tr key={c.name} className="hover:bg-white/[0.025] transition-colors">
                  <td className="py-3 px-4 font-semibold text-[var(--text-primary)]">
                    {c.name}
                  </td>
                  <td className="py-3 px-3 font-mono text-[var(--text-secondary)]">
                    {c.entryDelta} km/h
                  </td>
                  <td className="py-3 px-3 font-mono text-[var(--text-secondary)]">
                    {c.apexDelta} km/h
                  </td>
                  <td className="py-3 px-3 font-mono text-[var(--text-secondary)]">
                    {c.exitDelta} km/h
                  </td>
                  <td className="py-3 px-3 font-mono">
                    <Badge variant={c.lossMs <= 0 ? "success" : c.lossMs > 150 ? "danger" : "warning"}>
                      {c.lossMs > 0 ? "+" : ""}
                      {(c.lossMs / 1000).toFixed(3)}s
                    </Badge>
                  </td>
                  <td className="py-3 px-4 text-[var(--text-secondary)]">{c.note}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}

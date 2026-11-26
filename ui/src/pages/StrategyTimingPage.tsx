import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronUp,
  Clock,
  FileSpreadsheet,
  Flag,
  Fuel,
  Gauge,
  RefreshCw,
  Timer,
  Trophy,
  Upload,
} from "lucide-react";
import { api } from "../lib/api";
import type { StrategyPlan, TimingRow } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  KpiTile,
  PageHeader,
  SegmentedControl,
  Slider,
} from "../components/Primitives";

function formatMs(ms: number): string {
  if (!ms || ms <= 0) return "--:--.---";
  const m = Math.floor(ms / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  const rem = Math.floor(ms % 1000);
  return `${m}:${String(s).padStart(2, "0")}.${String(rem).padStart(3, "0")}`;
}

function formatSectorMs(ms?: number): string {
  if (!ms || ms <= 0) return "--.---";
  return (ms / 1000).toFixed(3);
}

function formatDuration(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  return h > 0
    ? `${h}h ${String(m).padStart(2, "0")}m ${String(s).padStart(2, "0")}s`
    : `${m}m ${String(s).padStart(2, "0")}s`;
}

const DEMO_TIMING: TimingRow[] = [
  { position: 1, driver: "L. Suvorkov", carId: "ks_porsche_911_gt3_r_2016", bestLapMs: 137842, lastLapMs: 138110, gapMs: 0, laps: 14, sector1Ms: 41210, sector2Ms: 56980, sector3Ms: 39652 },
  { position: 2, driver: "M. Rossi", carId: "ks_ferrari_488_gt3", bestLapMs: 138095, lastLapMs: 138420, gapMs: 1240, laps: 14, sector1Ms: 41340, sector2Ms: 57115, sector3Ms: 39640 },
  { position: 3, driver: "K. Takahashi", carId: "ks_porsche_911_gt3_r_2016", bestLapMs: 138410, lastLapMs: 138605, gapMs: 3890, laps: 14, sector1Ms: 41490, sector2Ms: 57280, sector3Ms: 39640 },
  { position: 4, driver: "J. Müller", carId: "ks_mclaren_p1_gtr", bestLapMs: 138920, lastLapMs: 139310, gapMs: 6520, laps: 14, sector1Ms: 41610, sector2Ms: 57420, sector3Ms: 39890 },
  { position: 5, driver: "S. Leclerc", carId: "ks_ferrari_488_gt3", bestLapMs: 139184, lastLapMs: 139540, gapMs: 9180, laps: 14, sector1Ms: 41720, sector2Ms: 57554, sector3Ms: 39910 },
];

const DEMO_PERSONAL_BESTS = [
  { track: "Spa-Francorchamps", car: "Porsche 911 GT3 R", s1: 41180, s2: 56840, s3: 39510, bestLap: 137842, date: "Today" },
  { track: "Monza GP", car: "Ferrari 488 GT3", s1: 35210, s2: 38640, s3: 34280, bestLap: 108415, date: "Yesterday" },
  { track: "Imola GP", car: "McLaren P1 GTR", s1: 29440, s2: 37810, s3: 34390, bestLap: 101920, date: "3 days ago" },
  { track: "Nürburgring Nordschleife", car: "Mazda 787B", s1: 118400, s2: 142900, s3: 112600, bestLap: 375120, date: "Last week" },
];

export default function StrategyTimingPage() {
  const { notify } = useAppStore();
  const [tab, setTab] = useState<"strategy" | "timing" | "laptimes">("strategy");

  // Strategy Sliders (sensible defaults so visuals paint immediately)
  const [fuelPerLap, setFuelPerLap] = useState(3.2);
  const [tankCapacity, setTankCapacity] = useState(110);
  const [raceLaps, setRaceLaps] = useState(36);
  const [tyreLife, setTyreLife] = useState(15);
  const [minStint, setMinStint] = useState(8);
  const [lapTimeSec, setLapTimeSec] = useState(138);
  const [pitLossSec, setPitLossSec] = useState(28);
  const [backendPlan, setBackendPlan] = useState<StrategyPlan | null>(null);

  // Timing Tower state
  const [timingRows, setTimingRows] = useState<TimingRow[]>([]);
  const [timingLoading, setTimingLoading] = useState(false);
  const [usingDemoTiming, setUsingDemoTiming] = useState(false);
  const [csvDrawerOpen, setCsvDrawerOpen] = useState(false);
  const [csvInput, setCsvInput] = useState("");

  // Compute local plan immediately + sync with backend when available
  const localPlan = useMemo<StrategyPlan>(() => {
    const totalFuelL = Number((raceLaps * fuelPerLap).toFixed(1));
    const maxFuelLaps = Math.max(1, Math.floor(tankCapacity / Math.max(fuelPerLap, 0.1)));
    const maxStint = Math.max(minStint, Math.min(maxFuelLaps, tyreLife));
    const stopsRequired = Math.max(0, Math.ceil(raceLaps / maxStint) - 1);
    const stintCount = stopsRequired + 1;
    const baseStint = Math.floor(raceLaps / stintCount);
    const remainder = raceLaps % stintCount;
    const stintLengths = Array.from({ length: stintCount }, (_, i) =>
      baseStint + (i < remainder ? 1 : 0)
    );
    const recommendedPitLaps: number[] = [];
    let acc = 0;
    for (let i = 0; i < stintLengths.length - 1; i++) {
      acc += stintLengths[i];
      recommendedPitLaps.push(acc);
    }
    const fuelPerMaxStint = Math.max(...stintLengths) * fuelPerLap;
    const fuelMarginL = Number((tankCapacity - fuelPerMaxStint).toFixed(1));
    const estimatedRaceTimeSec = raceLaps * lapTimeSec + stopsRequired * pitLossSec;
    return {
      totalFuelL,
      stopsRequired,
      recommendedPitLaps,
      stintLengths,
      estimatedRaceTimeSec,
      fuelMarginL,
    };
  }, [fuelPerLap, tankCapacity, raceLaps, tyreLife, minStint, lapTimeSec, pitLossSec]);

  useEffect(() => {
    let cancelled = false;
    api
      .computeStrategy({
        raceLaps,
        lapTimeSec,
        fuelPerLapL: fuelPerLap,
        tankCapacityL: tankCapacity,
        pitLossSec,
        tyreLifeLaps: tyreLife,
      })
      .then((res) => {
        if (!cancelled && res && res.stintLengths?.length > 0) {
          setBackendPlan(res);
        } else if (!cancelled) {
          setBackendPlan(null);
        }
      })
      .catch(() => {
        if (!cancelled) setBackendPlan(null);
      });
    return () => {
      cancelled = true;
    };
  }, [raceLaps, lapTimeSec, fuelPerLap, tankCapacity, pitLossSec, tyreLife]);

  const plan = backendPlan ?? localPlan;

  const loadTiming = useCallback(
    async (csv?: string | null) => {
      setTimingLoading(true);
      try {
        const rows = await api.loadTimingTower(csv ?? null);
        if (rows && rows.length > 0) {
          setTimingRows(rows);
          setUsingDemoTiming(false);
          if (csv) notify("Imported CSV timing table", "success", `${rows.length} drivers`);
        } else if (csv) {
          // Local CSV parse fallback for browser preview
          const parsed = csv
            .split(/\r?\n/)
            .map((l) => l.trim())
            .filter(Boolean)
            .slice(1)
            .map((line, i) => {
              const cols = line.split(",");
              return {
                position: i + 1,
                driver: cols[0]?.trim() || `Driver ${i + 1}`,
                carId: cols[1]?.trim() || "gt3_car",
                bestLapMs: Number(cols[2]) || 138000 + i * 350,
                lastLapMs: Number(cols[3]) || 138400 + i * 350,
                gapMs: i * 1450,
                laps: Number(cols[4]) || 12,
              } as TimingRow;
            });
          if (parsed.length > 0) {
            setTimingRows(parsed);
            setUsingDemoTiming(false);
            notify("Parsed CSV timing", "success", `${parsed.length} rows`);
          }
        } else {
          setTimingRows(DEMO_TIMING);
          setUsingDemoTiming(true);
        }
      } catch (err) {
        setTimingRows(DEMO_TIMING);
        setUsingDemoTiming(true);
        notify("Showing broadcast timing preview", "info", String(err));
      } finally {
        setTimingLoading(false);
      }
    },
    [notify]
  );

  // Auto-load race_out.json on mount
  useEffect(() => {
    void loadTiming(null);
  }, [loadTiming]);

  const maxStintFuel = Math.max(...plan.stintLengths, 1) * fuelPerLap;
  const tankFillPct = Math.min(100, Math.round((maxStintFuel / Math.max(tankCapacity, 1)) * 100));

  return (
    <div className="p-6 max-w-[1620px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="TELEMETRY · RACE OPERATIONS"
        title="Strategy & Timing Tower"
        subtitle="Fuel & pit-window calculator, F1-style Broadcast Timing Tower, and personal best sector records."
        actions={
          <SegmentedControl
            value={tab}
            onChange={setTab}
            options={[
              { value: "strategy", label: "Race Strategy", icon: <Fuel className="w-3.5 h-3.5" /> },
              { value: "timing", label: "Broadcast Timing Tower", icon: <Flag className="w-3.5 h-3.5" /> },
              { value: "laptimes", label: "Lap Times", icon: <Trophy className="w-3.5 h-3.5" /> },
            ]}
          />
        }
      />

      {/* TAB 1: RACE STRATEGY */}
      {tab === "strategy" && (
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
          {/* Left: Tactile Strategy Sliders */}
          <Card className="lg:col-span-4 space-y-4" padding="md">
            <div className="flex items-center justify-between">
              <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                Race Parameters
              </span>
              <Badge variant="accent">LIVE SOLVER</Badge>
            </div>

            <Slider
              label="Fuel Burn (L/lap)"
              value={fuelPerLap}
              min={0.8}
              max={7.5}
              step={0.1}
              formatValue={(v) => `${v.toFixed(1)} L`}
              onChange={setFuelPerLap}
            />
            <Slider
              label="Tank Capacity"
              value={tankCapacity}
              min={30}
              max={150}
              step={1}
              unit=" L"
              onChange={setTankCapacity}
            />
            <Slider
              label="Race Distance"
              value={raceLaps}
              min={5}
              max={100}
              step={1}
              unit=" laps"
              onChange={setRaceLaps}
            />
            <Slider
              label="Tyre Life Window"
              value={tyreLife}
              min={4}
              max={45}
              step={1}
              unit=" laps"
              onChange={setTyreLife}
            />
            <Slider
              label="Minimum Stint"
              value={minStint}
              min={3}
              max={Math.max(4, tyreLife)}
              step={1}
              unit=" laps"
              onChange={setMinStint}
            />
            <div className="grid grid-cols-2 gap-3 pt-2 border-t border-white/[0.06]">
              <Slider
                label="Avg Lap"
                value={lapTimeSec}
                min={60}
                max={240}
                step={1}
                formatValue={(v) => `${Math.floor(v / 60)}:${String(v % 60).padStart(2, "0")}`}
                onChange={setLapTimeSec}
              />
              <Slider
                label="Pit Loss"
                value={pitLossSec}
                min={12}
                max={60}
                step={1}
                unit=" s"
                onChange={setPitLossSec}
              />
            </div>
          </Card>

          {/* Right: Fuel Telemetry Bar + Pit Window Timeline + Stop Plan Table */}
          <div className="lg:col-span-8 space-y-4">
            <div className="grid grid-cols-2 xl:grid-cols-4 gap-3">
              <KpiTile
                label="Total Fuel Needed"
                value={plan.totalFuelL.toFixed(1)}
                unit="L"
                sublabel={`${fuelPerLap.toFixed(1)} L × ${raceLaps} laps`}
                icon={<Fuel className="w-4 h-4" />}
                accent
              />
              <KpiTile
                label="Pit Stops"
                value={plan.stopsRequired}
                unit={plan.stopsRequired === 1 ? "stop" : "stops"}
                sublabel={
                  plan.recommendedPitLaps.length > 0
                    ? `Box Laps: ${plan.recommendedPitLaps.join(", ")}`
                    : "No-stop run viable"
                }
                icon={<Flag className="w-4 h-4" />}
              />
              <KpiTile
                label="Est. Race Time"
                value={formatDuration(plan.estimatedRaceTimeSec)}
                sublabel={`Incl. ${plan.stopsRequired * pitLossSec}s pit lane`}
                icon={<Clock className="w-4 h-4" />}
              />
              <KpiTile
                label="Tank Margin"
                value={`${plan.fuelMarginL >= 0 ? "+" : ""}${plan.fuelMarginL.toFixed(1)}`}
                unit="L"
                sublabel={plan.fuelMarginL >= 2 ? "Safe stint reserve" : "Tight fuel window"}
                icon={<Gauge className="w-4 h-4" />}
              />
            </div>

            {/* Painted Fuel Telemetry Bar */}
            <Card padding="md" className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Peak Stint Fuel Telemetry Bar
                </span>
                <span className="text-xs font-mono text-[var(--text-primary)]">
                  {maxStintFuel.toFixed(1)} L / {tankCapacity} L ({tankFillPct}%)
                </span>
              </div>
              <div className="h-6 rounded-xl bg-black/45 border border-white/[0.08] p-1 relative overflow-hidden">
                <div
                  className="h-full rounded-lg transition-all duration-300"
                  style={{
                    width: `${tankFillPct}%`,
                    background:
                      tankFillPct > 94
                        ? "linear-gradient(90deg, #F59E0B 0%, #EF4444 100%)"
                        : "linear-gradient(90deg, #10B981 0%, #00D1FF 100%)",
                  }}
                />
              </div>
            </Card>

            {/* Painted Pit Window Timeline Bar */}
            <Card padding="md" className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Pit Window Timeline ({raceLaps} Laps)
                </span>
                <span className="text-xs font-mono text-[var(--text-secondary)]">
                  {plan.stintLengths.length} Stints
                </span>
              </div>
              <div className="flex items-center gap-1.5 h-11">
                {plan.stintLengths.map((len, idx) => {
                  const colors = ["#00D1FF", "#10B981", "#FFB020", "#FF4D2E", "#A855F7"];
                  const col = colors[idx % colors.length];
                  return (
                    <div
                      key={idx}
                      style={{ flexGrow: len }}
                      className="h-full rounded-xl border border-white/10 flex flex-col items-center justify-center relative overflow-hidden"
                    >
                      <div
                        className="absolute inset-0 opacity-20"
                        style={{ background: col }}
                      />
                      <span className="relative text-[11px] font-mono font-semibold text-white">
                        S{idx + 1} · {len}L
                      </span>
                      <span className="relative text-[10px] font-mono text-white/70">
                        {(len * fuelPerLap).toFixed(1)}L
                      </span>
                    </div>
                  );
                })}
              </div>
            </Card>

            {/* Stop Plan Table */}
            <Card padding="none" className="overflow-hidden">
              <div className="px-4 py-3 border-b border-white/[0.06] text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                Stint & Stop Plan
              </div>
              <table className="w-full text-left border-collapse text-xs">
                <thead>
                  <tr className="border-b border-white/[0.06] text-[10px] font-mono uppercase text-[var(--text-muted)] bg-white/[0.02]">
                    <th className="py-2.5 px-4">Stint</th>
                    <th className="py-2.5 px-3">Lap Range</th>
                    <th className="py-2.5 px-3">Laps</th>
                    <th className="py-2.5 px-3">Fuel Load</th>
                    <th className="py-2.5 px-4">Pit Action</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/[0.05] font-mono">
                  {plan.stintLengths.map((len, idx) => {
                    const startLap =
                      plan.stintLengths.slice(0, idx).reduce((a, b) => a + b, 0) + 1;
                    const endLap = startLap + len - 1;
                    const isFinal = idx === plan.stintLengths.length - 1;
                    return (
                      <tr key={idx} className="hover:bg-white/[0.02]">
                        <td className="py-2.5 px-4 font-semibold text-[var(--text-primary)]">
                          Stint {idx + 1}
                        </td>
                        <td className="py-2.5 px-3 text-[var(--text-secondary)]">
                          L{startLap} – L{endLap}
                        </td>
                        <td className="py-2.5 px-3 text-[var(--text-primary)]">{len}</td>
                        <td className="py-2.5 px-3 text-cyan-300">
                          {(len * fuelPerLap + 1.2).toFixed(1)} L
                        </td>
                        <td className="py-2.5 px-4">
                          {isFinal ? (
                            <Badge variant="success">Finish Flag</Badge>
                          ) : (
                            <Badge variant="warning">Box Lap {endLap} · Tyres + Fuel</Badge>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </Card>
          </div>
        </div>
      )}

      {/* TAB 2: BROADCAST TIMING TOWER */}
      {tab === "timing" && (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Badge variant={usingDemoTiming ? "info" : "success"} dot>
                {usingDemoTiming ? "Preview Grid (race_out.json not found)" : "Loaded from race_out.json"}
              </Badge>
              <span className="text-xs font-mono text-[var(--text-muted)]">
                {timingRows.length} classified cars
              </span>
            </div>

            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                loading={timingLoading}
                icon={<RefreshCw className="w-3.5 h-3.5" />}
                onClick={() => void loadTiming(null)}
              >
                Load race_out.json
              </Button>
              <Button
                variant="secondary"
                size="sm"
                icon={<FileSpreadsheet className="w-3.5 h-3.5" />}
                onClick={() => setCsvDrawerOpen((v) => !v)}
              >
                {csvDrawerOpen ? "Hide Paste CSV" : "Paste CSV"}
                {csvDrawerOpen ? (
                  <ChevronUp className="w-3.5 h-3.5 ml-1" />
                ) : (
                  <ChevronDown className="w-3.5 h-3.5 ml-1" />
                )}
              </Button>
            </div>
          </div>

          {/* Collapsible Paste CSV Drawer */}
          {csvDrawerOpen && (
            <Card padding="md" className="space-y-3 animate-fade-in">
              <div className="flex items-center justify-between">
                <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Import Timing CSV (driver,carId,bestLapMs,lastLapMs,laps)
                </span>
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Upload className="w-3.5 h-3.5" />}
                  disabled={!csvInput.trim()}
                  onClick={() => {
                    void loadTiming(csvInput);
                    setCsvDrawerOpen(false);
                  }}
                >
                  Load CSV
                </Button>
              </div>
              <textarea
                rows={4}
                value={csvInput}
                onChange={(e) => setCsvInput(e.target.value)}
                placeholder={"driver,carId,bestLapMs,lastLapMs,laps\nL. Suvorkov,ks_porsche_911_gt3_r_2016,137842,138110,14"}
                className="w-full rounded-xl bg-black/45 border border-white/[0.09] p-3 text-xs font-mono text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-border)]"
              />
            </Card>
          )}

          {/* Full-width F1-Style Broadcast Timing Tower */}
          <Card padding="none" className="overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-left border-collapse text-xs">
                <thead>
                  <tr className="border-b border-white/[0.08] text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)] bg-black/40">
                    <th className="py-3 px-4 w-14">POS</th>
                    <th className="py-3 px-3">DRIVER</th>
                    <th className="py-3 px-3">CAR</th>
                    <th className="py-3 px-3">LAPS</th>
                    <th className="py-3 px-3">GAP</th>
                    <th className="py-3 px-3">S1</th>
                    <th className="py-3 px-3">S2</th>
                    <th className="py-3 px-3">S3</th>
                    <th className="py-3 px-3">LAST LAP</th>
                    <th className="py-3 px-4">BEST LAP</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/[0.05] font-mono">
                  {timingRows.map((row) => {
                    const isLeader = row.position === 1;
                    return (
                      <tr
                        key={`${row.position}-${row.driver}`}
                        className="hover:bg-white/[0.03] transition-colors"
                      >
                        <td className="py-3 px-4">
                          <span
                            className={`inline-flex items-center justify-center w-6 h-6 rounded-md font-bold text-[11px] ${
                              isLeader
                                ? "bg-[var(--accent)] text-white"
                                : row.position <= 3
                                ? "bg-white/[0.1] text-white"
                                : "bg-white/[0.04] text-[var(--text-secondary)]"
                            }`}
                          >
                            {row.position}
                          </span>
                        </td>
                        <td className="py-3 px-3 font-sans font-semibold text-[var(--text-primary)]">
                          {row.driver}
                        </td>
                        <td className="py-3 px-3 text-[var(--text-secondary)]">
                          {row.carId.replace(/^ks_/, "").replace(/_/g, " ")}
                        </td>
                        <td className="py-3 px-3 text-[var(--text-secondary)]">{row.laps}</td>
                        <td className="py-3 px-3">
                          {isLeader ? (
                            <span className="text-[var(--accent)] font-semibold">LEADER</span>
                          ) : (
                            <span className="text-amber-300">
                              +{(row.gapMs / 1000).toFixed(3)}s
                            </span>
                          )}
                        </td>
                        <td className="py-3 px-3 text-[var(--text-secondary)]">
                          {formatSectorMs(row.sector1Ms)}
                        </td>
                        <td className="py-3 px-3 text-[var(--text-secondary)]">
                          {formatSectorMs(row.sector2Ms)}
                        </td>
                        <td className="py-3 px-3 text-[var(--text-secondary)]">
                          {formatSectorMs(row.sector3Ms)}
                        </td>
                        <td className="py-3 px-3 text-[var(--text-primary)]">
                          {formatMs(row.lastLapMs)}
                        </td>
                        <td className="py-3 px-4">
                          <span
                            className={
                              isLeader
                                ? "text-purple-300 font-semibold"
                                : "text-emerald-300"
                            }
                          >
                            {formatMs(row.bestLapMs)}
                          </span>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </Card>
        </div>
      )}

      {/* TAB 3: PERSONAL BEST LAP TIMES */}
      {tab === "laptimes" && (
        <Card padding="none" className="overflow-hidden">
          <div className="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Timer className="w-4 h-4 text-[var(--accent)]" />
              <span className="text-sm font-semibold text-[var(--text-primary)]">
                Personal Bests & Theoretical Optimal Sectors
              </span>
            </div>
            <Badge variant="success">PB + Theoretical</Badge>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-left border-collapse text-xs">
              <thead>
                <tr className="border-b border-white/[0.06] text-[10px] font-mono uppercase tracking-[0.1em] text-[var(--text-muted)] bg-white/[0.02]">
                  <th className="py-2.5 px-4">Track</th>
                  <th className="py-2.5 px-3">Car</th>
                  <th className="py-2.5 px-3">Best S1</th>
                  <th className="py-2.5 px-3">Best S2</th>
                  <th className="py-2.5 px-3">Best S3</th>
                  <th className="py-2.5 px-3">Personal Best</th>
                  <th className="py-2.5 px-3">Theoretical</th>
                  <th className="py-2.5 px-4">Potential</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/[0.05] font-mono">
                {DEMO_PERSONAL_BESTS.map((pb) => {
                  const theo = pb.s1 + pb.s2 + pb.s3;
                  const diff = pb.bestLap - theo;
                  return (
                    <tr key={`${pb.track}-${pb.car}`} className="hover:bg-white/[0.025]">
                      <td className="py-3 px-4 font-sans font-semibold text-[var(--text-primary)]">
                        {pb.track}
                      </td>
                      <td className="py-3 px-3 font-sans text-[var(--text-secondary)]">
                        {pb.car}
                      </td>
                      <td className="py-3 px-3 text-[var(--text-secondary)]">
                        {formatSectorMs(pb.s1)}
                      </td>
                      <td className="py-3 px-3 text-[var(--text-secondary)]">
                        {formatSectorMs(pb.s2)}
                      </td>
                      <td className="py-3 px-3 text-[var(--text-secondary)]">
                        {formatSectorMs(pb.s3)}
                      </td>
                      <td className="py-3 px-3 text-emerald-300 font-semibold">
                        {formatMs(pb.bestLap)}
                      </td>
                      <td className="py-3 px-3 text-purple-300">{formatMs(theo)}</td>
                      <td className="py-3 px-4">
                        <Badge variant="info">−{(diff / 1000).toFixed(3)}s</Badge>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </Card>
      )}
    </div>
  );
}

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  Cpu,
  Fuel,
  Globe,
  Plug,
  Radio,
  Sparkles,
  Unplug,
  Wifi,
} from "lucide-react";
import { api } from "../lib/api";
import type { LiveTelemetryFrame } from "../lib/types";
import { useAppStore } from "../store/appStore";
import { Badge, Button, Card, Toggle } from "../components/Primitives";

interface TraceSample {
  speed: number;
  throttle: number;
  brake: number;
  steer: number;
}

function formatLap(ms: number): string {
  if (!ms || ms <= 0) return "--:--.---";
  const m = Math.floor(ms / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  const rem = Math.floor(ms % 1000);
  return `${m}:${String(s).padStart(2, "0")}.${String(rem).padStart(3, "0")}`;
}

function formatGear(g: number): string {
  if (g < 0) return "R";
  if (g === 0) return "N";
  return String(g);
}

function tyreColor(tempC: number): string {
  if (tempC < 65) return "#38BDF8";
  if (tempC <= 96) return "#10B981";
  if (tempC <= 108) return "#FFB020";
  return "#EF4444";
}

const EMPTY_FRAME: LiveTelemetryFrame = {
  connected: false,
  speedKmh: 0,
  rpm: 0,
  maxRpm: 8500,
  gear: 0,
  throttle: 0,
  brake: 0,
  clutch: 0,
  steerDeg: 0,
  fuelL: 42.0,
  lapNumber: 0,
  currentLapMs: 0,
  lastLapMs: 0,
  bestLapMs: 0,
  tyreTempsC: [78, 80, 76, 77],
  tyrePressuresPsi: [26.8, 26.9, 26.5, 26.6],
  gLat: 0,
  gLong: 0,
};

export default function LiveTelemetryPage() {
  const { settings, notify } = useAppStore();
  const [udpHost, setUdpHost] = useState("127.0.0.1");
  const [udpPort, setUdpPort] = useState(settings.telemetryPort || 9996);
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);

  const [webPort, setWebPort] = useState(settings.webDashboardPort || 8787);
  const [webLan, setWebLan] = useState(true);
  const [webRunning, setWebRunning] = useState(false);

  const [simMode, setSimMode] = useState(true);
  const [frame, setFrame] = useState<LiveTelemetryFrame>(EMPTY_FRAME);
  const [trace, setTrace] = useState<TraceSample[]>([]);
  const [gHistory, setGHistory] = useState<Array<[number, number]>>([]);
  const simTickRef = useRef(0);

  // Poll live telemetry or run smooth 20Hz simulation preview when AC is offline
  useEffect(() => {
    const id = window.setInterval(async () => {
      if (connected && !simMode) {
        try {
          const live = await api.getLiveTelemetry();
          if (live) {
            setFrame(live);
            setTrace((prev) => [
              ...prev.slice(-119),
              {
                speed: live.speedKmh,
                throttle: live.throttle,
                brake: live.brake,
                steer: live.steerDeg,
              },
            ]);
            setGHistory((prev) => [...prev.slice(-34), [live.gLat, live.gLong]]);
          }
        } catch {
          // keep last frame
        }
        return;
      }

      if (simMode) {
        simTickRef.current += 0.085;
        const t = simTickRef.current;
        const cornerPhase = Math.sin(t * 0.7);
        const speedKmh = Math.max(
          74,
          Math.min(286, 182 + Math.sin(t * 0.55) * 78 + Math.cos(t * 1.1) * 24)
        );
        const braking = cornerPhase < -0.58;
        const brake = braking ? Math.min(1, (-cornerPhase - 0.58) * 2.4) : 0;
        const throttle = braking ? 0 : Math.min(1, 0.45 + (cornerPhase + 0.6) * 0.55);
        const gear = Math.max(2, Math.min(6, Math.round(speedKmh / 46)));
        const rpm = Math.min(
          8450,
          Math.round(3600 + ((speedKmh % 48) / 48) * 4400 + throttle * 650)
        );
        const steerDeg = Math.sin(t * 0.9) * 34;
        const gLat = Number((Math.sin(t * 0.9) * 1.85).toFixed(2));
        const gLong = Number((braking ? -brake * 2.1 : throttle * 0.85).toFixed(2));

        const nextFrame: LiveTelemetryFrame = {
          connected: true,
          speedKmh: Math.round(speedKmh),
          rpm,
          maxRpm: 8500,
          gear,
          throttle,
          brake,
          clutch: 0,
          steerDeg,
          fuelL: Math.max(12, Number((44.6 - (t * 0.015) % 18).toFixed(1))),
          lapNumber: 6,
          currentLapMs: Math.round((t * 1000) % 138000),
          lastLapMs: 138412,
          bestLapMs: 137842,
          tyreTempsC: [
            Math.round(86 + Math.abs(gLat) * 6),
            Math.round(84 + Math.abs(gLat) * 4),
            Math.round(82 + throttle * 5),
            Math.round(81 + throttle * 4),
          ],
          tyrePressuresPsi: [27.1, 27.0, 26.8, 26.7],
          gLat,
          gLong,
        };
        setFrame(nextFrame);
        setTrace((prev) => [
          ...prev.slice(-119),
          {
            speed: nextFrame.speedKmh,
            throttle: nextFrame.throttle,
            brake: nextFrame.brake,
            steer: nextFrame.steerDeg,
          },
        ]);
        setGHistory((prev) => [...prev.slice(-34), [gLat, gLong]]);
      }
    }, 65);
    return () => window.clearInterval(id);
  }, [connected, simMode]);

  const handleToggleUdp = async () => {
    setConnecting(true);
    try {
      if (connected) {
        await api.udpDisconnect();
        setConnected(false);
        notify("UDP stream disconnected", "info");
      } else {
        await api.udpConnect(udpHost, udpPort);
        setConnected(true);
        setSimMode(false);
        notify("Connected to AC UDP Telemetry", "success", `${udpHost}:${udpPort}`);
      }
    } catch (err) {
      notify("UDP connection error", "error", String(err));
    } finally {
      setConnecting(false);
    }
  };

  const handleToggleWeb = async () => {
    const next = !webRunning;
    try {
      await api.toggleWebDashboard(webPort, webLan, next);
      setWebRunning(next);
      notify(
        next ? "Web Pit-Wall Dashboard started" : "Web Dashboard stopped",
        next ? "success" : "info",
        next ? `http://${webLan ? "0.0.0.0" : "127.0.0.1"}:${webPort}` : undefined
      );
    } catch (err) {
      notify("Web Dashboard error", "error", String(err));
    }
  };

  // Multi-channel SVG trace polylines
  const tracePaths = useMemo(() => {
    const w = 640;
    const h = 156;
    const n = Math.max(trace.length - 1, 1);
    const makePath = (fn: (s: TraceSample) => number) =>
      trace
        .map((s, i) => {
          const x = (i / n) * w;
          const y = h - Math.max(0, Math.min(1, fn(s))) * (h - 14) - 7;
          return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
        })
        .join(" ");

    return {
      speed: makePath((s) => s.speed / 320),
      throttle: makePath((s) => s.throttle),
      brake: makePath((s) => s.brake),
      steer: makePath((s) => (s.steer + 60) / 120),
    };
  }, [trace]);

  const rpmPct = frame.maxRpm > 0 ? Math.min(1, frame.rpm / frame.maxRpm) : 0;
  const ledCount = 18;
  const activeLeds = Math.round(rpmPct * ledCount);

  // Live car position on procedural track loop
  const trackProgress = (simTickRef.current * 0.14) % (Math.PI * 2);
  const carDotX = 110 + Math.cos(trackProgress) * 76 + Math.sin(trackProgress * 2) * 16;
  const carDotY = 76 + Math.sin(trackProgress) * 46 + Math.cos(trackProgress * 3) * 10;

  return (
    <div className="p-5 max-w-[1680px] mx-auto space-y-4 animate-fade-in">
      {/* Compact Glass Pit-Wall Connection Bar */}
      <div className="glass-panel rounded-2xl px-4 py-3 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2.5">
          <Badge variant={connected ? "success" : simMode ? "info" : "neutral"} dot>
            {connected ? "UDP LIVE" : simMode ? "SIMULATED PIT-WALL" : "STANDBY"}
          </Badge>

          <div className="flex items-center gap-1.5 bg-black/35 border border-white/[0.08] rounded-lg px-2 py-1">
            <span className="text-[10px] font-mono uppercase text-[var(--text-muted)]">
              UDP
            </span>
            <input
              value={udpHost}
              onChange={(e) => setUdpHost(e.target.value)}
              className="w-28 bg-transparent text-xs font-mono text-[var(--text-primary)] focus:outline-none"
              placeholder="127.0.0.1"
            />
            <span className="text-[var(--text-muted)]">:</span>
            <input
              type="number"
              value={udpPort}
              onChange={(e) => setUdpPort(Number(e.target.value))}
              className="w-16 bg-transparent text-xs font-mono text-[var(--text-primary)] focus:outline-none"
            />
          </div>

          <Button
            variant={connected ? "danger" : "primary"}
            size="sm"
            loading={connecting}
            icon={connected ? <Unplug className="w-3.5 h-3.5" /> : <Plug className="w-3.5 h-3.5" />}
            onClick={() => void handleToggleUdp()}
          >
            {connected ? "Disconnect" : "Connect"}
          </Button>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <Toggle
            checked={simMode}
            onChange={setSimMode}
            label="Live Sim Feed"
            className="gap-2"
          />
          <div className="w-px h-5 bg-white/[0.08]" />
          <div className="flex items-center gap-2 bg-black/35 border border-white/[0.08] rounded-lg px-2.5 py-1">
            <Globe className="w-3.5 h-3.5 text-cyan-400" />
            <span className="text-[11px] font-mono text-[var(--text-secondary)]">
              Web Dash
            </span>
            <input
              type="number"
              value={webPort}
              onChange={(e) => setWebPort(Number(e.target.value))}
              className="w-14 bg-transparent text-xs font-mono text-[var(--text-primary)] focus:outline-none"
            />
            <button
              type="button"
              onClick={() => setWebLan((v) => !v)}
              className={`px-1.5 py-0.5 rounded text-[10px] font-mono cursor-pointer ${
                webLan
                  ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/30"
                  : "bg-white/[0.05] text-[var(--text-muted)]"
              }`}
            >
              LAN
            </button>
          </div>
          <Button
            variant={webRunning ? "danger" : "secondary"}
            size="sm"
            icon={<Wifi className="w-3.5 h-3.5" />}
            onClick={() => void handleToggleWeb()}
          >
            {webRunning ? "Stop Web" : "Start Web"}
          </Button>
        </div>
      </div>

      {/* Top Motorsport Cluster: Speed/RPM/Gear + Pedals + G-G Friction Circle + Track Map */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-4">
        {/* Digital Speed / RPM / Gear / Pedals */}
        <Card className="lg:col-span-5 flex flex-col justify-between gap-4" padding="md">
          {/* LED Shift Light Strip */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-[11px] font-mono text-[var(--text-muted)]">
              <span>ENGINE RPM</span>
              <span className="text-[var(--text-primary)]">
                {frame.rpm.toLocaleString()} / {frame.maxRpm.toLocaleString()}
              </span>
            </div>
            <div className="grid grid-cols-18 gap-1">
              {Array.from({ length: ledCount }, (_, i) => {
                const lit = i < activeLeds;
                const color =
                  i < 10 ? "#10B981" : i < 14 ? "#FFB020" : "#EF4444";
                return (
                  <div
                    key={i}
                    className="h-2.5 rounded-sm transition-opacity duration-75"
                    style={{
                      background: lit ? color : "rgba(255,255,255,0.07)",
                      boxShadow: lit ? `0 0 8px ${color}` : undefined,
                    }}
                  />
                );
              })}
            </div>
          </div>

          {/* Center Speed & Gear Readout */}
          <div className="grid grid-cols-12 items-center gap-3 py-1">
            <div className="col-span-5 flex flex-col">
              <span className="text-[10px] font-mono uppercase tracking-[0.14em] text-[var(--text-muted)]">
                Speed
              </span>
              <div className="flex items-baseline gap-1.5">
                <span className="font-mono text-6xl font-bold tracking-tight text-cyan-300 tabular-nums leading-none">
                  {String(frame.speedKmh).padStart(3, "0")}
                </span>
                <span className="font-mono text-xs text-[var(--text-muted)]">KM/H</span>
              </div>
            </div>

            <div className="col-span-3 flex flex-col items-center justify-center">
              <div className="w-16 h-16 rounded-2xl bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center">
                <span className="font-display text-5xl font-bold text-[var(--accent)] leading-none">
                  {formatGear(frame.gear)}
                </span>
              </div>
              <span className="text-[10px] font-mono text-[var(--text-muted)] mt-1">
                GEAR
              </span>
            </div>

            {/* Pedal Vertical Bars */}
            <div className="col-span-4 flex items-end justify-end gap-2.5 h-20">
              {[
                { label: "THR", val: frame.throttle, color: "#10B981" },
                { label: "BRK", val: frame.brake, color: "#EF4444" },
                { label: "CLT", val: frame.clutch, color: "#38BDF8" },
              ].map((p) => (
                <div key={p.label} className="flex flex-col items-center gap-1 h-full">
                  <div className="w-4 flex-1 rounded-md bg-white/[0.07] overflow-hidden flex items-end">
                    <div
                      className="w-full rounded-md transition-all duration-75"
                      style={{
                        height: `${Math.round(p.val * 100)}%`,
                        background: p.color,
                        boxShadow: p.val > 0.05 ? `0 0 8px ${p.color}` : undefined,
                      }}
                    />
                  </div>
                  <span className="text-[9px] font-mono text-[var(--text-muted)]">
                    {p.label}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* 4-Corner Tyre Temps & Pressures */}
          <div className="grid grid-cols-4 gap-2 pt-3 border-t border-white/[0.06]">
            {["FL", "FR", "RL", "RR"].map((corner, idx) => {
              const temp = frame.tyreTempsC[idx] ?? 80;
              const psi = frame.tyrePressuresPsi[idx] ?? 26.8;
              const col = tyreColor(temp);
              return (
                <div
                  key={corner}
                  className="rounded-lg bg-black/35 border border-white/[0.07] p-2 flex flex-col items-center"
                >
                  <span className="text-[10px] font-mono text-[var(--text-muted)]">
                    {corner}
                  </span>
                  <span className="font-mono text-sm font-semibold" style={{ color: col }}>
                    {temp}°C
                  </span>
                  <span className="text-[10px] font-mono text-[var(--text-secondary)]">
                    {psi.toFixed(1)} psi
                  </span>
                </div>
              );
            })}
          </div>
        </Card>

        {/* SVG Friction Circle (G-G Diagram) */}
        <Card className="lg:col-span-3 flex flex-col justify-between" padding="md">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Friction Circle (G-G)
            </span>
            <span className="text-xs font-mono text-[var(--text-secondary)]">
              {Math.hypot(frame.gLat, frame.gLong).toFixed(2)} G
            </span>
          </div>
          <div className="flex items-center justify-center py-1">
            <svg viewBox="0 0 180 180" className="w-[178px] h-[178px]">
              {[28, 54, 78].map((r, i) => (
                <circle
                  key={r}
                  cx="90"
                  cy="90"
                  r={r}
                  fill="none"
                  stroke="rgba(255,255,255,0.08)"
                  strokeDasharray={i === 1 ? "3 3" : undefined}
                />
              ))}
              <line x1="12" y1="90" x2="168" y2="90" stroke="rgba(255,255,255,0.08)" />
              <line x1="90" y1="12" x2="90" y2="168" stroke="rgba(255,255,255,0.08)" />
              <text x="93" y="20" fill="#5F6B7E" fontSize="8" fontFamily="JetBrains Mono">
                ACCEL
              </text>
              <text x="93" y="168" fill="#5F6B7E" fontSize="8" fontFamily="JetBrains Mono">
                BRAKE
              </text>
              {gHistory.map(([gl, gx], idx) => (
                <circle
                  key={idx}
                  cx={90 + gl * 30}
                  cy={90 - gx * 30}
                  r={2}
                  fill="var(--accent)"
                  fillOpacity={(idx + 1) / gHistory.length * 0.45}
                />
              ))}
              <circle
                cx={90 + frame.gLat * 30}
                cy={90 - frame.gLong * 30}
                r={5.5}
                fill="var(--accent)"
                stroke="#fff"
                strokeWidth="1.5"
              />
            </svg>
          </div>
          <div className="grid grid-cols-2 gap-2 text-center font-mono text-[11px]">
            <div className="rounded-lg bg-white/[0.03] py-1">
              LAT <span className="text-[var(--text-primary)]">{frame.gLat.toFixed(2)}g</span>
            </div>
            <div className="rounded-lg bg-white/[0.03] py-1">
              LONG <span className="text-[var(--text-primary)]">{frame.gLong.toFixed(2)}g</span>
            </div>
          </div>
        </Card>

        {/* Live Track Map + Lap / Fuel / Shared Memory Status */}
        <Card className="lg:col-span-4 flex flex-col justify-between gap-3" padding="md">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Live Circuit Radar
            </span>
            <Badge variant="accent">LAP {frame.lapNumber || 1}</Badge>
          </div>

          <div className="rounded-xl bg-[#070A10] border border-white/[0.06] p-1">
            <svg viewBox="0 0 220 152" className="w-full h-[132px]">
              <path
                d={
                  Array.from({ length: 64 }, (_, i) => {
                    const a = (i / 64) * Math.PI * 2;
                    const x = 110 + Math.cos(a) * 76 + Math.sin(a * 2) * 16;
                    const y = 76 + Math.sin(a) * 46 + Math.cos(a * 3) * 10;
                    return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
                  }).join(" ") + " Z"
                }
                fill="none"
                stroke="rgba(255,255,255,0.14)"
                strokeWidth="6"
              />
              <circle cx={carDotX} cy={carDotY} r={8} fill="var(--accent)" fillOpacity={0.28} />
              <circle cx={carDotX} cy={carDotY} r={4} fill="var(--accent)" stroke="#fff" strokeWidth="1.2" />
            </svg>
          </div>

          <div className="grid grid-cols-3 gap-2 font-mono">
            <div className="rounded-lg bg-white/[0.03] border border-white/[0.06] p-2">
              <div className="text-[10px] text-[var(--text-muted)]">CURRENT</div>
              <div className="text-xs font-semibold text-[var(--text-primary)] mt-0.5">
                {formatLap(frame.currentLapMs)}
              </div>
            </div>
            <div className="rounded-lg bg-white/[0.03] border border-white/[0.06] p-2">
              <div className="text-[10px] text-[var(--text-muted)]">BEST LAP</div>
              <div className="text-xs font-semibold text-emerald-300 mt-0.5">
                {formatLap(frame.bestLapMs)}
              </div>
            </div>
            <div className="rounded-lg bg-white/[0.03] border border-white/[0.06] p-2">
              <div className="text-[10px] text-[var(--text-muted)] flex items-center gap-1">
                <Fuel className="w-3 h-3 text-amber-400" /> FUEL
              </div>
              <div className="text-xs font-semibold text-amber-300 mt-0.5">
                {frame.fuelL.toFixed(1)} L
              </div>
            </div>
          </div>
        </Card>
      </div>

      {/* Live Multi-Channel SVG Trace Chart */}
      <Card padding="md" className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Activity className="w-4 h-4 text-[var(--accent)]" />
            <span className="text-sm font-semibold text-[var(--text-primary)]">
              Live Multi-Channel Telemetry Trace
            </span>
          </div>
          <div className="flex items-center gap-4 text-[11px] font-mono">
            <span className="flex items-center gap-1.5 text-cyan-300">
              <span className="w-2.5 h-0.5 bg-[#00D1FF] inline-block" /> Speed
            </span>
            <span className="flex items-center gap-1.5 text-emerald-300">
              <span className="w-2.5 h-0.5 bg-[#10B981] inline-block" /> Throttle
            </span>
            <span className="flex items-center gap-1.5 text-red-300">
              <span className="w-2.5 h-0.5 bg-[#EF4444] inline-block" /> Brake
            </span>
            <span className="flex items-center gap-1.5 text-amber-300">
              <span className="w-2.5 h-0.5 bg-[#FFB020] inline-block" /> Steer
            </span>
          </div>
        </div>

        <div className="rounded-xl bg-[#070A10] border border-white/[0.06] p-2 overflow-hidden">
          <svg viewBox="0 0 640 156" preserveAspectRatio="none" className="w-full h-[165px]">
            {[32, 78, 124].map((y) => (
              <line
                key={y}
                x1="0"
                y1={y}
                x2="640"
                y2={y}
                stroke="rgba(255,255,255,0.06)"
                strokeDasharray="4 4"
              />
            ))}
            <path d={tracePaths.steer} fill="none" stroke="#FFB020" strokeWidth="1.5" strokeOpacity="0.8" />
            <path d={tracePaths.brake} fill="none" stroke="#EF4444" strokeWidth="2" />
            <path d={tracePaths.throttle} fill="none" stroke="#10B981" strokeWidth="2" />
            <path d={tracePaths.speed} fill="none" stroke="#00D1FF" strokeWidth="2.2" />
          </svg>
        </div>
      </Card>

      {/* Shared Memory & ACXS Coach Status Strip */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card padding="md" className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-cyan-500/15 border border-cyan-500/30 flex items-center justify-center text-cyan-300">
              <Cpu className="w-4 h-4" />
            </div>
            <div>
              <div className="text-[13px] font-semibold text-[var(--text-primary)]">
                AC Shared Memory Bridge
              </div>
              <div className="text-[11px] text-[var(--text-secondary)] font-mono">
                acpmf_physics · acpmf_graphics · acpmf_static (60 Hz)
              </div>
            </div>
          </div>
          <Badge variant={connected ? "success" : "neutral"} dot>
            {connected ? "ATTACHED" : "WAITING FOR AC"}
          </Badge>
        </Card>

        <Card padding="md" className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center text-[var(--accent)]">
              <Sparkles className="w-4 h-4" />
            </div>
            <div>
              <div className="text-[13px] font-semibold text-[var(--text-primary)]">
                ACXS Live Race Engineer
              </div>
              <div className="text-[11px] text-[var(--text-secondary)]">
                {frame.brake > 0.85
                  ? "Heavy threshold braking detected — trail off smoothly toward apex."
                  : "Tyre core temps optimal (82–88°C). Push mode cleared for Sector 2."}
              </div>
            </div>
          </div>
          <Badge variant="accent" dot>
            <Radio className="w-3 h-3" /> ACTIVE
          </Badge>
        </Card>
      </div>
    </div>
  );
}

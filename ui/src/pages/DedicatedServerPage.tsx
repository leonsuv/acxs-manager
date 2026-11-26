import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Ban,
  Copy,
  ExternalLink,
  FolderOpen,
  MessageSquare,
  Play,
  RefreshCw,
  Save,
  Scale,
  Send,
  Server,
  Shield,
  SkipForward,
  Sliders,
  Square,
  Terminal,
  UserX,
} from "lucide-react";
import { api } from "../lib/api";
import type {
  DedicatedServerConfig,
  DedicatedServerStatus,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  PageHeader,
  SegmentedControl,
  Slider,
  Toggle,
} from "../components/Primitives";

type DedicatedView = "config" | "csp" | "rcon";

interface RconDriverRow {
  id: number;
  name: string;
  car: string;
  pingMs: number;
  ballastKg: number;
}

const DEFAULT_CFG: DedicatedServerConfig = {
  name: "ACXS Motorsport Dedicated",
  track: "spa",
  layout: "default",
  cars: ["ks_porsche_911_gt3_r_2016", "ks_ferrari_488_gt3"],
  maxClients: 24,
  udpPort: 9600,
  tcpPort: 9600,
  httpPort: 8081,
  password: "",
  adminPassword: "acxs-admin",
  registerToLobby: true,
  loopMode: true,
  pickupMode: true,
  practiceMinutes: 15,
  qualifyMinutes: 10,
  raceLaps: 12,
};

function formatUptime(sec?: number): string {
  if (!sec || sec <= 0) return "00:00:00";
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  return [h, m, s].map((v) => String(v).padStart(2, "0")).join(":");
}

export function DedicatedServerPage() {
  const { cars, tracks, settings, notify } = useAppStore();

  const [view, setView] = useState<DedicatedView>("config");
  const [serverDir, setServerDir] = useState(
    settings.acRoot ? `${settings.acRoot}/server` : "/ac/server"
  );
  const [cfg, setCfg] = useState<DedicatedServerConfig>(DEFAULT_CFG);
  const [status, setStatus] = useState<DedicatedServerStatus>({
    running: false,
    pid: null,
    uptimeSec: 0,
    recentLogs: [],
  });
  const [loadingCfg, setLoadingCfg] = useState(false);
  const [savingCfg, setSavingCfg] = useState(false);
  const [togglingServer, setTogglingServer] = useState(false);

  // CSP extra_cfg structured options
  const [cspAiFlood, setCspAiFlood] = useState(false);
  const [cspWeatherSync, setCspWeatherSync] = useState(true);
  const [cspExtendedPhysics, setCspExtendedPhysics] = useState(true);
  const [cspCollisionFix, setCspCollisionFix] = useState(true);
  const [cspRealMirrors, setCspRealMirrors] = useState(true);
  const [cspWrongWayReset, setCspWrongWayReset] = useState(true);
  const [cspWelcomeMsg, setCspWelcomeMsg] = useState(
    "Welcome to ACXS Dedicated! Keep racing clean."
  );

  // RCON state
  const [rconHost, setRconHost] = useState("127.0.0.1");
  const [rconPort, setRconPort] = useState(9600);
  const [rconConnected, setRconConnected] = useState(false);
  const [rconCommand, setRconCommand] = useState("");
  const [broadcastMsg, setBroadcastMsg] = useState("");
  const [rconLog, setRconLog] = useState<string[]>([
    "[RCON] Ready. Connect to your running acServer instance.",
  ]);
  const [rconDrivers, setRconDrivers] = useState<RconDriverRow[]>([
    { id: 0, name: "M. Rossi", car: "ks_ferrari_488_gt3", pingMs: 26, ballastKg: 0 },
    { id: 1, name: "L. Weber", car: "ks_porsche_911_gt3_r_2016", pingMs: 31, ballastKg: 15 },
    { id: 2, name: "K. Tanaka", car: "ks_mazda_787b", pingMs: 48, ballastKg: 0 },
  ]);

  const refreshStatus = async () => {
    try {
      const s = await api.getDedicatedServerStatus();
      if (s) setStatus(s);
    } catch {
      // ignore poll errors
    }
  };

  useEffect(() => {
    void refreshStatus();
    const id = window.setInterval(() => {
      void refreshStatus();
    }, 3500);
    return () => window.clearInterval(id);
  }, []);

  const handleLoadConfig = async () => {
    setLoadingCfg(true);
    try {
      const loaded = await api.loadDedicatedServer(serverDir);
      if (loaded) {
        setCfg(loaded);
        notify("Server config loaded", "success", serverDir);
      } else {
        notify("Using default server configuration", "info");
      }
    } catch (err) {
      notify("Failed to load server config", "error", String(err));
    } finally {
      setLoadingCfg(false);
    }
  };

  const handleSaveConfig = async () => {
    setSavingCfg(true);
    try {
      await api.saveDedicatedServer(serverDir, cfg, []);
      notify("Dedicated server config saved", "success", cfg.name);
    } catch (err) {
      notify("Failed to save server config", "error", String(err));
    } finally {
      setSavingCfg(false);
    }
  };

  const handleToggleServer = async () => {
    setTogglingServer(true);
    try {
      if (status.running) {
        await api.stopDedicatedServer();
        setStatus((prev) => ({
          ...prev,
          running: false,
          pid: null,
          recentLogs: [
            ...prev.recentLogs,
            `[${new Date().toLocaleTimeString()}] Server stopped by operator.`,
          ],
        }));
        notify("Dedicated server stopped", "info");
      } else {
        const next = await api.startDedicatedServer(serverDir);
        if (next) {
          setStatus(next);
        } else {
          setStatus({
            running: true,
            pid: 42108,
            uptimeSec: 1,
            recentLogs: [
              `[${new Date().toLocaleTimeString()}] Starting acServer in ${serverDir}...`,
              `[${new Date().toLocaleTimeString()}] Listening UDP ${cfg.udpPort} / TCP ${cfg.tcpPort} / HTTP ${cfg.httpPort}`,
              `[${new Date().toLocaleTimeString()}] Track: ${cfg.track} (${cfg.layout}) · Max Clients: ${cfg.maxClients}`,
            ],
          });
        }
        notify("Dedicated server started", "success", cfg.name);
      }
    } catch (err) {
      notify("Server process error", "error", String(err));
    } finally {
      setTogglingServer(false);
    }
  };

  const handleOpenSpectatorDashboard = async () => {
    const port = settings.webDashboardPort || 8787;
    try {
      await api.toggleWebDashboard(port, true, true);
    } catch {
      // may already be running
    }
    const url = `http://localhost:${port}`;
    window.open(url, "_blank", "noopener,noreferrer");
    notify("Spectator Dashboard opened", "info", url);
  };

  // Generated csp_extra_options.ini
  const cspExtraIni = useMemo(() => {
    return [
      "[EXTRA_RULES]",
      `ALLOW_WRONG_WAY = ${cspWrongWayReset ? 1 : 0}`,
      `ENABLE_EXTENDED_PHYSICS = ${cspExtendedPhysics ? 1 : 0}`,
      "",
      "[AI_FLOOD]",
      `ENABLED = ${cspAiFlood ? 1 : 0}`,
      "MIN_TRACK_LENGTH = 2000",
      "",
      "[WEATHER_FX]",
      `SYNC_WEATHER = ${cspWeatherSync ? 1 : 0}`,
      "TIME_MULT = 1",
      "",
      "[CUSTOM_PHYSICS]",
      `IMPROVED_COLLISIONS = ${cspCollisionFix ? 1 : 0}`,
      `REAL_MIRRORS = ${cspRealMirrors ? 1 : 0}`,
      "",
      "[CHAT]",
      `WELCOME_MESSAGE = ${cspWelcomeMsg}`,
    ].join("\n");
  }, [
    cspAiFlood,
    cspWeatherSync,
    cspExtendedPhysics,
    cspCollisionFix,
    cspRealMirrors,
    cspWrongWayReset,
    cspWelcomeMsg,
  ]);

  // RCON handlers
  const handleRconConnect = async () => {
    try {
      const res = await api.rconConnect(rconHost, rconPort);
      setRconConnected(res?.connected ?? true);
      setRconLog((prev) => [
        ...prev,
        `[RCON] Connected to ${rconHost}:${rconPort}${
          res?.output ? ` — ${res.output}` : ""
        }`,
      ]);
      notify("RCON connected", "success", `${rconHost}:${rconPort}`);
    } catch (err) {
      notify("RCON connection failed", "error", String(err));
    }
  };

  const sendRcon = async (cmd: string) => {
    const trimmed = cmd.trim();
    if (!trimmed) return;
    setRconLog((prev) => [...prev, `> ${trimmed}`]);
    try {
      const res = await api.rconSendCommand(trimmed);
      setRconLog((prev) => [
        ...prev,
        res?.output ? res.output : `[OK] Executed: ${trimmed}`,
      ]);
    } catch (err) {
      setRconLog((prev) => [...prev, `[ERR] ${String(err)}`]);
    }
  };

  const handleToggleCar = (carId: string) => {
    setCfg((prev) => ({
      ...prev,
      cars: prev.cars.includes(carId)
        ? prev.cars.filter((c) => c !== carId)
        : [...prev.cars, carId],
    }));
  };

  return (
    <div className="h-full flex flex-col overflow-hidden px-6 py-5 gap-4 animate-fade-in">
      <PageHeader
        eyebrow="Online · Dedicated Hosting"
        title="Dedicated Server Manager"
        subtitle="Configure acServer, CSP extra options, live process console, and real-time RCON race control."
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={status.running ? "success" : "neutral"} dot>
              {status.running
                ? `ONLINE · PID ${status.pid ?? "—"}`
                : "SERVER OFFLINE"}
            </Badge>
            <Button
              variant="secondary"
              size="sm"
              icon={<ExternalLink className="w-3.5 h-3.5" />}
              onClick={handleOpenSpectatorDashboard}
            >
              Spectator Dashboard
            </Button>
            <Button
              variant={status.running ? "danger" : "primary"}
              size="sm"
              loading={togglingServer}
              icon={
                status.running ? (
                  <Square className="w-3.5 h-3.5 fill-current" />
                ) : (
                  <Play className="w-3.5 h-3.5 fill-current" />
                )
              }
              onClick={handleToggleServer}
            >
              {status.running ? "Stop Server" : "Start Server"}
            </Button>
          </div>
        }
      />

      {/* View Switcher */}
      <div className="shrink-0 flex items-center justify-between gap-3">
        <SegmentedControl
          size="sm"
          value={view}
          onChange={setView}
          options={[
            {
              value: "config",
              label: "Config & Process",
              icon: <Server className="w-3.5 h-3.5" />,
            },
            {
              value: "csp",
              label: "CSP extra_cfg",
              icon: <Sliders className="w-3.5 h-3.5" />,
            },
            {
              value: "rcon",
              label: "Live Admin / RCON",
              icon: <Shield className="w-3.5 h-3.5" />,
            },
          ]}
        />
        {status.running && (
          <div className="text-xs font-mono text-[var(--text-secondary)] tabular-nums flex items-center gap-2">
            <Activity className="w-3.5 h-3.5 text-emerald-400" />
            Uptime {formatUptime(status.uptimeSec)}
          </div>
        )}
      </div>

      {/* =================================================================== */}
      {/* TAB 1: CONFIG & PROCESS (2-Column)                                  */}
      {/* =================================================================== */}
      {view === "config" && (
        <div className="grid grid-cols-1 xl:grid-cols-12 gap-4 flex-1 min-h-0">
          {/* Left: Server Config Form */}
          <Card
            variant="glass"
            padding="md"
            className="xl:col-span-7 overflow-y-auto space-y-4"
          >
            {/* Server Directory Bar */}
            <div className="flex flex-wrap items-center gap-2 pb-3 border-b border-white/[0.06]">
              <FolderOpen className="w-4 h-4 text-[var(--accent)] shrink-0" />
              <input
                type="text"
                value={serverDir}
                onChange={(e) => setServerDir(e.target.value)}
                className="flex-1 min-w-[200px] h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white outline-none focus:border-[var(--accent-border)]"
              />
              <Button
                variant="secondary"
                size="sm"
                loading={loadingCfg}
                onClick={handleLoadConfig}
              >
                Load
              </Button>
              <Button
                variant="primary"
                size="sm"
                loading={savingCfg}
                icon={<Save className="w-3.5 h-3.5" />}
                onClick={handleSaveConfig}
              >
                Save Config
              </Button>
            </div>

            {/* Identity & Track */}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div className="space-y-1">
                <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                  Server Name
                </label>
                <input
                  type="text"
                  value={cfg.name}
                  onChange={(e) => setCfg({ ...cfg, name: e.target.value })}
                  className="w-full h-9 px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white outline-none focus:border-[var(--accent-border)]"
                />
              </div>

              <div className="space-y-1">
                <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                  Track
                </label>
                <select
                  value={cfg.track}
                  onChange={(e) => setCfg({ ...cfg, track: e.target.value })}
                  className="w-full h-9 px-2.5 rounded-lg bg-black/60 border border-white/[0.1] text-xs text-white outline-none"
                >
                  {tracks.map((t) => (
                    <option key={t.id} value={t.id} className="bg-[#0D1118]">
                      {t.name} ({t.id})
                    </option>
                  ))}
                </select>
              </div>
            </div>

            {/* Ports & Passwords */}
            <div className="grid grid-cols-2 sm:grid-cols-5 gap-2.5">
              {[
                { label: "UDP Port", key: "udpPort" as const },
                { label: "TCP Port", key: "tcpPort" as const },
                { label: "HTTP Port", key: "httpPort" as const },
              ].map((p) => (
                <div key={p.key} className="space-y-1">
                  <label className="text-[10px] font-mono uppercase text-[var(--text-muted)]">
                    {p.label}
                  </label>
                  <input
                    type="number"
                    value={cfg[p.key]}
                    onChange={(e) =>
                      setCfg({ ...cfg, [p.key]: Number(e.target.value) })
                    }
                    className="w-full h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white tabular-nums"
                  />
                </div>
              ))}
              <div className="space-y-1">
                <label className="text-[10px] font-mono uppercase text-[var(--text-muted)]">
                  Password
                </label>
                <input
                  type="password"
                  value={cfg.password}
                  onChange={(e) => setCfg({ ...cfg, password: e.target.value })}
                  placeholder="Public"
                  className="w-full h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white"
                />
              </div>
              <div className="space-y-1">
                <label className="text-[10px] font-mono uppercase text-[var(--text-muted)]">
                  Admin Pass
                </label>
                <input
                  type="password"
                  value={cfg.adminPassword}
                  onChange={(e) =>
                    setCfg({ ...cfg, adminPassword: e.target.value })
                  }
                  className="w-full h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white"
                />
              </div>
            </div>

            {/* Session Sliders */}
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-1">
              <Slider
                label="Max Clients"
                value={cfg.maxClients}
                min={2}
                max={64}
                onChange={(v) => setCfg({ ...cfg, maxClients: v })}
              />
              <Slider
                label="Practice"
                value={cfg.practiceMinutes}
                min={0}
                max={90}
                step={5}
                unit="m"
                onChange={(v) => setCfg({ ...cfg, practiceMinutes: v })}
              />
              <Slider
                label="Qualify"
                value={cfg.qualifyMinutes}
                min={0}
                max={60}
                step={5}
                unit="m"
                onChange={(v) => setCfg({ ...cfg, qualifyMinutes: v })}
              />
              <Slider
                label="Race Laps"
                value={cfg.raceLaps}
                min={0}
                max={60}
                onChange={(v) => setCfg({ ...cfg, raceLaps: v })}
              />
            </div>

            {/* Toggles */}
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 pt-2 border-t border-white/[0.06]">
              <Toggle
                checked={cfg.registerToLobby}
                onChange={(v) => setCfg({ ...cfg, registerToLobby: v })}
                label="Public Lobby"
              />
              <Toggle
                checked={cfg.pickupMode}
                onChange={(v) => setCfg({ ...cfg, pickupMode: v })}
                label="Pickup Mode"
              />
              <Toggle
                checked={cfg.loopMode}
                onChange={(v) => setCfg({ ...cfg, loopMode: v })}
                label="Loop Sessions"
              />
            </div>

            {/* Allowed Cars Multi-Select Chips */}
            <div className="space-y-2 pt-2 border-t border-white/[0.06]">
              <div className="flex items-center justify-between text-xs">
                <span className="font-medium text-[var(--text-secondary)]">
                  Allowed Cars ({cfg.cars.length})
                </span>
              </div>
              <div className="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto">
                {cars.map((c) => {
                  const active = cfg.cars.includes(c.id);
                  return (
                    <button
                      key={c.id}
                      type="button"
                      onClick={() => handleToggleCar(c.id)}
                      className={`px-2.5 py-1 rounded-lg text-[11px] font-medium border transition-all cursor-pointer ${
                        active
                          ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                          : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-white"
                      }`}
                    >
                      {c.name}
                    </button>
                  );
                })}
              </div>
            </div>
          </Card>

          {/* Right: Live Process Status & Console */}
          <Card
            variant="glass"
            padding="none"
            className="xl:col-span-5 flex flex-col overflow-hidden"
          >
            <div className="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Terminal className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-xs font-semibold uppercase tracking-wider text-white">
                  Live Process Console
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<RefreshCw className="w-3.5 h-3.5" />}
                  onClick={refreshStatus}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Copy className="w-3.5 h-3.5" />}
                  onClick={() => {
                    void navigator.clipboard.writeText(
                      status.recentLogs.join("\n")
                    );
                    notify("Console logs copied", "info");
                  }}
                />
              </div>
            </div>

            <div className="flex-1 bg-black/55 p-3.5 font-mono text-[11px] leading-relaxed text-emerald-300/90 overflow-y-auto space-y-1">
              {status.recentLogs.length === 0 ? (
                <div className="text-[var(--text-muted)]">
                  // acServer process is idle. Click &quot;Start Server&quot; to
                  stream live stdout/stderr here.
                </div>
              ) : (
                status.recentLogs.map((line, i) => (
                  <div key={i} className="break-all">
                    {line}
                  </div>
                ))
              )}
            </div>
          </Card>
        </div>
      )}

      {/* =================================================================== */}
      {/* TAB 2: CSP extra_cfg (2-Column)                                     */}
      {/* =================================================================== */}
      {view === "csp" && (
        <div className="grid grid-cols-1 xl:grid-cols-12 gap-4 flex-1 min-h-0">
          <Card
            variant="glass"
            padding="md"
            className="xl:col-span-6 overflow-y-auto space-y-3.5"
          >
            <div className="text-xs font-semibold uppercase tracking-wider text-white">
              CSP Server Extensions
            </div>
            <Toggle
              checked={cspWeatherSync}
              onChange={setCspWeatherSync}
              label="WeatherFX Time & Sky Sync"
              description="Synchronize Sol/Pure dynamic weather across all connected clients"
            />
            <Toggle
              checked={cspExtendedPhysics}
              onChange={setCspExtendedPhysics}
              label="Extended Car Physics"
              description="Enable CSP rain physics, tyre thermal extensions, and aero updates"
            />
            <Toggle
              checked={cspCollisionFix}
              onChange={setCspCollisionFix}
              label="Improved Netcode Collisions"
              description="Reduce high-ping contact bounce and ghosting artifacts"
            />
            <Toggle
              checked={cspRealMirrors}
              onChange={setCspRealMirrors}
              label="Enforce Real Mirrors"
              description="Synchronize interior/exterior mirror geometry for cockpit realism"
            />
            <Toggle
              checked={cspWrongWayReset}
              onChange={setCspWrongWayReset}
              label="Allow Wrong-Way Pit Reset"
              description="Permit safe teleport back to pits on freeroam/trackday maps"
            />
            <Toggle
              checked={cspAiFlood}
              onChange={setCspAiFlood}
              label="CSP Server AI Traffic Flood"
              description="Spawn synchronized traffic cars on SRP / freeroam circuits"
            />

            <div className="pt-2 border-t border-white/[0.06] space-y-1.5">
              <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                Join Welcome Message
              </label>
              <input
                type="text"
                value={cspWelcomeMsg}
                onChange={(e) => setCspWelcomeMsg(e.target.value)}
                className="w-full h-9 px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white outline-none focus:border-[var(--accent-border)]"
              />
            </div>
          </Card>

          <Card
            variant="glass"
            padding="none"
            className="xl:col-span-6 flex flex-col overflow-hidden"
          >
            <div className="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between">
              <span className="text-xs font-mono text-[var(--text-secondary)]">
                csp_extra_options.ini
              </span>
              <div className="flex items-center gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<Copy className="w-3.5 h-3.5" />}
                  onClick={() => {
                    void navigator.clipboard.writeText(cspExtraIni);
                    notify("csp_extra_options.ini copied", "success");
                  }}
                >
                  Copy INI
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Save className="w-3.5 h-3.5" />}
                  onClick={handleSaveConfig}
                >
                  Apply to Server
                </Button>
              </div>
            </div>
            <pre className="flex-1 p-4 bg-black/55 font-mono text-xs text-cyan-200/90 overflow-auto leading-relaxed">
              {cspExtraIni}
            </pre>
          </Card>
        </div>
      )}

      {/* =================================================================== */}
      {/* TAB 3: LIVE ADMIN / RCON (2-Column)                                 */}
      {/* =================================================================== */}
      {view === "rcon" && (
        <div className="grid grid-cols-1 xl:grid-cols-12 gap-4 flex-1 min-h-0">
          {/* Left: RCON Connection, Driver Table, Broadcast & Session Controls */}
          <div className="xl:col-span-7 flex flex-col gap-3.5 min-h-0 overflow-y-auto pr-1">
            {/* Connection Bar */}
            <Card variant="glass" padding="sm">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant={rconConnected ? "success" : "neutral"} dot>
                  {rconConnected ? "RCON LIVE" : "DISCONNECTED"}
                </Badge>
                <input
                  type="text"
                  value={rconHost}
                  onChange={(e) => setRconHost(e.target.value)}
                  className="h-8 w-36 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white"
                />
                <input
                  type="number"
                  value={rconPort}
                  onChange={(e) => setRconPort(Number(e.target.value))}
                  className="h-8 w-24 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white tabular-nums"
                />
                <Button
                  variant="primary"
                  size="sm"
                  onClick={handleRconConnect}
                >
                  Connect RCON
                </Button>
                <div className="flex-1" />
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<SkipForward className="w-3.5 h-3.5" />}
                  onClick={() => sendRcon("/next_session")}
                >
                  Next Session
                </Button>
              </div>
            </Card>

            {/* RCON Driver Table */}
            <Card variant="glass" padding="none" className="overflow-hidden">
              <div className="px-4 py-2.5 border-b border-white/[0.06] flex items-center justify-between">
                <span className="text-xs font-semibold uppercase tracking-wider text-white">
                  Connected Drivers ({rconDrivers.length})
                </span>
                <span className="text-[11px] font-mono text-[var(--text-muted)]">
                  Kick · Ban · Ballast
                </span>
              </div>
              <div className="overflow-x-auto">
                <table className="w-full text-left text-xs">
                  <thead>
                    <tr className="border-b border-white/[0.06] text-[10px] font-mono uppercase text-[var(--text-muted)]">
                      <th className="py-2 px-3">ID</th>
                      <th className="py-2 px-3">Driver</th>
                      <th className="py-2 px-3">Car</th>
                      <th className="py-2 px-3 text-right">Ping</th>
                      <th className="py-2 px-3 text-right">Ballast</th>
                      <th className="py-2 px-3 text-right">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-white/[0.05]">
                    {rconDrivers.map((d) => (
                      <tr key={d.id} className="hover:bg-white/[0.02]">
                        <td className="py-2 px-3 font-mono text-[var(--text-muted)]">
                          #{d.id}
                        </td>
                        <td className="py-2 px-3 font-semibold text-white">
                          {d.name}
                        </td>
                        <td className="py-2 px-3 text-[var(--text-secondary)] truncate max-w-[140px]">
                          {cars.find((c) => c.id === d.car)?.name ?? d.car}
                        </td>
                        <td className="py-2 px-3 text-right font-mono tabular-nums text-emerald-300">
                          {d.pingMs} ms
                        </td>
                        <td className="py-2 px-3 text-right font-mono tabular-nums text-amber-300">
                          +{d.ballastKg} kg
                        </td>
                        <td className="py-2 px-3">
                          <div className="flex items-center justify-end gap-1">
                            <button
                              type="button"
                              title="Add +10kg Ballast"
                              onClick={() => {
                                const nextKg = d.ballastKg + 10;
                                setRconDrivers((prev) =>
                                  prev.map((row) =>
                                    row.id === d.id
                                      ? { ...row, ballastKg: nextKg }
                                      : row
                                  )
                                );
                                void sendRcon(`/ballast ${d.id} ${nextKg}`);
                              }}
                              className="h-7 px-2 rounded-md bg-amber-500/15 border border-amber-500/30 text-amber-300 hover:bg-amber-500/25 text-[11px] font-mono inline-flex items-center gap-1 cursor-pointer"
                            >
                              <Scale className="w-3 h-3" />
                              +10kg
                            </button>
                            <button
                              type="button"
                              title="Kick Driver"
                              onClick={() => {
                                void sendRcon(`/kick_id ${d.id}`);
                                setRconDrivers((prev) =>
                                  prev.filter((row) => row.id !== d.id)
                                );
                              }}
                              className="h-7 px-2 rounded-md bg-white/[0.06] border border-white/[0.1] text-[var(--text-secondary)] hover:text-white text-[11px] inline-flex items-center gap-1 cursor-pointer"
                            >
                              <UserX className="w-3 h-3" />
                              Kick
                            </button>
                            <button
                              type="button"
                              title="Ban Driver"
                              onClick={() => {
                                void sendRcon(`/ban_id ${d.id}`);
                                setRconDrivers((prev) =>
                                  prev.filter((row) => row.id !== d.id)
                                );
                              }}
                              className="h-7 px-2 rounded-md bg-red-500/15 border border-red-500/30 text-red-300 hover:bg-red-500/25 text-[11px] inline-flex items-center gap-1 cursor-pointer"
                            >
                              <Ban className="w-3 h-3" />
                              Ban
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Card>

            {/* Server Broadcast Chat */}
            <Card variant="glass" padding="sm">
              <div className="flex items-center gap-2">
                <MessageSquare className="w-4 h-4 text-[var(--accent)] shrink-0" />
                <input
                  type="text"
                  value={broadcastMsg}
                  onChange={(e) => setBroadcastMsg(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && broadcastMsg.trim()) {
                      void sendRcon(`/broadcast ${broadcastMsg.trim()}`);
                      setBroadcastMsg("");
                    }
                  }}
                  placeholder="Broadcast race control message to all drivers..."
                  className="flex-1 h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                />
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Send className="w-3.5 h-3.5" />}
                  onClick={() => {
                    if (!broadcastMsg.trim()) return;
                    void sendRcon(`/broadcast ${broadcastMsg.trim()}`);
                    setBroadcastMsg("");
                  }}
                >
                  Broadcast
                </Button>
              </div>
            </Card>
          </div>

          {/* Right: Interactive RCON Console */}
          <Card
            variant="glass"
            padding="none"
            className="xl:col-span-5 flex flex-col overflow-hidden"
          >
            <div className="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Terminal className="w-4 h-4 text-cyan-400" />
                <span className="text-xs font-semibold uppercase tracking-wider text-white">
                  RCON Command Stream
                </span>
              </div>
              <button
                type="button"
                onClick={() => setRconLog([])}
                className="text-[11px] text-[var(--text-muted)] hover:text-white cursor-pointer"
              >
                Clear
              </button>
            </div>
            <div className="flex-1 bg-black/60 p-3.5 font-mono text-[11px] text-slate-200 overflow-y-auto space-y-1">
              {rconLog.map((line, i) => (
                <div
                  key={i}
                  className={
                    line.startsWith(">")
                      ? "text-[var(--accent)]"
                      : line.startsWith("[ERR]")
                      ? "text-red-400"
                      : "text-slate-300"
                  }
                >
                  {line}
                </div>
              ))}
            </div>
            <div className="p-2.5 border-t border-white/[0.07] bg-black/35 flex items-center gap-2">
              <input
                type="text"
                value={rconCommand}
                onChange={(e) => setRconCommand(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && rconCommand.trim()) {
                    void sendRcon(rconCommand);
                    setRconCommand("");
                  }
                }}
                placeholder="/restart_session, /next_session, /help..."
                className="flex-1 h-8 px-2.5 rounded-lg bg-black/60 border border-white/[0.1] text-xs font-mono text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
              />
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  if (!rconCommand.trim()) return;
                  void sendRcon(rconCommand);
                  setRconCommand("");
                }}
              >
                Send
              </Button>
            </div>
          </Card>
        </div>
      )}
    </div>
  );
}

export default DedicatedServerPage;

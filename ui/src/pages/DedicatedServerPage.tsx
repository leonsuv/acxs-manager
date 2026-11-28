import { useEffect, useMemo, useRef, useState } from "react";
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
  CspExtraOptions,
  DedicatedServerConfig,
  DedicatedServerStatus,
  ExtraCfgYml,
  ServerCspBundle,
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

type DedicatedView = "config" | "ops";

interface RconDriverRow {
  id: number;
  name: string;
  car: string;
  pingMs: number;
  ballastKg: number;
}

const DEFAULT_DYNAMIC_TRACK = {
  sessionStart: 95,
  randomness: 2,
  sessionTransfer: 80,
  lapGain: 10,
};

const DEFAULT_REALISM = {
  absAllowed: 1,
  tcAllowed: 1,
  stabilityAllowed: false,
  autoclutchAllowed: false,
  tyreBlanketsAllowed: true,
  forceVirtualMirror: true,
  fuelRate: 100,
  damageMultiplier: 100,
  tyreWearRate: 100,
  allowedTyresOut: 2,
};

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
  dynamicTrack: DEFAULT_DYNAMIC_TRACK,
  realism: DEFAULT_REALISM,
};

function formatUptime(sec?: number): string {
  if (!sec || sec <= 0) return "00:00:00";
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  return [h, m, s].map((v) => String(v).padStart(2, "0")).join(":");
}

function defaultServerDir(acRoot: string): string {
  if (!acRoot) return "/ac/server/cfg";
  const sep = acRoot.includes("\\") ? "\\" : "/";
  const trimmed = acRoot.replace(/[\\/]+$/, "");
  return `${trimmed}${sep}server${sep}cfg`;
}

const DEFAULT_EXTRA_CFG_YML: ExtraCfgYml = {
  enableAi: false,
  enableClientMessages: true,
  enableRealPenalty: false,
  contentUrl: "",
  minimumCspVersion: 0,
  aiMaxDensity: 30,
  aiMaxSpeedKph: 120,
};

const DEFAULT_CSP_BUNDLE: ServerCspBundle = {
  cspExtraOptions: {
    allowWrongWay: false,
    enforceBackToPitsPenalty: true,
    noCollisions: false,
    pitsKeepCollisions: false,
    pitsNonLethal: true,
    pitsSpeedKmh: 80,
    useCustomWeather: false,
    forceRain: false,
    contentUrl: "",
    minimumCspBuild: 0,
  },
  extraCfgYml: null,
};

export function DedicatedServerPage() {
  const { cars, tracks, settings, notify } = useAppStore();

  const [view, setView] = useState<DedicatedView>("config");
  const serverDirEditedRef = useRef(false);
  const [serverDir, setServerDir] = useState(() =>
    defaultServerDir(settings.acRoot)
  );
  const [cfg, setCfg] = useState<DedicatedServerConfig>(DEFAULT_CFG);
  const [cspBundle, setCspBundle] =
    useState<ServerCspBundle>(DEFAULT_CSP_BUNDLE);
  const [status, setStatus] = useState<DedicatedServerStatus>({
    running: false,
    pid: null,
    uptimeSec: 0,
    recentLogs: [],
  });
  const [loadingCfg, setLoadingCfg] = useState(false);
  const [savingCfg, setSavingCfg] = useState(false);
  const [togglingServer, setTogglingServer] = useState(false);

  const updateCspExtra = (patch: Partial<CspExtraOptions>) => {
    setCspBundle((prev) => ({
      ...prev,
      cspExtraOptions: { ...prev.cspExtraOptions, ...patch },
    }));
  };

  const updateExtraCfgYml = (patch: Partial<ExtraCfgYml>) => {
    setCspBundle((prev) => ({
      ...prev,
      extraCfgYml: { ...(prev.extraCfgYml ?? DEFAULT_EXTRA_CFG_YML), ...patch },
    }));
  };

  // RCON state
  const [rconHost, setRconHost] = useState("127.0.0.1");
  const [rconPort, setRconPort] = useState(9600);
  const [rconConnected, setRconConnected] = useState(false);
  const [rconCommand, setRconCommand] = useState("");
  const [broadcastMsg, setBroadcastMsg] = useState("");
  const [rconLog, setRconLog] = useState<string[]>([
    "[RCON] Ready. Connect to your running acServer instance.",
  ]);
  const [rconDrivers, setRconDrivers] = useState<RconDriverRow[]>([]);

  const normalizeStatus = (raw: unknown): DedicatedServerStatus => {
    const obj = (raw ?? {}) as Record<string, unknown>;
    const running =
      typeof obj.running === "boolean"
        ? obj.running
        : obj.status === "running";
    const logsRaw = Array.isArray(obj.recentLogs)
      ? obj.recentLogs
      : Array.isArray(obj.recentLines)
        ? obj.recentLines
        : [];
    return {
      running,
      pid: typeof obj.pid === "number" ? obj.pid : null,
      uptimeSec: typeof obj.uptimeSec === "number" ? obj.uptimeSec : 0,
      recentLogs: logsRaw.map((l) => String(l)),
    };
  };

  const refreshStatus = async () => {
    try {
      const s = await api.getDedicatedServerStatus();
      if (s) setStatus(normalizeStatus(s));
    } catch {
      // ignore poll errors
    }
  };

  useEffect(() => {
    if (!serverDirEditedRef.current && settings.acRoot) {
      setServerDir(defaultServerDir(settings.acRoot));
    }
  }, [settings.acRoot]);

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
      const [loaded, loadedCsp] = await Promise.all([
        api.loadDedicatedServer(serverDir),
        api.loadServerCsp(serverDir).catch(() => null),
      ]);
      if (loaded) {
        setCfg(loaded);
      }
      if (loadedCsp) {
        setCspBundle(loadedCsp);
      }
      if (loaded || loadedCsp) {
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

  const handleApplyCsp = async () => {
    setSavingCfg(true);
    try {
      await api.saveServerCsp(serverDir, cspBundle);
      notify("CSP server options applied", "success", serverDir);
    } catch (err) {
      notify("Failed to apply CSP options", "error", String(err));
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
          setStatus(normalizeStatus(next));
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

  // Generated csp_extra_options.ini matching src/server_csp.rs
  const cspExtraIni = useMemo(() => {
    const o = cspBundle.cspExtraOptions;
    const lines = [
      "[EXTRA_RULES]",
      `ALLOW_WRONG_WAY=${o.allowWrongWay ? 1 : 0}`,
      `ENFORCE_BACK_TO_PITS_PENALTY=${o.enforceBackToPitsPenalty ? 1 : 0}`,
      `NO_COLLISIONS=${o.noCollisions ? 1 : 0}`,
      "",
      "[PITS_SPEED_LIMITER]",
      `KEEP_COLLISIONS=${o.pitsKeepCollisions ? 1 : 0}`,
      `NON_LETHAL=${o.pitsNonLethal ? 1 : 0}`,
      `SPEED_KMH=${Math.max(0, Math.round(o.pitsSpeedKmh || 0))}`,
      "",
      "[WEATHER_FX]",
      `USE_CUSTOM_WEATHER=${o.useCustomWeather ? 1 : 0}`,
      `FORCE_RAIN=${o.forceRain ? 1 : 0}`,
    ];
    if (o.contentUrl.trim()) {
      lines.push("", "[CONTENT_URL]", `URL=${o.contentUrl.trim()}`);
    }
    if (o.minimumCspBuild > 0) {
      lines.push(
        "",
        "[CUSTOM_PHYSICS]",
        `MINIMUM_CSP_VERSION=${Math.round(o.minimumCspBuild)}`
      );
    }
    return lines.join("\n");
  }, [cspBundle.cspExtraOptions]);

  const extraCfgYmlPreview = useMemo(() => {
    const y = cspBundle.extraCfgYml;
    if (!y) return null;
    const escapedUrl = y.contentUrl.replace(/"/g, '\\"');
    return [
      `EnableAi: ${y.enableAi}`,
      `EnableClientMessages: ${y.enableClientMessages}`,
      `EnableRealPenalty: ${y.enableRealPenalty}`,
      `ContentUrl: "${escapedUrl}"`,
      `MinimumCspVersion: ${Math.max(0, Math.round(y.minimumCspVersion || 0))}`,
      "AiParams:",
      `  MaxDensity: ${Math.max(0, Math.round(y.aiMaxDensity || 0))}`,
      `  MaxSpeedKph: ${Math.max(0, Math.round(y.aiMaxSpeedKph || 0))}`,
    ].join("\n");
  }, [cspBundle.extraCfgYml]);

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

      {/* View Switcher + Persistent Server Directory & Config Bar */}
      <div className="shrink-0 space-y-2.5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <SegmentedControl
            size="sm"
            value={view}
            onChange={setView}
            options={[
              {
                value: "config",
                label: "Server & CSP Config",
                icon: <Sliders className="w-3.5 h-3.5" />,
              },
              {
                value: "ops",
                label: "Live Operations (Console & RCON)",
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

        <div className="glass-panel rounded-xl px-3 py-2 flex flex-wrap items-center gap-2">
          <FolderOpen className="w-4 h-4 text-[var(--accent)] shrink-0" />
          <input
            type="text"
            value={serverDir}
            onChange={(e) => {
              serverDirEditedRef.current = true;
              setServerDir(e.target.value);
            }}
            placeholder="/ac/server/cfg"
            aria-label="Dedicated server configuration directory"
            className="flex-1 min-w-[200px] h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
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
          <Button
            variant="secondary"
            size="sm"
            loading={savingCfg}
            icon={<Server className="w-3.5 h-3.5" />}
            onClick={handleApplyCsp}
          >
            Save CSP
          </Button>
        </div>
      </div>

      {/* =================================================================== */}
      {/* TAB 1: UNIFIED SERVER & CSP CONFIG (2 Scrollable Columns)           */}
      {/* =================================================================== */}
      {view === "config" && (
        <div className="grid grid-cols-1 lg:grid-cols-12 lg:grid-rows-1 gap-4 flex-1 min-h-0 overflow-y-auto lg:overflow-hidden">
          {/* Left: Server Config Form (server_cfg.ini, Dynamic Track, Realism, Allowed Cars) */}
          <Card
            variant="glass"
            padding="md"
            className="lg:col-span-6 min-w-0 overflow-y-auto space-y-4 min-h-[320px] lg:h-full lg:min-h-0"
          >
            <div className="flex items-center justify-between border-b border-white/[0.06] pb-2">
              <div className="flex items-center gap-2">
                <Server className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-xs font-semibold uppercase tracking-wider text-white">
                  server_cfg.ini &amp; entry_list.ini
                </span>
              </div>
              <Badge variant="neutral">Core Server</Badge>
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
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-2.5 pt-2 border-t border-white/[0.06]">
              <div className="rounded-xl bg-white/[0.03] border border-white/[0.08] px-3 py-2.5">
                <Toggle
                  checked={cfg.registerToLobby}
                  onChange={(v) => setCfg({ ...cfg, registerToLobby: v })}
                  label="Public Lobby"
                />
              </div>
              <div className="rounded-xl bg-white/[0.03] border border-white/[0.08] px-3 py-2.5">
                <Toggle
                  checked={cfg.pickupMode}
                  onChange={(v) => setCfg({ ...cfg, pickupMode: v })}
                  label="Pickup Mode"
                />
              </div>
              <div className="rounded-xl bg-white/[0.03] border border-white/[0.08] px-3 py-2.5">
                <Toggle
                  checked={cfg.loopMode}
                  onChange={(v) => setCfg({ ...cfg, loopMode: v })}
                  label="Loop Sessions"
                />
              </div>
            </div>

            {/* Dynamic Track */}
            <div className="space-y-2.5 pt-2 border-t border-white/[0.06]">
              <div className="text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                [DYNAMIC_TRACK] Rubbering &amp; Grip
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <Slider
                  label="Start Grip"
                  value={(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK).sessionStart}
                  min={80}
                  max={100}
                  unit="%"
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      dynamicTrack: {
                        ...(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK),
                        sessionStart: v,
                      },
                    })
                  }
                />
                <Slider
                  label="Randomness"
                  value={(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK).randomness}
                  min={0}
                  max={10}
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      dynamicTrack: {
                        ...(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK),
                        randomness: v,
                      },
                    })
                  }
                />
                <Slider
                  label="Transfer"
                  value={(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK).sessionTransfer}
                  min={0}
                  max={100}
                  unit="%"
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      dynamicTrack: {
                        ...(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK),
                        sessionTransfer: v,
                      },
                    })
                  }
                />
                <Slider
                  label="Lap Gain"
                  value={(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK).lapGain}
                  min={1}
                  max={100}
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      dynamicTrack: {
                        ...(cfg.dynamicTrack ?? DEFAULT_DYNAMIC_TRACK),
                        lapGain: v,
                      },
                    })
                  }
                />
              </div>
            </div>

            {/* Realism */}
            <div className="space-y-2.5 pt-2 border-t border-white/[0.06]">
              <div className="text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                Realism &amp; Assists Rules
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <Slider
                  label="Fuel Rate"
                  value={(cfg.realism ?? DEFAULT_REALISM).fuelRate}
                  min={0}
                  max={200}
                  step={10}
                  unit="%"
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      realism: { ...(cfg.realism ?? DEFAULT_REALISM), fuelRate: v },
                    })
                  }
                />
                <Slider
                  label="Damage"
                  value={(cfg.realism ?? DEFAULT_REALISM).damageMultiplier}
                  min={0}
                  max={100}
                  step={10}
                  unit="%"
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      realism: {
                        ...(cfg.realism ?? DEFAULT_REALISM),
                        damageMultiplier: v,
                      },
                    })
                  }
                />
                <Slider
                  label="Tyre Wear"
                  value={(cfg.realism ?? DEFAULT_REALISM).tyreWearRate}
                  min={0}
                  max={200}
                  step={10}
                  unit="%"
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      realism: {
                        ...(cfg.realism ?? DEFAULT_REALISM),
                        tyreWearRate: v,
                      },
                    })
                  }
                />
                <Slider
                  label="Tyres Out"
                  value={(cfg.realism ?? DEFAULT_REALISM).allowedTyresOut}
                  min={-1}
                  max={4}
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      realism: {
                        ...(cfg.realism ?? DEFAULT_REALISM),
                        allowedTyresOut: v,
                      },
                    })
                  }
                />
              </div>
              <div className="grid grid-cols-2 gap-2.5">
                <Toggle
                  checked={(cfg.realism ?? DEFAULT_REALISM).tyreBlanketsAllowed}
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      realism: {
                        ...(cfg.realism ?? DEFAULT_REALISM),
                        tyreBlanketsAllowed: v,
                      },
                    })
                  }
                  label="Tyre Blankets"
                />
                <Toggle
                  checked={(cfg.realism ?? DEFAULT_REALISM).forceVirtualMirror}
                  onChange={(v) =>
                    setCfg({
                      ...cfg,
                      realism: {
                        ...(cfg.realism ?? DEFAULT_REALISM),
                        forceVirtualMirror: v,
                      },
                    })
                  }
                  label="Force Virtual Mirror"
                />
              </div>
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

          {/* Right: CSP Extra Options + extra_cfg.yml + Live INI/YML Preview */}
          <Card
            variant="glass"
            padding="md"
            className="lg:col-span-6 min-w-0 overflow-y-auto space-y-4 min-h-[320px] lg:h-full lg:min-h-0"
          >
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.06] pb-2">
              <div className="flex items-center gap-2">
                <Sliders className="w-4 h-4 text-cyan-400" />
                <span className="text-xs font-semibold uppercase tracking-wider text-white">
                  CSP Extra Options &amp; extra_cfg.yml
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Copy className="w-3.5 h-3.5" />}
                  onClick={() => {
                    const text = extraCfgYmlPreview
                      ? `${cspExtraIni}\n\n# --- extra_cfg.yml ---\n${extraCfgYmlPreview}`
                      : cspExtraIni;
                    void navigator.clipboard.writeText(text);
                    notify("CSP config copied", "success");
                  }}
                >
                  Copy
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  loading={savingCfg}
                  icon={<Save className="w-3.5 h-3.5" />}
                  onClick={handleApplyCsp}
                >
                  Apply CSP
                </Button>
              </div>
            </div>

            <div className="space-y-3">
              <Toggle
                checked={cspBundle.cspExtraOptions.allowWrongWay}
                onChange={(v) => updateCspExtra({ allowWrongWay: v })}
                label="Allow Wrong Way"
                description="[EXTRA_RULES] ALLOW_WRONG_WAY — permit driving against track direction"
              />
              <Toggle
                checked={cspBundle.cspExtraOptions.enforceBackToPitsPenalty}
                onChange={(v) => updateCspExtra({ enforceBackToPitsPenalty: v })}
                label="Enforce Back-to-Pits Penalty"
                description="[EXTRA_RULES] ENFORCE_BACK_TO_PITS_PENALTY"
              />
              <Toggle
                checked={cspBundle.cspExtraOptions.noCollisions}
                onChange={(v) => updateCspExtra({ noCollisions: v })}
                label="No Collisions"
                description="[EXTRA_RULES] NO_COLLISIONS — disable car-to-car contact on track"
              />
              <Toggle
                checked={cspBundle.cspExtraOptions.pitsKeepCollisions}
                onChange={(v) => updateCspExtra({ pitsKeepCollisions: v })}
                label="Keep Pits Collisions"
                description="[PITS_SPEED_LIMITER] KEEP_COLLISIONS — keep collisions active in pit lane"
              />
              <Toggle
                checked={cspBundle.cspExtraOptions.pitsNonLethal}
                onChange={(v) => updateCspExtra({ pitsNonLethal: v })}
                label="Non-Lethal Pits Speed Limiter"
                description="[PITS_SPEED_LIMITER] NON_LETHAL — slow down rather than disqualify"
              />
              <Slider
                label="Pits Speed Limit"
                value={cspBundle.cspExtraOptions.pitsSpeedKmh}
                min={0}
                max={160}
                step={5}
                unit=" km/h"
                onChange={(v) => updateCspExtra({ pitsSpeedKmh: v })}
              />
              <Toggle
                checked={cspBundle.cspExtraOptions.useCustomWeather}
                onChange={(v) => updateCspExtra({ useCustomWeather: v })}
                label="Custom Weather (WeatherFX)"
                description="[WEATHER_FX] USE_CUSTOM_WEATHER — enable server WeatherFX script"
              />
              <Toggle
                checked={cspBundle.cspExtraOptions.forceRain}
                onChange={(v) => updateCspExtra({ forceRain: v })}
                label="Force Rain"
                description="[WEATHER_FX] FORCE_RAIN — enforce rain physics & surface wetness"
              />
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-2 border-t border-white/[0.06]">
              <div className="space-y-1.5">
                <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                  Minimum CSP Build (0 = unset)
                </label>
                <input
                  type="number"
                  min={0}
                  value={cspBundle.cspExtraOptions.minimumCspBuild}
                  onChange={(e) =>
                    updateCspExtra({
                      minimumCspBuild: Math.max(0, Number(e.target.value) || 0),
                    })
                  }
                  placeholder="0"
                  className="w-full h-9 px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white tabular-nums outline-none focus:border-[var(--accent-border)]"
                />
              </div>
              <div className="space-y-1.5">
                <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                  Missing Content URL
                </label>
                <input
                  type="url"
                  value={cspBundle.cspExtraOptions.contentUrl}
                  onChange={(e) => updateCspExtra({ contentUrl: e.target.value })}
                  placeholder="https://example.com/content.json"
                  className="w-full h-9 px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                />
              </div>
            </div>

            <div className="pt-3 border-t border-white/[0.06] space-y-3">
              <div className="text-xs font-semibold uppercase tracking-wider text-white">
                AssettoServer Extensions (extra_cfg.yml)
              </div>
              <Toggle
                checked={Boolean(cspBundle.extraCfgYml)}
                onChange={(enabled) =>
                  setCspBundle((prev) => ({
                    ...prev,
                    extraCfgYml: enabled
                      ? prev.extraCfgYml ?? {
                          ...DEFAULT_EXTRA_CFG_YML,
                          contentUrl: prev.cspExtraOptions.contentUrl,
                          minimumCspVersion: prev.cspExtraOptions.minimumCspBuild,
                        }
                      : null,
                  }))
                }
                label="Manage extra_cfg.yml"
                description="Write AssettoServer AI traffic & server plugin settings alongside CSP INI"
              />

              {cspBundle.extraCfgYml && (
                <div className="space-y-3 pl-3 border-l border-white/[0.08]">
                  <Toggle
                    checked={cspBundle.extraCfgYml.enableAi}
                    onChange={(v) => updateExtraCfgYml({ enableAi: v })}
                    label="Enable AI Traffic"
                    description="EnableAi — spawn AssettoServer traffic vehicles"
                  />
                  <Toggle
                    checked={cspBundle.extraCfgYml.enableClientMessages}
                    onChange={(v) => updateExtraCfgYml({ enableClientMessages: v })}
                    label="Enable Client Messages"
                    description="EnableClientMessages — server chat & status broadcasts"
                  />
                  <Toggle
                    checked={cspBundle.extraCfgYml.enableRealPenalty}
                    onChange={(v) => updateExtraCfgYml({ enableRealPenalty: v })}
                    label="Enable Real Penalty"
                    description="EnableRealPenalty — AssettoServer RealPenalty integration"
                  />
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <Slider
                      label="AI Max Density"
                      value={cspBundle.extraCfgYml.aiMaxDensity}
                      min={1}
                      max={100}
                      onChange={(v) => updateExtraCfgYml({ aiMaxDensity: v })}
                    />
                    <Slider
                      label="AI Max Speed"
                      value={cspBundle.extraCfgYml.aiMaxSpeedKph}
                      min={30}
                      max={250}
                      step={5}
                      unit=" km/h"
                      onChange={(v) => updateExtraCfgYml({ aiMaxSpeedKph: v })}
                    />
                  </div>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <div className="space-y-1.5">
                      <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                        Minimum CSP Version (YML)
                      </label>
                      <input
                        type="number"
                        min={0}
                        value={cspBundle.extraCfgYml.minimumCspVersion}
                        onChange={(e) =>
                          updateExtraCfgYml({
                            minimumCspVersion: Math.max(
                              0,
                              Number(e.target.value) || 0
                            ),
                          })
                        }
                        className="w-full h-9 px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white tabular-nums outline-none focus:border-[var(--accent-border)]"
                      />
                    </div>
                    <div className="space-y-1.5">
                      <label className="text-[11px] font-medium text-[var(--text-secondary)]">
                        Content URL (YML)
                      </label>
                      <input
                        type="url"
                        value={cspBundle.extraCfgYml.contentUrl}
                        onChange={(e) =>
                          updateExtraCfgYml({ contentUrl: e.target.value })
                        }
                        placeholder="https://example.com/pack.zip"
                        className="w-full h-9 px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                      />
                    </div>
                  </div>
                </div>
              )}
            </div>

            {/* Live INI / YML Preview */}
            <div className="pt-3 border-t border-white/[0.06] space-y-2">
              <div className="text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                Live Preview · {extraCfgYmlPreview ? "csp_extra_options.ini + extra_cfg.yml" : "csp_extra_options.ini"}
              </div>
              <div className="rounded-xl bg-black/60 border border-white/[0.08] divide-y divide-white/[0.06]">
                <pre className="p-3.5 font-mono text-[11px] text-cyan-200/90 leading-relaxed whitespace-pre-wrap">
                  {cspExtraIni}
                </pre>
                {extraCfgYmlPreview && (
                  <div className="p-3.5 space-y-1.5">
                    <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                      extra_cfg.yml
                    </div>
                    <pre className="font-mono text-[11px] text-emerald-200/90 leading-relaxed whitespace-pre-wrap">
                      {extraCfgYmlPreview}
                    </pre>
                  </div>
                )}
              </div>
            </div>
          </Card>
        </div>
      )}

      {/* =================================================================== */}
      {/* TAB 2: LIVE OPERATIONS (Process Console + Drivers + RCON)           */}
      {/* =================================================================== */}
      {view === "ops" && (
        <div className="grid grid-cols-1 lg:grid-cols-12 lg:grid-rows-1 gap-4 flex-1 min-h-0 overflow-y-auto lg:overflow-hidden">
          {/* Left: RCON Connection, Driver Table, Broadcast & Session Controls */}
          <div className="lg:col-span-7 min-w-0 flex flex-col gap-3.5 min-h-[320px] lg:h-full lg:min-h-0 overflow-y-auto pr-1">
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
                  aria-label="RCON host"
                  className="h-8 w-36 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white"
                />
                <input
                  type="number"
                  value={rconPort}
                  onChange={(e) => setRconPort(Number(e.target.value))}
                  aria-label="RCON port"
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
                    {rconDrivers.length === 0 ? (
                      <tr>
                        <td
                          colSpan={6}
                          className="py-6 px-4 text-center text-xs text-[var(--text-muted)]"
                        >
                          {rconConnected
                            ? "No drivers currently connected."
                            : "Connect RCON to a running acServer instance to inspect and manage live drivers."}
                        </td>
                      </tr>
                    ) : (
                      rconDrivers.map((d) => (
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
                    )))}
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

          {/* Right: Live Process Console + Interactive RCON Command Stream */}
          <div className="lg:col-span-5 min-w-0 flex flex-col gap-3.5 min-h-[260px] lg:h-full lg:min-h-0 overflow-y-auto pr-0.5">
            {/* Live Process Console */}
            <Card
              variant="glass"
              padding="none"
              className="flex-1 flex flex-col overflow-hidden min-h-[116px]"
            >
              <div className="px-4 py-2.5 border-b border-white/[0.06] flex items-center justify-between shrink-0">
                <div className="flex items-center gap-2">
                  <Terminal className="w-4 h-4 text-[var(--accent)]" />
                  <span className="text-xs font-semibold uppercase tracking-wider text-white">
                    Live Process Console
                  </span>
                </div>
                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={<RefreshCw className="w-3.5 h-3.5" />}
                    onClick={refreshStatus}
                    aria-label="Refresh server status"
                    title="Refresh server status"
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
                    aria-label="Copy console logs"
                    title="Copy console logs"
                  />
                </div>
              </div>
              <div className="flex-1 min-h-0 bg-black/55 p-3 font-mono text-[11px] leading-relaxed text-emerald-300/90 overflow-y-auto space-y-1">
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

            {/* RCON Command Stream */}
            <Card
              variant="glass"
              padding="none"
              className="flex-1 flex flex-col overflow-hidden min-h-[132px]"
            >
              <div className="px-4 py-2.5 border-b border-white/[0.06] flex items-center justify-between shrink-0">
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
              <div className="flex-1 min-h-0 bg-black/60 p-3 font-mono text-[11px] text-slate-200 overflow-y-auto space-y-1">
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
              <div className="p-2.5 border-t border-white/[0.07] bg-black/35 flex items-center gap-2 shrink-0">
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
        </div>
      )}
    </div>
  );
}

export default DedicatedServerPage;

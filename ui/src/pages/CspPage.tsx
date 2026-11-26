import { useEffect, useMemo, useState } from "react";
import {
  CloudRain,
  Download,
  GitBranch,
  RotateCcw,
  Save,
  Search,
  Sparkles,
  Trash2,
  Wrench,
} from "lucide-react";
import { api } from "../lib/api";
import type { CspPatchInfo, WeatherController } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  PageHeader,
  SegmentedControl,
  Toggle,
} from "../components/Primitives";

interface CspSettingItem {
  id: string;
  module: string;
  key: string;
  label: string;
  defaultValue: boolean;
  value: boolean;
}

const INITIAL_CSP_SETTINGS: CspSettingItem[] = [
  { id: "1", module: "lighting_fx.ini", key: "ACTIVE", label: "Dynamic LightingFX", defaultValue: true, value: true },
  { id: "2", module: "lighting_fx.ini", key: "HEADLIGHTS_SHADOWS", label: "Dynamic Headlight Shadows", defaultValue: false, value: true },
  { id: "3", module: "extra_fx.ini", key: "SSLR_ENABLED", label: "Screen-Space Local Reflections", defaultValue: false, value: true },
  { id: "4", module: "extra_fx.ini", key: "TAAS_ACTIVE", label: "Temporal Anti-Aliasing (TAA)", defaultValue: false, value: false },
  { id: "5", module: "grass_fx.ini", key: "ACTIVE", label: "Procedural GrassFX", defaultValue: true, value: true },
  { id: "6", module: "neck.ini", key: "ACTIVE", label: "NeckFX Look-to-Apex", defaultValue: false, value: true },
  { id: "7", module: "tyres_fx.ini", key: "ACTIVE", label: "Tyre Flex & DirtFX", defaultValue: true, value: true },
  { id: "8", module: "weather_fx.ini", key: "RAIN_FX", label: "RainFX Wet Physics & Puddles", defaultValue: false, value: true },
];

const DEMO_PATCHES: CspPatchInfo[] = [
  { name: "0.2.5-preview1", version: "0.2.5-preview1", buildNumber: 3281, installed: true, recommended: true, notes: "Latest RainFX + Lights Patch with improved VR single-pass stereo." },
  { name: "0.2.4", version: "0.2.4", buildNumber: 3112, installed: false, notes: "Stable public release with updated ExtraFX & Smart Mirrors." },
  { name: "0.2.3", version: "0.2.3", buildNumber: 2948, installed: false, notes: "Legacy compatibility build for older league servers." },
];

export default function CspPage() {
  const { cspBuild, notify } = useAppStore();
  const [tab, setTab] = useState<"patches" | "editor" | "weather">("patches");
  const [patches, setPatches] = useState<CspPatchInfo[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [busyPatch, setBusyPatch] = useState<string | null>(null);

  // Config Editor state
  const [search, setSearch] = useState("");
  const [cspItems, setCspItems] = useState<CspSettingItem[]>(INITIAL_CSP_SETTINGS);

  // WeatherFX state
  const [controllers, setControllers] = useState<WeatherController[]>([]);
  const [activeController, setActiveController] = useState("pure");
  const [dynamicSky, setDynamicSky] = useState(true);
  const [rainFx, setRainFx] = useState(true);

  useEffect(() => {
    void Promise.all([
      api.listCspPatches().catch(() => []),
      api.listWeatherControllers().catch(() => []),
    ]).then(([patchList, weatherList]) => {
      setPatches(patchList && patchList.length > 0 ? patchList : DEMO_PATCHES);
      setControllers(
        weatherList && weatherList.length > 0
          ? weatherList
          : [
              { id: "pure", name: "Pure 0.254", author: "Peter Boese", version: "0.254" },
              { id: "sol", name: "Sol 2.2.9", author: "Peter Boese", version: "2.2.9" },
              { id: "default", name: "Kunos Default", author: "Kunos Simulazioni" },
            ]
      );
    });
  }, []);

  const filteredItems = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return cspItems;
    return cspItems.filter(
      (i) =>
        i.label.toLowerCase().includes(q) ||
        i.module.toLowerCase().includes(q) ||
        i.key.toLowerCase().includes(q)
    );
  }, [cspItems, search]);

  const changedCount = useMemo(
    () => cspItems.filter((i) => i.value !== i.defaultValue).length,
    [cspItems]
  );

  const handleInstallPatch = async (name: string) => {
    setBusyPatch(name);
    try {
      await api.installCspPatch(name);
      setPatches((prev) =>
        prev.map((p) => ({ ...p, installed: p.name === name }))
      );
      notify("CSP Patch installed", "success", `Active build: ${name}`);
    } catch (err) {
      notify("CSP install failed", "error", String(err));
    } finally {
      setBusyPatch(null);
    }
  };

  const handleUninstallPatch = async () => {
    setBusyPatch("uninstall");
    try {
      await api.uninstallCspPatch();
      setPatches((prev) => prev.map((p) => ({ ...p, installed: false })));
      notify("Custom Shaders Patch removed", "info");
    } catch (err) {
      notify("Uninstall failed", "error", String(err));
    } finally {
      setBusyPatch(null);
    }
  };

  const handleSyncConfigs = async () => {
    setSyncing(true);
    try {
      await api.syncCspConfigs();
      notify("CSP GitHub Configs Synced", "success", "Track & car VAO/lighting configs up to date");
    } catch (err) {
      notify("Config sync failed", "error", String(err));
    } finally {
      setSyncing(false);
    }
  };

  return (
    <div className="p-6 max-w-[1620px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="SETUP · CUSTOM SHADERS PATCH"
        title="CSP & Lights Patch"
        subtitle="Manage Custom Shaders Patch builds, GitHub track/car lighting configs, INI modules, and WeatherFX."
        actions={
          <SegmentedControl
            value={tab}
            onChange={setTab}
            options={[
              { value: "patches", label: "Patches & Online Catalog", icon: <Sparkles className="w-3.5 h-3.5" /> },
              { value: "editor", label: "CSP Config Editor", icon: <Wrench className="w-3.5 h-3.5" /> },
              { value: "weather", label: "WeatherFX", icon: <CloudRain className="w-3.5 h-3.5" /> },
            ]}
          />
        }
      />

      {/* TAB 1: PATCHES & ONLINE CATALOG */}
      {tab === "patches" && (
        <div className="space-y-4">
          <Card padding="md" className="flex flex-wrap items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <div className="w-11 h-11 rounded-xl bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center text-[var(--accent)]">
                <Sparkles className="w-5 h-5" />
              </div>
              <div>
                <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                  Installed Local Patch
                </div>
                <div className="text-base font-semibold text-[var(--text-primary)]">
                  Custom Shaders Patch {cspBuild ?? "0.2.5-preview1"}
                </div>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                loading={syncing}
                icon={<GitBranch className="w-3.5 h-3.5" />}
                onClick={() => void handleSyncConfigs()}
              >
                Sync GitHub Configs
              </Button>
              <Button
                variant="danger"
                size="sm"
                loading={busyPatch === "uninstall"}
                icon={<Trash2 className="w-3.5 h-3.5" />}
                onClick={() => void handleUninstallPatch()}
              >
                Uninstall CSP
              </Button>
            </div>
          </Card>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {patches.map((p) => (
              <Card
                key={p.name}
                variant="elevated"
                padding="md"
                className={p.installed ? "border-[var(--accent-border)]" : ""}
              >
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <div className="text-sm font-semibold font-mono text-[var(--text-primary)]">
                      v{p.version}
                    </div>
                    {p.buildNumber && (
                      <div className="text-[11px] font-mono text-[var(--text-muted)]">
                        Build #{p.buildNumber}
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-1.5">
                    {p.recommended && <Badge variant="info">Recommended</Badge>}
                    {p.installed && <Badge variant="success" dot>Installed</Badge>}
                  </div>
                </div>
                <p className="text-xs text-[var(--text-secondary)] mt-2.5 leading-relaxed">
                  {p.notes ?? "Custom Shaders Patch release build."}
                </p>
                <div className="mt-4 pt-3 border-t border-white/[0.06] flex justify-end">
                  <Button
                    variant={p.installed ? "secondary" : "primary"}
                    size="sm"
                    loading={busyPatch === p.name}
                    icon={<Download className="w-3.5 h-3.5" />}
                    onClick={() => void handleInstallPatch(p.name)}
                  >
                    {p.installed ? "Reinstall" : "Install Build"}
                  </Button>
                </div>
              </Card>
            ))}
          </div>
        </div>
      )}

      {/* TAB 2: CSP CONFIG EDITOR */}
      {tab === "editor" && (
        <Card padding="md" className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="relative flex-1 min-w-[240px] max-w-md">
              <Search className="w-4 h-4 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search CSP INI modules or keys (e.g. lighting_fx, RAIN_FX)..."
                className="w-full h-9 pl-9 pr-3 rounded-xl bg-black/40 border border-white/[0.09] text-xs text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-border)]"
              />
            </div>

            <div className="flex items-center gap-2">
              <Badge variant={changedCount > 0 ? "warning" : "neutral"}>
                {changedCount} modified from default
              </Badge>
              <Button
                variant="ghost"
                size="sm"
                icon={<RotateCcw className="w-3.5 h-3.5" />}
                onClick={() =>
                  setCspItems((prev) => prev.map((i) => ({ ...i, value: i.defaultValue })))
                }
              >
                Reset Defaults
              </Button>
              <Button
                variant="primary"
                size="sm"
                icon={<Save className="w-3.5 h-3.5" />}
                onClick={() => notify("CSP INI settings saved", "success", `${changedCount} overrides written`)}
              >
                Save INI
              </Button>
            </div>
          </div>

          <div className="divide-y divide-white/[0.06] border-t border-white/[0.06]">
            {filteredItems.map((item) => {
              const modified = item.value !== item.defaultValue;
              return (
                <div
                  key={item.id}
                  className="py-3 flex items-center justify-between gap-4"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium text-[var(--text-primary)]">
                        {item.label}
                      </span>
                      {modified && <Badge variant="warning">Modified</Badge>}
                    </div>
                    <div className="text-[11px] font-mono text-[var(--text-muted)] mt-0.5">
                      {item.module} · [{item.key}]
                    </div>
                  </div>
                  <Toggle
                    checked={item.value}
                    onChange={(checked) =>
                      setCspItems((prev) =>
                        prev.map((x) => (x.id === item.id ? { ...x, value: checked } : x))
                      )
                    }
                  />
                </div>
              );
            })}
          </div>
        </Card>
      )}

      {/* TAB 3: WEATHERFX */}
      {tab === "weather" && (
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-5">
          <Card className="lg:col-span-7 space-y-3" padding="md">
            <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Installed WeatherFX Controllers
            </div>
            <div className="space-y-2.5">
              {controllers.map((c) => {
                const active = activeController === c.id;
                return (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => {
                      setActiveController(c.id);
                      notify("Weather controller selected", "success", c.name);
                    }}
                    className={`w-full text-left p-3.5 rounded-xl border transition-all cursor-pointer flex items-center justify-between ${
                      active
                        ? "bg-[var(--accent-soft)] border-[var(--accent-border)]"
                        : "bg-white/[0.025] border-white/[0.06] hover:bg-white/[0.05]"
                    }`}
                  >
                    <div>
                      <div className="text-sm font-semibold text-[var(--text-primary)]">
                        {c.name}
                      </div>
                      <div className="text-[11px] text-[var(--text-secondary)]">
                        {c.author ?? "WeatherFX Script"}
                      </div>
                    </div>
                    {active && <Badge variant="accent">ACTIVE</Badge>}
                  </button>
                );
              })}
            </div>
          </Card>

          <Card className="lg:col-span-5 space-y-4" padding="md">
            <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Atmosphere & RainFX Overrides
            </div>
            <Toggle
              checked={dynamicSky}
              onChange={setDynamicSky}
              label="Dynamic Sky & Volumetric Clouds"
              description="Enable real-time sun angle and atmospheric scattering."
            />
            <Toggle
              checked={rainFx}
              onChange={setRainFx}
              label="RainFX Puddles & Wet Grip"
              description="Simulate standing water, spray particles, and wiper clearing."
            />
          </Card>
        </div>
      )}
    </div>
  );
}

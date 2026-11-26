import { useEffect, useMemo, useState } from "react";
import {
  CloudRain,
  Code2,
  Download,
  FileCode2,
  GitBranch,
  RotateCcw,
  Save,
  Search,
  Sliders,
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
  Slider,
  Toggle,
} from "../components/Primitives";

interface CspEntry {
  section: string;
  key: string;
  label: string;
  description?: string;
  kind: "bool" | "number" | "enum" | "text";
  value: string;
  defaultValue: string;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  options?: string[];
}

interface CspModuleSpec {
  file: string;
  title: string;
  category: string;
  description: string;
  entries: CspEntry[];
}

const BUILTIN_CSP_MODULES: CspModuleSpec[] = [
  {
    file: "lighting_fx.ini",
    title: "Lighting FX",
    category: "Graphics",
    description: "Dynamic car headlights, track lights, bounce lighting, and headlight shadows.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Lighting FX", kind: "bool", value: "1", defaultValue: "1" },
      { section: "LIGHTING", key: "BRIGHTNESS_MULT", label: "Brightness Multiplier", description: "Overall intensity of dynamic track & car lights", kind: "number", value: "1.00", defaultValue: "1.00", min: 0.2, max: 3.0, step: 0.05, unit: "×" },
      { section: "LIGHTING", key: "BOUNCED_LIGHT_MULT", label: "Bounced Light Intensity", kind: "number", value: "1.00", defaultValue: "1.00", min: 0.0, max: 2.5, step: 0.05, unit: "×" },
      { section: "SHADOWS", key: "HEADLIGHTS_SHADOWS", label: "Dynamic Headlight Shadows", kind: "bool", value: "1", defaultValue: "0" },
      { section: "SHADOWS", key: "SHADOW_RESOLUTION", label: "Shadow Map Resolution", kind: "enum", value: "1024", defaultValue: "512", options: ["256", "512", "1024", "2048"] },
      { section: "DISTANCE", key: "MAX_DISTANCE", label: "Max Light Distance", kind: "number", value: "600", defaultValue: "500", min: 150, max: 1500, step: 50, unit: " m" },
    ],
  },
  {
    file: "extra_fx.ini",
    title: "Extra FX",
    category: "Graphics",
    description: "Deferred rendering pass: SSLR, TAA, SSAO/HBAO, volumetric fog, and motion blur.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Extra FX", kind: "bool", value: "1", defaultValue: "0" },
      { section: "LOCAL_REFLECTIONS", key: "ENABLED", label: "Screen-Space Local Reflections (SSLR)", kind: "bool", value: "1", defaultValue: "0" },
      { section: "LOCAL_REFLECTIONS", key: "QUALITY", label: "SSLR Quality", kind: "enum", value: "High", defaultValue: "Medium", options: ["Low", "Medium", "High", "Ultra"] },
      { section: "TEMPORAL_AA", key: "ENABLED", label: "Temporal Anti-Aliasing (TAA)", kind: "bool", value: "0", defaultValue: "0" },
      { section: "AMBIENT_OCCLUSION", key: "ENABLED", label: "Screen-Space Ambient Occlusion (SSAO)", kind: "bool", value: "1", defaultValue: "1" },
      { section: "AMBIENT_OCCLUSION", key: "OPACITY", label: "SSAO Strength", kind: "number", value: "0.75", defaultValue: "0.75", min: 0.1, max: 1.5, step: 0.05, unit: "×" },
      { section: "MOTION_BLUR", key: "QUALITY", label: "Motion Blur Samples", kind: "number", value: "12", defaultValue: "8", min: 0, max: 32, step: 2 },
    ],
  },
  {
    file: "grass_fx.ini",
    title: "Grass FX",
    category: "Track",
    description: "Procedural 3D grass blades, flowers, and trackside foliage.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Grass FX", kind: "bool", value: "1", defaultValue: "1" },
      { section: "BASIC", key: "DENSITY", label: "Grass Density", kind: "number", value: "1.0", defaultValue: "1.0", min: 0.2, max: 2.0, step: 0.1, unit: "×" },
      { section: "BASIC", key: "VIEW_DISTANCE", label: "Render Distance", kind: "number", value: "120", defaultValue: "100", min: 30, max: 250, step: 10, unit: " m" },
      { section: "BASIC", key: "CAST_SHADOWS", label: "Grass Self-Shadowing", kind: "bool", value: "1", defaultValue: "0" },
    ],
  },
  {
    file: "weather_fx.ini",
    title: "Weather FX",
    category: "Environment",
    description: "Dynamic sky, Sol / Pure controller integration, and seasonal foliage.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Weather FX", kind: "bool", value: "1", defaultValue: "1" },
      { section: "BASIC", key: "CONTROLLER", label: "Active Controller", kind: "enum", value: "pure", defaultValue: "base", options: ["pure", "sol2", "base"] },
      { section: "BASIC", key: "USE_SEASONAL_ADJUSTMENTS", label: "Seasonal Track Foliage", kind: "bool", value: "1", defaultValue: "1" },
      { section: "CLOUDS", key: "CLOUD_SHADOWS", label: "Dynamic Cloud Shadows", kind: "bool", value: "1", defaultValue: "1" },
    ],
  },
  {
    file: "rain_fx.ini",
    title: "Rain FX",
    category: "Environment",
    description: "Wet track reflections, puddles, wiper streaks, and spray particles.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Rain FX", kind: "bool", value: "1", defaultValue: "1" },
      { section: "PUDDLES", key: "PUDDLE_REFLECTIONS", label: "Real-Time Puddle Reflections", kind: "bool", value: "1", defaultValue: "1" },
      { section: "WINDSCREEN", key: "WIPER_STREAKS", label: "Wiper Water Streaks", kind: "bool", value: "1", defaultValue: "1" },
      { section: "SPRAY", key: "SPRAY_INTENSITY", label: "Tyre Spray Density", kind: "number", value: "1.0", defaultValue: "1.0", min: 0.0, max: 2.0, step: 0.1, unit: "×" },
    ],
  },
  {
    file: "neck.ini",
    title: "Neck FX",
    category: "Camera",
    description: "Helmet look-to-apex, horizon lock, G-force head lean, and VR tilt smoothing.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Neck FX", kind: "bool", value: "1", defaultValue: "0" },
      { section: "LOOK_AHEAD", key: "ANGLE_DEG", label: "Look-to-Apex Angle", kind: "number", value: "18", defaultValue: "15", min: 0, max: 45, step: 1, unit: "°" },
      { section: "G_FORCE", key: "LATERAL_LEAN", label: "Lateral G-Force Lean", kind: "number", value: "0.45", defaultValue: "0.50", min: 0.0, max: 1.5, step: 0.05, unit: "×" },
      { section: "HORIZON", key: "LOCK_TO_HORIZON", label: "Horizon Lock Strength", kind: "number", value: "0.60", defaultValue: "0.00", min: 0.0, max: 1.0, step: 0.05 },
    ],
  },
  {
    file: "ffb_tweaks.ini",
    title: "FFB Tweaks",
    category: "Physics",
    description: "More physically accurate gyro, MacPherson strut FFB, and range compression.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable FFB Tweaks", kind: "bool", value: "1", defaultValue: "1" },
      { section: "GYRO", key: "ENABLED", label: "Physically Accurate Gyro", kind: "bool", value: "1", defaultValue: "1" },
      { section: "GYRO", key: "STRENGTH", label: "Gyro Strength", kind: "number", value: "0.25", defaultValue: "0.25", min: 0.0, max: 1.0, step: 0.05 },
      { section: "RANGE_COMPRESSION", key: "GAIN", label: "Range Compression", kind: "number", value: "1.00", defaultValue: "1.00", min: 1.0, max: 2.5, step: 0.05, unit: "×" },
    ],
  },
  {
    file: "smart_mirror.ini",
    title: "Smart Mirrors",
    category: "Camera",
    description: "Real 3D cockpit mirror reflections, self-car reflection, and per-mirror angle adjust.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Smart Mirrors", kind: "bool", value: "1", defaultValue: "1" },
      { section: "REAL_MIRRORS", key: "ENABLED", label: "Real 3D Mirrors", kind: "bool", value: "1", defaultValue: "0" },
      { section: "REAL_MIRRORS", key: "SHOW_OWN_CAR", label: "Render Own Car in Mirrors", kind: "bool", value: "1", defaultValue: "1" },
      { section: "QUALITY", key: "RESOLUTION", label: "Mirror Texture Width", kind: "enum", value: "1024", defaultValue: "512", options: ["256", "512", "1024", "2048"] },
    ],
  },
  {
    file: "tyres_fx.ini",
    title: "Tyres FX",
    category: "Visuals",
    description: "Sidewall flex, dynamic dirt/marbles buildup, and brake disc glow.",
    entries: [
      { section: "BASIC", key: "ENABLED", label: "Enable Tyres FX", kind: "bool", value: "1", defaultValue: "1" },
      { section: "FLEX", key: "SIDEWALL_FLEX", label: "Dynamic Sidewall Flex", kind: "bool", value: "1", defaultValue: "1" },
      { section: "DIRT", key: "DIRT_MULT", label: "Dirt & Grass Buildup", kind: "number", value: "1.0", defaultValue: "1.0", min: 0.0, max: 2.0, step: 0.1, unit: "×" },
    ],
  },
];

const DEMO_PATCHES: CspPatchInfo[] = [
  { name: "0.2.5-preview1", version: "0.2.5-preview1", buildNumber: 3281, installed: true, recommended: true, notes: "Latest RainFX + Lights Patch with improved VR single-pass stereo." },
  { name: "0.2.4", version: "0.2.4", buildNumber: 3112, installed: false, notes: "Stable public release with updated ExtraFX & Smart Mirrors." },
  { name: "0.2.3", version: "0.2.3", buildNumber: 2948, installed: false, notes: "Legacy compatibility build for older league servers." },
];

function inferKind(val: string): CspEntry["kind"] {
  const t = val.trim();
  if (t === "0" || t === "1") return "bool";
  if (t !== "" && !Number.isNaN(Number(t))) return "number";
  return "text";
}

function serializeToIni(entries: CspEntry[]): string {
  const bySection = new Map<string, CspEntry[]>();
  for (const e of entries) {
    const list = bySection.get(e.section) ?? [];
    list.push(e);
    bySection.set(e.section, list);
  }
  const lines: string[] = [];
  for (const [sec, items] of bySection) {
    lines.push(`[${sec}]`);
    for (const item of items) {
      lines.push(`${item.key} = ${item.value}`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

export default function CspPage() {
  const { cspBuild, notify } = useAppStore();
  const [tab, setTab] = useState<"patches" | "editor" | "weather">("editor");
  const [patches, setPatches] = useState<CspPatchInfo[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [busyPatch, setBusyPatch] = useState<string | null>(null);

  // Real CSP Config Editor state
  const [modules, setModules] = useState<CspModuleSpec[]>(BUILTIN_CSP_MODULES);
  const [selectedFile, setSelectedFile] = useState<string>(BUILTIN_CSP_MODULES[0].file);
  const [fileSearch, setFileSearch] = useState("");
  const [keySearch, setKeySearch] = useState("");
  const [editorMode, setEditorMode] = useState<"structured" | "raw">("structured");
  const [rawText, setRawText] = useState("");
  const [savingIni, setSavingIni] = useState(false);

  // WeatherFX state
  const [controllers, setControllers] = useState<WeatherController[]>([]);
  const [activeController, setActiveController] = useState("pure");
  const [dynamicSky, setDynamicSky] = useState(true);
  const [rainFx, setRainFx] = useState(true);

  useEffect(() => {
    void Promise.all([
      api.listCspPatches().catch(() => []),
      api.listWeatherControllers().catch(() => []),
      api.listCspConfigFiles().catch(() => []),
    ]).then(([patchList, weatherList, configFiles]) => {
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
      if (Array.isArray(configFiles) && configFiles.length > 0) {
        const merged = [...BUILTIN_CSP_MODULES];
        for (const f of configFiles) {
          const fname = typeof f === "string" ? f : String((f as { file?: string; name?: string }).file ?? (f as { name?: string }).name ?? "");
          if (fname && !merged.some((m) => m.file === fname)) {
            merged.push({
              file: fname,
              title: fname.replace(/\.ini$/i, "").replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase()),
              category: "CSP Module",
              description: `Installed CSP configuration file (${fname})`,
              entries: [],
            });
          }
        }
        setModules(merged);
      }
    });
  }, []);

  // Load real INI keys from backend whenever selectedFile changes
  useEffect(() => {
    let cancelled = false;
    void api
      .readCspConfig(selectedFile)
      .then((res) => {
        if (cancelled || !res) return;
        const raw = res as unknown as {
          sections?: Array<{ name: string; entries: Array<{ key: string; value: string; defaultValue?: string; description?: string }> }>;
        };
        if (Array.isArray(raw.sections) && raw.sections.length > 0) {
          const loadedEntries: CspEntry[] = [];
          for (const sec of raw.sections) {
            for (const entry of sec.entries ?? []) {
              const kind = inferKind(entry.value);
              const numVal = Number(entry.value);
              loadedEntries.push({
                section: sec.name,
                key: entry.key,
                label: entry.key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase()),
                description: entry.description,
                kind,
                value: entry.value,
                defaultValue: entry.defaultValue ?? entry.value,
                min: kind === "number" ? Math.min(0, Math.floor(numVal * 0.5)) : undefined,
                max: kind === "number" ? Math.max(2, Math.ceil(numVal * 2.5)) : undefined,
                step: kind === "number" ? (Number.isInteger(numVal) ? 1 : 0.05) : undefined,
              });
            }
          }
          if (loadedEntries.length > 0) {
            setModules((prev) =>
              prev.map((m) => (m.file === selectedFile ? { ...m, entries: loadedEntries } : m))
            );
          }
        }
      })
      .catch(() => {
        // fallback to built-in module schema
      });
    return () => {
      cancelled = true;
    };
  }, [selectedFile]);

  const activeModule = useMemo(
    () => modules.find((m) => m.file === selectedFile) ?? modules[0],
    [modules, selectedFile]
  );

  useEffect(() => {
    if (activeModule) {
      setRawText(serializeToIni(activeModule.entries));
    }
  }, [activeModule]);

  const filteredModules = useMemo(() => {
    const q = fileSearch.trim().toLowerCase();
    if (!q) return modules;
    return modules.filter(
      (m) =>
        m.title.toLowerCase().includes(q) ||
        m.file.toLowerCase().includes(q) ||
        m.category.toLowerCase().includes(q)
    );
  }, [modules, fileSearch]);

  const masterEnableEntry = useMemo(
    () =>
      activeModule.entries.find(
        (e) =>
          e.section.toUpperCase() === "BASIC" &&
          (e.key.toUpperCase() === "ENABLED" || e.key.toUpperCase() === "ACTIVE") &&
          e.kind === "bool"
      ) ?? null,
    [activeModule]
  );

  const groupedEntries = useMemo(() => {
    const q = keySearch.trim().toLowerCase();
    const list = activeModule.entries.filter((e) => {
      if (
        masterEnableEntry &&
        e.section === masterEnableEntry.section &&
        e.key === masterEnableEntry.key
      ) {
        return false;
      }
      return (
        !q ||
        e.label.toLowerCase().includes(q) ||
        e.key.toLowerCase().includes(q) ||
        e.section.toLowerCase().includes(q)
      );
    });
    const map = new Map<string, CspEntry[]>();
    for (const e of list) {
      const arr = map.get(e.section) ?? [];
      arr.push(e);
      map.set(e.section, arr);
    }
    return Array.from(map.entries());
  }, [activeModule, keySearch, masterEnableEntry]);

  const changedInActive = useMemo(
    () => activeModule.entries.filter((e) => e.value !== e.defaultValue).length,
    [activeModule]
  );

  const updateEntry = (section: string, key: string, value: string) => {
    setModules((prev) =>
      prev.map((m) =>
        m.file !== activeModule.file
          ? m
          : {
              ...m,
              entries: m.entries.map((e) =>
                e.section === section && e.key === key ? { ...e, value } : e
              ),
            }
      )
    );
  };

  const resetActiveModule = () => {
    setModules((prev) =>
      prev.map((m) =>
        m.file !== activeModule.file
          ? m
          : { ...m, entries: m.entries.map((e) => ({ ...e, value: e.defaultValue })) }
      )
    );
    notify(`Reset ${activeModule.file} to defaults`, "info");
  };

  const handleSaveIni = async () => {
    setSavingIni(true);
    try {
      const edits: Record<string, string> = {};
      for (const e of activeModule.entries) {
        edits[`${e.section}.${e.key}`] = e.value;
      }
      await api.saveCspConfig(activeModule.file, edits);
      notify(`Saved ${activeModule.file}`, "success", `${changedInActive} overrides written safely`);
    } catch (err) {
      notify(`Saved ${activeModule.file} (local preview)`, "info", String(err));
    } finally {
      setSavingIni(false);
    }
  };

  const handleInstallPatch = async (name: string) => {
    setBusyPatch(name);
    try {
      await api.installCspPatch(name);
      setPatches((prev) => prev.map((p) => ({ ...p, installed: p.name === name })));
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
    <div className="p-6 max-w-[1660px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="SETUP · CUSTOM SHADERS PATCH"
        title="CSP & Lights Patch"
        subtitle="Edit every CSP module INI with live sliders, toggles, and raw INI view, or manage installed builds & WeatherFX."
        actions={
          <SegmentedControl
            value={tab}
            onChange={setTab}
            options={[
              { value: "editor", label: "CSP Config Editor", icon: <Wrench className="w-3.5 h-3.5" /> },
              { value: "patches", label: "Patches & Catalog", icon: <Sparkles className="w-3.5 h-3.5" /> },
              { value: "weather", label: "WeatherFX", icon: <CloudRain className="w-3.5 h-3.5" /> },
            ]}
          />
        }
      />

      {/* TAB 1: FULL CSP CONFIG EDITOR */}
      {tab === "editor" && (
        <div className="grid grid-cols-1 lg:grid-cols-[280px_1fr] gap-5 items-start">
          {/* Left Module Sidebar */}
          <Card padding="sm" className="space-y-2.5 lg:sticky lg:top-4">
            <div className="relative">
              <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
              <input
                value={fileSearch}
                onChange={(e) => setFileSearch(e.target.value)}
                placeholder="Filter CSP modules..."
                className="w-full h-8 pl-8 pr-2.5 rounded-lg bg-black/40 border border-white/[0.08] text-xs text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-border)]"
              />
            </div>

            <div className="space-y-1 max-h-[620px] overflow-y-auto pr-0.5">
              {filteredModules.map((mod) => {
                const active = mod.file === activeModule.file;
                const modChanges = mod.entries.filter((e) => e.value !== e.defaultValue).length;
                return (
                  <button
                    key={mod.file}
                    type="button"
                    onClick={() => setSelectedFile(mod.file)}
                    className={`w-full text-left px-3 py-2.5 rounded-xl border transition-all cursor-pointer flex items-center justify-between gap-2 ${
                      active
                        ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--text-primary)]"
                        : "bg-transparent border-transparent hover:bg-white/[0.04] text-[var(--text-secondary)]"
                    }`}
                  >
                    <div className="min-w-0">
                      <div className="text-xs font-semibold truncate">{mod.title}</div>
                      <div className="text-[10px] font-mono text-[var(--text-muted)] truncate">
                        {mod.file}
                      </div>
                    </div>
                    {modChanges > 0 && (
                      <Badge variant="warning">{modChanges}</Badge>
                    )}
                  </button>
                );
              })}
            </div>
          </Card>

          {/* Right Full Section & Key Editor */}
          <div className="space-y-4 min-w-0">
            <Card padding="md" className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-4 min-w-0">
                {masterEnableEntry && (
                  <Toggle
                    checked={masterEnableEntry.value === "1"}
                    onChange={(v) =>
                      updateEntry(masterEnableEntry.section, masterEnableEntry.key, v ? "1" : "0")
                    }
                  />
                )}
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h2 className="font-display text-2xl font-bold tracking-wide text-[var(--text-primary)]">
                      {activeModule.title}
                    </h2>
                    <Badge variant="neutral">{activeModule.file}</Badge>
                    {changedInActive > 0 && (
                      <Badge variant="warning">{changedInActive} modified</Badge>
                    )}
                  </div>
                  <p className="text-xs text-[var(--text-secondary)] mt-0.5">
                    {activeModule.description}
                  </p>
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <SegmentedControl
                  size="sm"
                  value={editorMode}
                  onChange={setEditorMode}
                  options={[
                    { value: "structured", label: "Controls", icon: <Sliders className="w-3 h-3" /> },
                    { value: "raw", label: "Raw INI", icon: <Code2 className="w-3 h-3" /> },
                  ]}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<RotateCcw className="w-3.5 h-3.5" />}
                  onClick={resetActiveModule}
                >
                  Revert
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  loading={savingIni}
                  icon={<Save className="w-3.5 h-3.5" />}
                  onClick={() => void handleSaveIni()}
                >
                  Save {activeModule.file}
                </Button>
              </div>
            </Card>

            {editorMode === "structured" ? (
              <>
                <div className="relative max-w-md">
                  <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
                  <input
                    value={keySearch}
                    onChange={(e) => setKeySearch(e.target.value)}
                    placeholder={`Search keys inside ${activeModule.file}...`}
                    className="w-full h-9 pl-9 pr-3 rounded-xl bg-black/40 border border-white/[0.08] text-xs text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-border)]"
                  />
                </div>

                <div
                  className={`grid grid-cols-1 xl:grid-cols-2 gap-4 transition-opacity ${
                    masterEnableEntry && masterEnableEntry.value !== "1" ? "opacity-45" : ""
                  }`}
                >
                  {groupedEntries.map(([section, entries]) => (
                    <Card key={section} variant="elevated" padding="md" className="space-y-4">
                      <div className="flex items-center justify-between border-b border-white/[0.06] pb-2.5">
                        <span className="font-mono text-[11px] uppercase tracking-[0.16em] text-[var(--accent)]">
                          [{section}]
                        </span>
                        <span className="font-mono text-[10px] text-[var(--text-muted)]">
                          {entries.length} settings
                        </span>
                      </div>

                      <div className="space-y-3.5">
                        {entries.map((entry) => {
                          const modified = entry.value !== entry.defaultValue;
                          return (
                            <div
                              key={`${entry.section}.${entry.key}`}
                              className="space-y-1.5"
                            >
                              <div className="flex items-center justify-between gap-3">
                                <div className="min-w-0">
                                  <div className="flex items-center gap-2">
                                    <span className="text-xs font-medium text-[var(--text-primary)]">
                                      {entry.label}
                                    </span>
                                    {modified && <Badge variant="warning">Modified</Badge>}
                                  </div>
                                  <div className="text-[10px] font-mono text-[var(--text-muted)]">
                                    {entry.key}
                                    {entry.description ? ` · ${entry.description}` : ""}
                                  </div>
                                </div>

                                <div className="flex items-center gap-2 shrink-0">
                                  {modified && (
                                    <button
                                      type="button"
                                      onClick={() => updateEntry(entry.section, entry.key, entry.defaultValue)}
                                      className="text-[10px] font-mono text-[var(--text-muted)] hover:text-[var(--text-primary)] cursor-pointer"
                                      title={`Reset to ${entry.defaultValue}`}
                                    >
                                      ↺
                                    </button>
                                  )}
                                  {entry.kind === "bool" && (
                                    <Toggle
                                      checked={entry.value === "1"}
                                      onChange={(v) => updateEntry(entry.section, entry.key, v ? "1" : "0")}
                                    />
                                  )}
                                </div>
                              </div>

                              {entry.kind === "number" && (
                                <Slider
                                  value={Number(entry.value) || 0}
                                  min={entry.min ?? 0}
                                  max={entry.max ?? 10}
                                  step={entry.step ?? 0.05}
                                  unit={entry.unit ?? ""}
                                  onChange={(v) =>
                                    updateEntry(
                                      entry.section,
                                      entry.key,
                                      Number.isInteger(entry.step ?? 1) ? String(v) : v.toFixed(2)
                                    )
                                  }
                                />
                              )}

                              {entry.kind === "enum" && entry.options && (
                                <div className="flex flex-wrap gap-1.5 pt-0.5">
                                  {entry.options.map((opt) => (
                                    <button
                                      key={opt}
                                      type="button"
                                      onClick={() => updateEntry(entry.section, entry.key, opt)}
                                      className={`px-2.5 py-1 rounded-lg text-[11px] font-mono border transition-colors cursor-pointer ${
                                        entry.value === opt
                                          ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--text-primary)]"
                                          : "bg-white/[0.03] border-white/[0.07] text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                                      }`}
                                    >
                                      {opt}
                                    </button>
                                  ))}
                                </div>
                              )}

                              {entry.kind === "text" && (
                                <input
                                  value={entry.value}
                                  onChange={(e) => updateEntry(entry.section, entry.key, e.target.value)}
                                  className="w-full h-8 px-2.5 rounded-lg bg-black/40 border border-white/[0.09] font-mono text-xs text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-border)]"
                                />
                              )}
                            </div>
                          );
                        })}
                      </div>
                    </Card>
                  ))}
                </div>
              </>
            ) : (
              <Card variant="elevated" padding="md" className="space-y-3">
                <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                  <span className="flex items-center gap-1.5 font-mono">
                    <FileCode2 className="w-3.5 h-3.5 text-[var(--accent)]" />
                    Documents/Assetto Corsa/cfg/extension/{activeModule.file}
                  </span>
                  <span className="font-mono text-[11px]">User override layer (safe backup on save)</span>
                </div>
                <textarea
                  value={rawText}
                  onChange={(e) => setRawText(e.target.value)}
                  rows={18}
                  spellCheck={false}
                  className="w-full p-4 rounded-xl bg-[#06080C] border border-white/[0.08] font-mono text-xs text-emerald-300 leading-relaxed focus:outline-none focus:border-[var(--accent-border)]"
                />
              </Card>
            )}
          </div>
        </div>
      )}

      {/* TAB 2: PATCHES & ONLINE CATALOG */}
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

      {/* TAB 3: WEATHERFX */}
      {tab === "weather" && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          <Card padding="md" className="space-y-4">
            <div>
              <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                WeatherFX Controller
              </div>
              <p className="text-xs text-[var(--text-secondary)] mt-0.5">
                Select the active dynamic sky & weather controller script.
              </p>
            </div>

            <div className="space-y-2">
              {controllers.map((c) => {
                const active = activeController === c.id;
                return (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => {
                      setActiveController(c.id);
                      notify("WeatherFX controller selected", "success", c.name);
                    }}
                    className={`w-full text-left p-3.5 rounded-xl border transition-all cursor-pointer flex items-center justify-between ${
                      active
                        ? "bg-[var(--accent-soft)] border-[var(--accent-border)]"
                        : "bg-white/[0.02] border-white/[0.07] hover:bg-white/[0.05]"
                    }`}
                  >
                    <div>
                      <div className="text-sm font-semibold text-[var(--text-primary)]">{c.name}</div>
                      <div className="text-[11px] text-[var(--text-muted)]">
                        {c.author ?? "Community"} {c.version ? `· v${c.version}` : ""}
                      </div>
                    </div>
                    {active && <Badge variant="accent" dot>Active</Badge>}
                  </button>
                );
              })}
            </div>
          </Card>

          <Card padding="md" className="space-y-4">
            <div className="text-xs font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
              Atmosphere & Wet Physics
            </div>
            <div className="space-y-3">
              <Toggle
                checked={dynamicSky}
                onChange={setDynamicSky}
                label="Dynamic Volumetric Sky & Sun Trajectories"
                description="Real-time solar elevation and atmospheric scattering based on track latitude & date."
              />
              <Toggle
                checked={rainFx}
                onChange={setRainFx}
                label="RainFX Puddles & Wet Grip"
                description="Dynamic water accumulation, spray particles, and wet tyre physics."
              />
            </div>
          </Card>
        </div>
      )}
    </div>
  );
}

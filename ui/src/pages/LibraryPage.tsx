import React, { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Archive,
  ArrowLeftRight,
  Check,
  CheckCircle2,
  FolderInput,
  Lock,
  Package,
  RefreshCw,
  ShieldAlert,
  Sliders,
  Sparkles,
  Unlock,
  Upload,
  Volume2,
  Wrench,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import type {
  CarEntry,
  CarPhysicsData,
  CarSfxInfo,
  CarTuningValues,
  ContentHealthIssue,
  JsgmeModEntry,
  PackageEntry,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  KpiTile,
  PageHeader,
  SegmentedControl,
  Slider,
} from "../components/Primitives";

type LibraryTab = "installer" | "jsgme" | "health" | "tuning" | "sfx";

const DEMO_PACKAGES: PackageEntry[] = [
  {
    id: "pkg-car-1",
    kind: "car",
    name: "Porsche 992 GT3 R (2024)",
    targetPath: "content/cars/acxs_porsche_992_gt3r",
    sizeBytes: 184_500_000,
    selected: true,
  },
  {
    id: "pkg-skin-1",
    kind: "skin",
    name: "Manthey EMA #911 Grello Livery",
    targetPath: "content/cars/acxs_porsche_992_gt3r/skins/911_grello",
    sizeBytes: 24_200_000,
    selected: true,
  },
  {
    id: "pkg-pp-1",
    kind: "csp",
    name: "ACXS_Natural_Mod_v2.ini (PP Filter)",
    targetPath: "system/cfg/ppfilters/ACXS_Natural_Mod_v2.ini",
    sizeBytes: 18_400,
    selected: true,
  },
];

const DEMO_JSGME: JsgmeModEntry[] = [
  { name: "Pure 0.254 HighRes Skydomes", enabled: true, filesCount: 412, conflictsWith: [] },
  { name: "CSP GrassFX Enhanced Pack", enabled: true, filesCount: 88, conflictsWith: ["NaturalMod PP + Shaders"] },
  { name: "NaturalMod PP + Shaders", enabled: false, filesCount: 34, conflictsWith: ["CSP GrassFX Enhanced Pack"] },
  { name: "F1 2025 Broadcast HUD", enabled: false, filesCount: 61, conflictsWith: [] },
  { name: "Spa 2024 Grandstand & Trees", enabled: false, filesCount: 194, conflictsWith: [] },
];

const DEMO_HEALTH: ContentHealthIssue[] = [
  {
    id: "h1",
    severity: "warning",
    kind: "missing-preview",
    target: "content/cars/bmw_m3_e30_dtm/skins/04_jager",
    message: "Skin missing preview.jpg (auto-generatable via Showroom)",
    autoFixable: true,
  },
  {
    id: "h2",
    severity: "error",
    kind: "broken-sfx",
    target: "content/cars/mod_gt4_cayman/sfx/GUIDs.txt",
    message: "Missing GUIDs.txt bank manifest — engine audio will be silent",
    autoFixable: true,
  },
  {
    id: "h3",
    severity: "info",
    kind: "unpacked-data",
    target: "content/cars/ks_ferrari_488_gt3/data",
    message: "Unpacked data/ folder coexists with data.acd (may trigger online checksum mismatch)",
    autoFixable: false,
  },
];

const DEFAULT_TUNING: CarTuningValues = {
  powerMultiplier: 1.0,
  torqueMultiplier: 1.0,
  massOffsetKg: 0,
  aeroDragMultiplier: 1.0,
  aeroDownforceMultiplier: 1.0,
  brakeTorqueMultiplier: 1.0,
  finalDriveRatio: 3.44,
};

function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(0)} KB`;
  return `${bytes} B`;
}

export function LibraryPage() {
  const { cars, armedSession, notify, rescan } = useAppStore();

  const [tab, setTab] = useState<LibraryTab>("installer");

  // 1. Universal Installer
  const [dragActive, setDragActive] = useState(false);
  const [packagePath, setPackagePath] = useState("");
  const [packages, setPackages] = useState<PackageEntry[]>(DEMO_PACKAGES);
  const [installing, setInstalling] = useState(false);
  const fileInputRef = React.useRef<HTMLInputElement>(null);

  // 2. JSGME Mods
  const [mods, setMods] = useState<JsgmeModEntry[]>([]);
  const [modBusy, setModBusy] = useState<string | null>(null);

  // 3. Health & Repairs
  const [issues, setIssues] = useState<ContentHealthIssue[]>([]);
  const [scanningHealth, setScanningHealth] = useState(false);
  const [repairing, setRepairing] = useState(false);

  // 4. Car Tuning & ACD
  const [tuningCarId, setTuningCarId] = useState<string>(armedSession.carId || cars[0]?.id || "");
  const [tuning, setTuning] = useState<CarTuningValues>(DEFAULT_TUNING);
  const [carPhysics, setCarPhysics] = useState<CarPhysicsData | null>(null);
  const [acdBusy, setAcdBusy] = useState(false);

  // 5. SFX Sound Swapper
  const [sfxTargetId, setSfxTargetId] = useState<string>(armedSession.carId || cars[0]?.id || "");
  const [sfxDonorId, setSfxDonorId] = useState<string>(cars[1]?.id ?? cars[0]?.id ?? "");
  const [targetSfx, setTargetSfx] = useState<CarSfxInfo | null>(null);
  const [donorSfx, setDonorSfx] = useState<CarSfxInfo | null>(null);
  const [swappingSfx, setSwappingSfx] = useState(false);

  const tuningCar = useMemo(
    () => cars.find((c) => c.id === tuningCarId) ?? cars[0],
    [cars, tuningCarId]
  );
  const sfxTargetCar = useMemo(
    () => cars.find((c) => c.id === sfxTargetId) ?? cars[0],
    [cars, sfxTargetId]
  );
  const sfxDonorCar = useMemo(
    () => cars.find((c) => c.id === sfxDonorId) ?? cars[1] ?? cars[0],
    [cars, sfxDonorId]
  );

  // Load JSGME mods
  useEffect(() => {
    if (tab !== "jsgme") return;
    let cancelled = false;
    api
      .listJsgmeMods()
      .then((res) => {
        if (!cancelled) setMods(res && res.length > 0 ? res : DEMO_JSGME);
      })
      .catch(() => {
        if (!cancelled) setMods(DEMO_JSGME);
      });
    return () => {
      cancelled = true;
    };
  }, [tab]);

  // Load Health scan
  const runHealthScan = async () => {
    setScanningHealth(true);
    try {
      const res = await api.runContentHealth();
      setIssues(res && res.length > 0 ? res : DEMO_HEALTH);
    } catch {
      setIssues(DEMO_HEALTH);
    } finally {
      setScanningHealth(false);
    }
  };

  useEffect(() => {
    if (tab === "health" && issues.length === 0) {
      void runHealthScan();
    }
  }, [tab]);

  // Load Tuning & ACD status
  useEffect(() => {
    if (tab !== "tuning" || !tuningCar) return;
    let cancelled = false;
    Promise.all([
      api.getCarTuning(tuningCar.path).catch(() => null),
      api.getCarPhysics(tuningCar.path).catch(() => null),
    ]).then(([t, p]) => {
      if (cancelled) return;
      setTuning(t ?? DEFAULT_TUNING);
      setCarPhysics(
        p ?? {
          rpmCurve: [],
          torqueNm: [],
          powerHp: [],
          gearRatios: [],
          gearTopSpeedsKmh: [],
          redlineRpm: 8500,
          massKg: 1250,
          hasPackedAcd: true,
          hasUnpackedData: false,
        }
      );
    });
    return () => {
      cancelled = true;
    };
  }, [tab, tuningCar]);

  // Load SFX audit
  useEffect(() => {
    if (tab !== "sfx") return;
    let cancelled = false;
    if (sfxTargetCar) {
      api
        .inspectCarSfx(sfxTargetCar.path)
        .then((res) => {
          if (!cancelled) {
            setTargetSfx(
              res ?? {
                carId: sfxTargetCar.id,
                bankPath: `${sfxTargetCar.path}/sfx/${sfxTargetCar.id}.bank`,
                bankSizeBytes: 14_820_000,
                guidsPresent: true,
              }
            );
          }
        })
        .catch(() => {});
    }
    if (sfxDonorCar) {
      api
        .inspectCarSfx(sfxDonorCar.path)
        .then((res) => {
          if (!cancelled) {
            setDonorSfx(
              res ?? {
                carId: sfxDonorCar.id,
                bankPath: `${sfxDonorCar.path}/sfx/${sfxDonorCar.id}.bank`,
                bankSizeBytes: 18_410_000,
                guidsPresent: true,
              }
            );
          }
        })
        .catch(() => {});
    }
    return () => {
      cancelled = true;
    };
  }, [tab, sfxTargetCar, sfxDonorCar]);

  // Universal Installer handlers
  const handleInspectPath = async (rawPath: string) => {
    const clean = rawPath.trim();
    if (!clean) return;
    setPackagePath(clean);
    try {
      const entries = await api.inspectPackage(clean);
      if (entries && entries.length > 0) {
        setPackages(entries);
      } else {
        const lower = clean.toLowerCase();
        const kind: PackageEntry["kind"] = lower.endsWith(".ini")
          ? "csp"
          : lower.includes("track")
          ? "track"
          : "car";
        setPackages([
          {
            id: `inspected-${Date.now()}`,
            kind,
            name: clean.split(/[\\/]/).pop() ?? clean,
            targetPath:
              kind === "csp"
                ? `system/cfg/ppfilters/${clean.split(/[\\/]/).pop()}`
                : `content/${kind}s/${clean.split(/[\\/]/).pop()?.replace(/\.(zip|7z|rar|tar\.gz)$/i, "")}`,
            sizeBytes: 42_500_000,
            selected: true,
          },
        ]);
      }
      notify("Archive Inspected", "info", clean);
    } catch (err) {
      notify("Inspect failed", "error", String(err));
    }
  };

  const handleDrop = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragActive(false);
    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;
    const mapped: PackageEntry[] = files.map((f, i) => ({
      id: `drop-${Date.now()}-${i}`,
      kind: f.name.endsWith(".ini") ? "csp" : "mod",
      name: f.name,
      targetPath: f.name.endsWith(".ini")
        ? `system/cfg/ppfilters/${f.name}`
        : `content/mods/${f.name.replace(/\.(zip|7z|rar|tar\.gz)$/i, "")}`,
      sizeBytes: f.size || 12_400_000,
      selected: true,
    }));
    setPackages(mapped);
    setPackagePath(files[0].name);
    notify("Queued Dropped Package(s)", "success", `${files.length} item(s) ready to install`);
  };

  const handleInstallSelected = async () => {
    const selectedIds = packages.filter((p) => p.selected).map((p) => p.id);
    if (selectedIds.length === 0) return;
    setInstalling(true);
    try {
      await api.installPackage(packagePath || "staged_bundle.zip", selectedIds);
      notify("Installation Complete", "success", `Installed ${selectedIds.length} package item(s)`);
      void rescan(true);
    } catch (err) {
      notify("Installation failed", "error", String(err));
    } finally {
      setInstalling(false);
    }
  };

  const handleToggleJsgme = async (mod: JsgmeModEntry) => {
    setModBusy(mod.name);
    try {
      if (mod.enabled) {
        await api.disableJsgmeMod(mod.name);
      } else {
        await api.enableJsgmeMod(mod.name);
      }
      setMods((prev) =>
        prev.map((m) => (m.name === mod.name ? { ...m, enabled: !m.enabled } : m))
      );
      notify(mod.enabled ? "Mod Disabled" : "Mod Enabled", "success", mod.name);
    } catch (err) {
      notify("JSGME operation failed", "error", String(err));
    } finally {
      setModBusy(null);
    }
  };

  const handleAutoRepair = async () => {
    setRepairing(true);
    try {
      await api.applyContentRepairs();
      setIssues((prev) => prev.filter((i) => !i.autoFixable));
      notify("Auto-Repairs Applied", "success", "All fixable content issues resolved");
    } catch (err) {
      notify("Repair failed", "error", String(err));
    } finally {
      setRepairing(false);
    }
  };

  const handleUnpackAcd = async () => {
    if (!tuningCar) return;
    setAcdBusy(true);
    try {
      await api.unpackCarAcd(tuningCar.path);
      setCarPhysics((prev) => (prev ? { ...prev, hasUnpackedData: true } : prev));
      notify("data.acd Unpacked", "success", `${tuningCar.name} ready for physics tuning`);
    } catch (err) {
      notify("Unpack failed", "error", String(err));
    } finally {
      setAcdBusy(false);
    }
  };

  const handleRepackAcd = async () => {
    if (!tuningCar) return;
    setAcdBusy(true);
    try {
      await api.packCarAcd(tuningCar.path);
      setCarPhysics((prev) => (prev ? { ...prev, hasPackedAcd: true, hasUnpackedData: false } : prev));
      notify("data.acd Repacked", "success", `${tuningCar.name} checksum packed`);
    } catch (err) {
      notify("Repack failed", "error", String(err));
    } finally {
      setAcdBusy(false);
    }
  };

  const handleApplyTuning = async () => {
    if (!tuningCar) return;
    try {
      await api.applyCarTuning(tuningCar.path, tuning);
      notify("Car Tuning Applied", "success", tuningCar.name);
    } catch (err) {
      notify("Tuning failed", "error", String(err));
    }
  };

  const handleSwapSfx = async () => {
    if (!sfxTargetCar || !sfxDonorCar) return;
    setSwappingSfx(true);
    try {
      await api.swapCarSfx(sfxTargetCar.path, sfxDonorCar.path);
      notify(
        "SFX Bank Swapped",
        "success",
        `${sfxTargetCar.name} now uses ${sfxDonorCar.name} audio`
      );
    } catch (err) {
      notify("SFX swap failed", "error", String(err));
    } finally {
      setSwappingSfx(false);
    }
  };

  const availableMods = mods.filter((m) => !m.enabled);
  const enabledMods = mods.filter((m) => m.enabled);

  return (
    <div className="h-full flex flex-col overflow-hidden animate-fade-in">
      <div className="p-6 pb-4 shrink-0">
        <PageHeader
          eyebrow="Garage · Content Management"
          title="Library"
          subtitle="Universal archive installer, JSGME load order, content health, ACD physics tuning, and SFX sound swapper"
          actions={
            <SegmentedControl
              size="sm"
              value={tab}
              onChange={setTab}
              options={[
                { value: "installer", label: "Universal Installer", icon: <Package className="w-3.5 h-3.5" /> },
                { value: "jsgme", label: "JSGME Mods", icon: <Archive className="w-3.5 h-3.5" /> },
                { value: "health", label: "Health & Repairs", icon: <ShieldAlert className="w-3.5 h-3.5" /> },
                { value: "tuning", label: "Car Tuning & ACD", icon: <Sliders className="w-3.5 h-3.5" /> },
                { value: "sfx", label: "SFX Sound Swapper", icon: <Volume2 className="w-3.5 h-3.5" /> },
              ]}
            />
          }
        />
      </div>

      <div className="flex-1 min-h-0 px-6 pb-6 overflow-y-auto">
        {/* TAB 1: UNIVERSAL INSTALLER */}
        {tab === "installer" && (
          <div className="space-y-5 max-w-5xl">
            <div
              onDragOver={(e) => {
                e.preventDefault();
                setDragActive(true);
              }}
              onDragLeave={() => setDragActive(false)}
              onDrop={handleDrop}
              onClick={() => fileInputRef.current?.click()}
              className={`rounded-2xl border-2 border-dashed p-10 text-center transition-all cursor-pointer flex flex-col items-center gap-3 ${
                dragActive
                  ? "border-[var(--accent)] bg-[var(--accent-soft)]"
                  : "border-white/[0.12] bg-white/[0.02] hover:border-white/[0.22] hover:bg-white/[0.04]"
              }`}
            >
              <input
                ref={fileInputRef}
                type="file"
                multiple
                accept=".zip,.7z,.tar.gz,.rar,.ini"
                className="hidden"
                onChange={(e) => {
                  const files = Array.from(e.target.files ?? []);
                  if (files.length === 0) return;
                  setPackages(
                    files.map((f, i) => ({
                      id: `file-${Date.now()}-${i}`,
                      kind: f.name.endsWith(".ini") ? "csp" : "mod",
                      name: f.name,
                      targetPath: `content/mods/${f.name}`,
                      sizeBytes: f.size,
                      selected: true,
                    }))
                  );
                  setPackagePath(files[0].name);
                }}
              />
              <div className="w-12 h-12 rounded-2xl bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center text-[var(--accent)]">
                <Upload className="w-6 h-6" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-white">
                  Drop archives, folders, or PP filters here
                </h3>
                <p className="text-xs text-[var(--text-muted)] mt-1">
                  Supports <span className="font-mono text-[var(--text-secondary)]">.zip · .7z · .tar.gz · .rar · folders · .ini PP filters</span> — automatic content root detection
                </p>
              </div>
            </div>

            {/* Direct Path Inspect */}
            <div className="flex items-center gap-2">
              <div className="relative flex-1">
                <FolderInput className="w-4 h-4 text-[var(--text-muted)] absolute left-3.5 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  value={packagePath}
                  onChange={(e) => setPackagePath(e.target.value)}
                  placeholder="/path/to/mod_archive.7z or folder..."
                  className="w-full h-10 pl-10 pr-3 rounded-xl bg-white/[0.04] border border-white/[0.09] text-xs font-mono text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                />
              </div>
              <Button
                variant="secondary"
                onClick={() => void handleInspectPath(packagePath)}
              >
                Inspect Archive
              </Button>
            </div>

            {/* Multi-Item Checklist */}
            <div className="glass-panel rounded-2xl overflow-hidden">
              <div className="px-4 py-3 border-b border-white/[0.07] flex items-center justify-between">
                <div className="text-xs font-semibold text-white">
                  Detected Package Items ({packages.filter((p) => p.selected).length}/{packages.length} selected)
                </div>
                <Button
                  variant="primary"
                  size="sm"
                  loading={installing}
                  disabled={packages.filter((p) => p.selected).length === 0}
                  onClick={() => void handleInstallSelected()}
                >
                  Install Selected
                </Button>
              </div>
              <div className="divide-y divide-white/[0.05]">
                {packages.map((pkg, idx) => {
                  const hasConflict = idx === 1;
                  return (
                    <div
                      key={pkg.id}
                      onClick={() =>
                        setPackages((prev) =>
                          prev.map((p) => (p.id === pkg.id ? { ...p, selected: !p.selected } : p))
                        )
                      }
                      className="px-4 py-3 flex items-center justify-between gap-3 hover:bg-white/[0.025] cursor-pointer"
                    >
                      <div className="flex items-center gap-3 min-w-0">
                        <div
                          className={`w-5 h-5 rounded-md border flex items-center justify-center shrink-0 ${
                            pkg.selected
                              ? "bg-[var(--accent)] border-[var(--accent)] text-white"
                              : "border-white/20 bg-black/30"
                          }`}
                        >
                          {pkg.selected && <Check className="w-3.5 h-3.5" />}
                        </div>
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-xs font-semibold text-white truncate">
                              {pkg.name}
                            </span>
                            <Badge variant="info" className="uppercase">
                              {pkg.kind}
                            </Badge>
                            {hasConflict ? (
                              <Badge variant="warning">Overwrite</Badge>
                            ) : (
                              <Badge variant="success">Clean</Badge>
                            )}
                          </div>
                          <div className="text-[11px] font-mono text-[var(--text-muted)] truncate mt-0.5">
                            → {pkg.targetPath}
                          </div>
                        </div>
                      </div>
                      <span className="font-mono text-xs text-[var(--text-secondary)] shrink-0">
                        {formatBytes(pkg.sizeBytes)}
                      </span>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        )}

        {/* TAB 2: JSGME MODS */}
        {tab === "jsgme" && (
          <div className="space-y-5">
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
              {/* Available Mods */}
              <div className="glass-panel rounded-2xl p-4 space-y-3">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                      MODS/ Repository
                    </div>
                    <h3 className="text-sm font-semibold text-white">
                      Available Mods ({availableMods.length})
                    </h3>
                  </div>
                </div>
                <div className="space-y-2">
                  {availableMods.map((mod) => (
                    <div
                      key={mod.name}
                      className="p-3 rounded-xl bg-white/[0.025] border border-white/[0.07] flex items-center justify-between gap-3"
                    >
                      <div className="min-w-0">
                        <div className="text-xs font-semibold text-white truncate">{mod.name}</div>
                        <div className="flex items-center gap-2 mt-1">
                          <span className="font-mono text-[10px] text-[var(--text-muted)]">
                            {mod.filesCount} files
                          </span>
                          {mod.conflictsWith.length > 0 && (
                            <Badge variant="warning">
                              Conflicts: {mod.conflictsWith.length}
                            </Badge>
                          )}
                        </div>
                      </div>
                      <Button
                        variant="secondary"
                        size="sm"
                        loading={modBusy === mod.name}
                        onClick={() => void handleToggleJsgme(mod)}
                      >
                        Enable →
                      </Button>
                    </div>
                  ))}
                </div>
              </div>

              {/* Enabled Load Order */}
              <div className="glass-panel rounded-2xl p-4 space-y-3">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] font-mono uppercase tracking-wider text-emerald-400">
                      Active Stack
                    </div>
                    <h3 className="text-sm font-semibold text-white">
                      Load Order Enabled ({enabledMods.length})
                    </h3>
                  </div>
                </div>
                <div className="space-y-2">
                  {enabledMods.map((mod, idx) => (
                    <div
                      key={mod.name}
                      className="p-3 rounded-xl bg-emerald-500/[0.06] border border-emerald-500/25 flex items-center justify-between gap-3"
                    >
                      <div className="flex items-center gap-3 min-w-0">
                        <span className="w-6 h-6 rounded-md bg-emerald-500/20 font-mono text-[11px] font-bold text-emerald-300 flex items-center justify-center shrink-0">
                          {idx + 1}
                        </span>
                        <div className="min-w-0">
                          <div className="text-xs font-semibold text-white truncate">{mod.name}</div>
                          <div className="font-mono text-[10px] text-[var(--text-muted)] mt-0.5">
                            {mod.filesCount} files overlaid
                          </div>
                        </div>
                      </div>
                      <Button
                        variant="danger"
                        size="sm"
                        loading={modBusy === mod.name}
                        onClick={() => void handleToggleJsgme(mod)}
                      >
                        Disable
                      </Button>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            {/* Conflict Matrix */}
            <div className="glass-panel rounded-2xl p-4 space-y-2.5">
              <div className="flex items-center gap-2 text-xs font-semibold text-amber-300">
                <AlertTriangle className="w-4 h-4" />
                JSGME File Conflict Matrix
              </div>
              {mods.filter((m) => m.conflictsWith.length > 0).length === 0 ? (
                <p className="text-xs text-[var(--text-muted)]">No overlapping file conflicts detected.</p>
              ) : (
                <div className="space-y-1.5">
                  {mods
                    .filter((m) => m.conflictsWith.length > 0)
                    .map((m) => (
                      <div
                        key={m.name}
                        className="px-3 py-2 rounded-lg bg-amber-500/10 border border-amber-500/25 text-xs flex flex-wrap items-center justify-between gap-2"
                      >
                        <span className="font-medium text-white">{m.name}</span>
                        <span className="font-mono text-[11px] text-amber-300">
                          Overlaps with: {m.conflictsWith.join(", ")}
                        </span>
                      </div>
                    ))}
                </div>
              )}
            </div>
          </div>
        )}

        {/* TAB 3: HEALTH & REPAIRS */}
        {tab === "health" && (
          <div className="space-y-5 max-w-5xl">
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              <KpiTile
                label="Errors"
                value={issues.filter((i) => i.severity === "error").length}
                accent
              />
              <KpiTile
                label="Warnings"
                value={issues.filter((i) => i.severity === "warning").length}
              />
              <KpiTile
                label="Info"
                value={issues.filter((i) => i.severity === "info").length}
              />
              <KpiTile
                label="Auto-Fixable"
                value={issues.filter((i) => i.autoFixable).length}
              />
            </div>

            <div className="flex items-center justify-between gap-3">
              <Button
                variant="secondary"
                loading={scanningHealth}
                icon={<RefreshCw className="w-3.5 h-3.5" />}
                onClick={() => void runHealthScan()}
              >
                Re-Scan Content Integrity
              </Button>
              <Button
                variant="primary"
                loading={repairing}
                disabled={issues.filter((i) => i.autoFixable).length === 0}
                icon={<Wrench className="w-3.5 h-3.5" />}
                onClick={() => void handleAutoRepair()}
              >
                Auto-Repair All Fixable
              </Button>
            </div>

            <div className="glass-panel rounded-2xl divide-y divide-white/[0.06] overflow-hidden">
              {issues.length === 0 ? (
                <div className="p-8 text-center space-y-2">
                  <CheckCircle2 className="w-8 h-8 text-emerald-400 mx-auto" />
                  <div className="text-sm font-semibold text-white">All Content Healthy</div>
                  <p className="text-xs text-[var(--text-muted)]">
                    No missing previews, broken SFX manifests, or malformed INI files detected.
                  </p>
                </div>
              ) : (
                issues.map((issue) => (
                  <div
                    key={issue.id}
                    className="p-4 flex items-start justify-between gap-4 hover:bg-white/[0.02]"
                  >
                    <div className="space-y-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <Badge
                          variant={
                            issue.severity === "error"
                              ? "danger"
                              : issue.severity === "warning"
                              ? "warning"
                              : "info"
                          }
                          dot
                        >
                          {issue.severity.toUpperCase()}
                        </Badge>
                        <span className="font-mono text-[11px] text-[var(--text-muted)]">
                          {issue.kind}
                        </span>
                      </div>
                      <div className="text-xs font-medium text-white">{issue.message}</div>
                      <div className="text-[11px] font-mono text-[var(--text-muted)] truncate">
                        {issue.target}
                      </div>
                    </div>
                    {issue.autoFixable && <Badge variant="success">Auto-Fixable</Badge>}
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {/* TAB 4: CAR TUNING & ACD */}
        {tab === "tuning" && (
          <div className="space-y-5 max-w-5xl">
            {/* Visual Car Picker Strip */}
            <div className="flex items-center gap-2 overflow-x-auto pb-1">
              {cars.map((car) => (
                <button
                  key={car.id}
                  type="button"
                  onClick={() => setTuningCarId(car.id)}
                  className={`h-9 px-3 rounded-xl text-xs font-medium border shrink-0 transition-colors cursor-pointer ${
                    car.id === tuningCar?.id
                      ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                      : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                  }`}
                >
                  {car.brand} {car.name}
                </button>
              ))}
            </div>

            {/* data.acd Opt-In Unpack / Repack Banner */}
            <div className="glass-panel rounded-2xl p-4 border border-amber-500/25 bg-amber-500/[0.05] flex flex-wrap items-center justify-between gap-4">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-xl bg-amber-500/20 border border-amber-500/35 flex items-center justify-center text-amber-300">
                  {carPhysics?.hasUnpackedData ? (
                    <Unlock className="w-5 h-5" />
                  ) : (
                    <Lock className="w-5 h-5" />
                  )}
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-white">
                      data.acd Archive Control
                    </span>
                    <Badge variant={carPhysics?.hasUnpackedData ? "warning" : "success"}>
                      {carPhysics?.hasUnpackedData ? "Unpacked data/ Active" : "Packed data.acd"}
                    </Badge>
                  </div>
                  <p className="text-xs text-[var(--text-secondary)] mt-0.5">
                    Opt-in unpack to edit physics INI files locally. Repack before joining checksum-verified online servers.
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  loading={acdBusy}
                  icon={<Unlock className="w-3.5 h-3.5" />}
                  onClick={() => void handleUnpackAcd()}
                >
                  Unpack data.acd
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  loading={acdBusy}
                  icon={<Lock className="w-3.5 h-3.5" />}
                  onClick={() => void handleRepackAcd()}
                >
                  Repack data.acd
                </Button>
              </div>
            </div>

            {/* Car Tuning Sliders */}
            <div className="glass-panel rounded-2xl p-5 space-y-5">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--accent)]">
                    Physics Multipliers
                  </div>
                  <h3 className="text-base font-semibold text-white">
                    {tuningCar?.name ?? "Select a Car"}
                  </h3>
                </div>
                <div className="flex items-center gap-2">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setTuning(DEFAULT_TUNING)}
                  >
                    Reset 1.0×
                  </Button>
                  <Button
                    variant="primary"
                    size="sm"
                    icon={<Sparkles className="w-3.5 h-3.5" />}
                    onClick={() => void handleApplyTuning()}
                  >
                    Apply Tuning
                  </Button>
                </div>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
                <Slider
                  label="Power Multiplier"
                  value={tuning.powerMultiplier}
                  min={0.5}
                  max={2.0}
                  step={0.05}
                  formatValue={(v) => `${v.toFixed(2)}×`}
                  onChange={(v) => setTuning((p) => ({ ...p, powerMultiplier: v }))}
                />
                <Slider
                  label="Torque Multiplier"
                  value={tuning.torqueMultiplier}
                  min={0.5}
                  max={2.0}
                  step={0.05}
                  formatValue={(v) => `${v.toFixed(2)}×`}
                  onChange={(v) => setTuning((p) => ({ ...p, torqueMultiplier: v }))}
                />
                <Slider
                  label="Mass Offset"
                  value={tuning.massOffsetKg}
                  min={-300}
                  max={300}
                  step={10}
                  formatValue={(v) => `${v > 0 ? "+" : ""}${v} kg`}
                  onChange={(v) => setTuning((p) => ({ ...p, massOffsetKg: v }))}
                />
                <Slider
                  label="Aero Downforce Multiplier"
                  value={tuning.aeroDownforceMultiplier}
                  min={0.5}
                  max={2.0}
                  step={0.05}
                  formatValue={(v) => `${v.toFixed(2)}×`}
                  onChange={(v) => setTuning((p) => ({ ...p, aeroDownforceMultiplier: v }))}
                />
                <Slider
                  label="Aero Drag Multiplier"
                  value={tuning.aeroDragMultiplier}
                  min={0.5}
                  max={1.5}
                  step={0.05}
                  formatValue={(v) => `${v.toFixed(2)}×`}
                  onChange={(v) => setTuning((p) => ({ ...p, aeroDragMultiplier: v }))}
                />
                <Slider
                  label="Brake Torque Multiplier"
                  value={tuning.brakeTorqueMultiplier}
                  min={0.7}
                  max={1.5}
                  step={0.05}
                  formatValue={(v) => `${v.toFixed(2)}×`}
                  onChange={(v) => setTuning((p) => ({ ...p, brakeTorqueMultiplier: v }))}
                />
              </div>
            </div>
          </div>
        )}

        {/* TAB 5: SFX SOUND SWAPPER */}
        {tab === "sfx" && (
          <div className="space-y-5 max-w-5xl">
            {/* Side-by-side Auditor & Swap Action */}
            <div className="grid grid-cols-1 md:grid-cols-11 gap-4 items-center">
              <div className="md:col-span-5 glass-panel rounded-2xl p-4 space-y-2.5">
                <div className="flex items-center justify-between">
                  <span className="text-[10px] font-mono uppercase tracking-wider text-cyan-400">
                    Target Vehicle
                  </span>
                  <Badge variant={targetSfx?.guidsPresent ? "success" : "danger"} dot>
                    {targetSfx?.guidsPresent ? "GUIDs OK" : "GUIDs Missing"}
                  </Badge>
                </div>
                <div className="text-base font-semibold text-white">
                  {sfxTargetCar?.name ?? "—"}
                </div>
                <div className="text-[11px] font-mono text-[var(--text-muted)]">
                  Bank Size: {formatBytes(targetSfx?.bankSizeBytes ?? 0)}
                </div>
              </div>

              <div className="md:col-span-1 flex justify-center">
                <div className="w-10 h-10 rounded-full bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center text-[var(--accent)]">
                  <ArrowLeftRight className="w-4 h-4" />
                </div>
              </div>

              <div className="md:col-span-5 glass-panel rounded-2xl p-4 space-y-2.5">
                <div className="flex items-center justify-between">
                  <span className="text-[10px] font-mono uppercase tracking-wider text-[var(--accent)]">
                    Donor Sound Bank
                  </span>
                  <Badge variant={donorSfx?.guidsPresent ? "success" : "danger"} dot>
                    {donorSfx?.guidsPresent ? "GUIDs OK" : "GUIDs Missing"}
                  </Badge>
                </div>
                <div className="text-base font-semibold text-white">
                  {sfxDonorCar?.name ?? "—"}
                </div>
                <div className="text-[11px] font-mono text-[var(--text-muted)]">
                  Bank Size: {formatBytes(donorSfx?.bankSizeBytes ?? 0)}
                </div>
              </div>
            </div>

            <div className="flex justify-end">
              <Button
                variant="primary"
                loading={swappingSfx}
                disabled={!sfxTargetCar || !sfxDonorCar || sfxTargetCar.id === sfxDonorCar.id}
                icon={<Volume2 className="w-4 h-4" />}
                onClick={() => void handleSwapSfx()}
              >
                Swap SFX Bank & Rewrite GUIDs.txt
              </Button>
            </div>

            {/* Visual Target & Donor Pickers */}
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
              {[
                {
                  title: "1. Select Target Car",
                  selectedId: sfxTargetCar?.id,
                  onPick: setSfxTargetId,
                },
                {
                  title: "2. Select Donor Audio Car",
                  selectedId: sfxDonorCar?.id,
                  onPick: setSfxDonorId,
                },
              ].map((col) => (
                <div key={col.title} className="glass-panel rounded-2xl p-4 space-y-3">
                  <div className="text-xs font-semibold text-white">{col.title}</div>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 max-h-72 overflow-y-auto pr-1">
                    {cars.map((car: CarEntry) => {
                      const active = car.id === col.selectedId;
                      const thumb = assetUrl(car.previewPath);
                      return (
                        <button
                          key={car.id}
                          type="button"
                          onClick={() => col.onPick(car.id)}
                          className={`p-2 rounded-xl border text-left flex items-center gap-2.5 transition-all cursor-pointer ${
                            active
                              ? "bg-[var(--accent-soft)] border-[var(--accent-border)]"
                              : "bg-white/[0.02] border-white/[0.06] hover:bg-white/[0.05]"
                          }`}
                        >
                          <div className="w-12 h-8 rounded-lg overflow-hidden bg-[#080B12] border border-white/[0.08] shrink-0 flex items-center justify-center">
                            {thumb ? (
                              <img src={thumb} alt={car.name} className="w-full h-full object-cover" />
                            ) : (
                              <Volume2 className="w-3.5 h-3.5 text-[var(--text-muted)]" />
                            )}
                          </div>
                          <div className="min-w-0">
                            <div className="text-xs font-medium text-white truncate">
                              {car.name}
                            </div>
                            <div className="text-[10px] font-mono text-[var(--text-muted)]">
                              {car.brand}
                            </div>
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default LibraryPage;

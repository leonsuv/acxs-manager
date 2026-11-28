import { useEffect, useMemo, useRef, useState } from "react";
import {
  CheckCircle2,
  FileCode2,
  GitCompare,
  Plus,
  RotateCcw,
  Save,
  Search,
  Sliders,
  Timer,
  Trophy,
  Wrench,
} from "lucide-react";
import { api, assetUrl, isTauri } from "../lib/api";
import type {
  CarEntry,
  SetupDiffEntry,
  SetupDocument,
  SetupFileSummary,
  SetupParameter,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  EmptyState,
  KpiTile,
  PageHeader,
  SegmentedControl,
  Slider,
} from "../components/Primitives";

type SetupModeTab = "editor" | "diff" | "stint";

function buildDefaultParameters(variant: "base" | "quali" | "endurance" = "base"): SetupParameter[] {
  const isQuali = variant === "quali";
  const isEndu = variant === "endurance";
  return [
    { key: "PRESSURE_LF", section: "Tyres", label: "Front Left Pressure", value: isQuali ? 26.4 : 26.8, min: 20, max: 35, step: 0.1, unit: " psi" },
    { key: "PRESSURE_RF", section: "Tyres", label: "Front Right Pressure", value: isQuali ? 26.2 : 26.6, min: 20, max: 35, step: 0.1, unit: " psi" },
    { key: "PRESSURE_LR", section: "Tyres", label: "Rear Left Pressure", value: isQuali ? 26.0 : 26.5, min: 20, max: 35, step: 0.1, unit: " psi" },
    { key: "PRESSURE_RR", section: "Tyres", label: "Rear Right Pressure", value: isQuali ? 25.9 : 26.4, min: 20, max: 35, step: 0.1, unit: " psi" },
    { key: "WING_1", section: "Aero", label: "Front Splitter / Diveplane", value: isQuali ? 4 : isEndu ? 3 : 3, min: 0, max: 10, step: 1, unit: " deg" },
    { key: "WING_2", section: "Aero", label: "Rear Wing Angle", value: isQuali ? 7 : isEndu ? 5 : 6, min: 0, max: 14, step: 1, unit: " deg" },
    { key: "CAMBER_LF", section: "Alignment", label: "Front Camber", value: isQuali ? -3.4 : -3.1, min: -5.0, max: -0.5, step: 0.1, unit: " deg" },
    { key: "CAMBER_LR", section: "Alignment", label: "Rear Camber", value: isQuali ? -2.4 : -2.2, min: -4.0, max: -0.5, step: 0.1, unit: " deg" },
    { key: "TOE_OUT_LF", section: "Alignment", label: "Front Toe", value: -0.08, min: -0.3, max: 0.3, step: 0.01, unit: " deg" },
    { key: "TOE_OUT_LR", section: "Alignment", label: "Rear Toe", value: 0.14, min: -0.2, max: 0.4, step: 0.01, unit: " deg" },
    { key: "SPRING_RATE_LF", section: "Suspension", label: "Front Spring Rate", value: isQuali ? 155 : 140, min: 80, max: 240, step: 5, unit: " N/mm" },
    { key: "SPRING_RATE_LR", section: "Suspension", label: "Rear Spring Rate", value: isQuali ? 145 : 130, min: 80, max: 240, step: 5, unit: " N/mm" },
    { key: "ROD_LENGTH_LF", section: "Suspension", label: "Front Ride Height", value: isQuali ? 54 : 58, min: 45, max: 85, step: 1, unit: " mm" },
    { key: "ROD_LENGTH_LR", section: "Suspension", label: "Rear Ride Height", value: isQuali ? 62 : 66, min: 50, max: 95, step: 1, unit: " mm" },
    { key: "ARB_FRONT", section: "Suspension", label: "Front Anti-Roll Bar", value: isQuali ? 5 : 4, min: 0, max: 8, step: 1, unit: "" },
    { key: "ARB_REAR", section: "Suspension", label: "Rear Anti-Roll Bar", value: isQuali ? 4 : 3, min: 0, max: 8, step: 1, unit: "" },
    { key: "DIFF_POWER", section: "Drivetrain", label: "Diff Power Lock", value: isQuali ? 45 : 35, min: 10, max: 90, step: 5, unit: "%" },
    { key: "DIFF_COAST", section: "Drivetrain", label: "Diff Coast Lock", value: isQuali ? 55 : 50, min: 10, max: 90, step: 5, unit: "%" },
    { key: "BRAKE_POWER_MULT", section: "Brakes & Fuel", label: "Brake Power", value: 98, min: 80, max: 100, step: 1, unit: "%" },
    { key: "FRONT_BIAS", section: "Brakes & Fuel", label: "Brake Bias (Front)", value: isQuali ? 61.5 : 62.5, min: 52, max: 72, step: 0.5, unit: "%" },
    { key: "FUEL", section: "Brakes & Fuel", label: "Fuel Load", value: isQuali ? 18 : isEndu ? 95 : 45, min: 5, max: 120, step: 1, unit: " L" },
  ];
}

function buildDemoSetups(cars: CarEntry[]): SetupFileSummary[] {
  const firstCar = cars[0]?.id ?? "ks_porsche_911_gt3_r_2016";
  const secondCar = cars[1]?.id ?? "ks_ferrari_488_gt3";
  return [
    { name: "spa_quali_low_fuel.ini", carId: firstCar, trackId: "spa", path: `/setups/${firstCar}/spa/spa_quali_low_fuel.ini`, modifiedAt: "Today 19:42" },
    { name: "spa_race_stint_v3.ini", carId: firstCar, trackId: "spa", path: `/setups/${firstCar}/spa/spa_race_stint_v3.ini`, modifiedAt: "Yesterday" },
    { name: "monza_low_drag.ini", carId: firstCar, trackId: "monza", path: `/setups/${firstCar}/monza/monza_low_drag.ini`, modifiedAt: "3d ago" },
    { name: "nordschleife_24h_safe.ini", carId: firstCar, trackId: "ks_nordschleife", path: `/setups/${firstCar}/ks_nordschleife/nordschleife_24h_safe.ini`, modifiedAt: "5d ago" },
    { name: "spa_endurance_stable.ini", carId: secondCar, trackId: "spa", path: `/setups/${secondCar}/spa/spa_endurance_stable.ini`, modifiedAt: "2d ago" },
    { name: "imola_kerb_attack.ini", carId: secondCar, trackId: "imola", path: `/setups/${secondCar}/imola/imola_kerb_attack.ini`, modifiedAt: "6d ago" },
  ];
}

interface StintRecord {
  label: string;
  setupName: string;
  laps: number[];
  sector1: number;
  sector2: number;
  sector3: number;
  tyreTempC: number;
  tyreWearPct: number;
}

function formatLapSec(sec: number): string {
  const mins = Math.floor(sec / 60);
  const rem = sec - mins * 60;
  return `${mins}:${rem.toFixed(3).padStart(6, "0")}`;
}

export function SetupsPage() {
  const { cars, tracks, armedSession, notify } = useAppStore();

  const [carSearch, setCarSearch] = useState("");
  const [selectedCarId, setSelectedCarId] = useState<string>(
    armedSession.carId || cars[0]?.id || ""
  );
  const [selectedTrackFilter, setSelectedTrackFilter] = useState<string>("all");
  const [setups, setSetups] = useState<SetupFileSummary[]>([]);
  const [selectedSetupPath, setSelectedSetupPath] = useState<string>("");
  const [modeTab, setModeTab] = useState<SetupModeTab>("editor");

  // Schema editor state
  const [doc, setDoc] = useState<SetupDocument | null>(null);
  const [values, setValues] = useState<Record<string, number>>({});
  const [initialValues, setInitialValues] = useState<Record<string, number>>({});
  const [activeSection, setActiveSection] = useState<string>("all");
  const [saving, setSaving] = useState(false);
  const localSetupDocsRef = useRef<Record<string, SetupDocument>>({});

  // Diff & 3-Way Merge state
  const [diffLeftPath, setDiffLeftPath] = useState<string>("");
  const [diffRightPath, setDiffRightPath] = useState<string>("");
  const [diffEntries, setDiffEntries] = useState<SetupDiffEntry[]>([]);
  const [mergeChoice, setMergeChoice] = useState<Record<string, "left" | "right" | "avg">>({});

  // Stint A/B state
  const [stintA, setStintA] = useState<StintRecord>({
    label: "Stint A · Baseline",
    setupName: "spa_race_stint_v3.ini",
    laps: [138.412, 137.985, 138.104, 137.892, 138.015],
    sector1: 41.24,
    sector2: 59.81,
    sector3: 36.84,
    tyreTempC: 86,
    tyreWearPct: 91,
  });
  const [stintB, setStintB] = useState<StintRecord>({
    label: "Stint B · Candidate",
    setupName: "spa_quali_low_fuel.ini",
    laps: [137.742, 137.418, 137.509, 137.381, 137.622],
    sector1: 41.05,
    sector2: 59.48,
    sector3: 36.85,
    tyreTempC: 89,
    tyreWearPct: 87,
  });

  // Load setups list
  useEffect(() => {
    let cancelled = false;
    api
      .listSetups()
      .then((list) => {
        if (cancelled) return;
        const resolved = list && list.length > 0 ? list : buildDemoSetups(cars);
        setSetups(resolved);
      })
      .catch(() => {
        if (!cancelled) setSetups(buildDemoSetups(cars));
      });
    return () => {
      cancelled = true;
    };
  }, [cars]);

  useEffect(() => {
    if (!selectedCarId && cars.length > 0) {
      setSelectedCarId(cars[0].id);
    }
  }, [cars, selectedCarId]);

  const filteredCars = useMemo(() => {
    const q = carSearch.trim().toLowerCase();
    if (!q) return cars;
    return cars.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        c.brand.toLowerCase().includes(q) ||
        c.class.toLowerCase().includes(q)
    );
  }, [cars, carSearch]);

  const selectedCar = useMemo(
    () => cars.find((c) => c.id === selectedCarId) ?? cars[0],
    [cars, selectedCarId]
  );

  const carSetups = useMemo(() => {
    return setups.filter((s) => s.carId === selectedCar?.id);
  }, [setups, selectedCar]);

  const visibleSetups = useMemo(() => {
    if (selectedTrackFilter === "all") return carSetups;
    return carSetups.filter((s) => s.trackId === selectedTrackFilter);
  }, [carSetups, selectedTrackFilter]);

  const setupTracks = useMemo(() => {
    const ids = new Set(carSetups.map((s) => s.trackId));
    return tracks.filter((t) => ids.has(t.id));
  }, [carSetups, tracks]);

  // Auto-select first setup when car/track changes
  useEffect(() => {
    if (visibleSetups.length === 0) {
      setSelectedSetupPath("");
      return;
    }
    if (!visibleSetups.some((s) => s.path === selectedSetupPath)) {
      setSelectedSetupPath(visibleSetups[0].path);
    }
    if (!diffLeftPath || !carSetups.some((s) => s.path === diffLeftPath)) {
      setDiffLeftPath(carSetups[0]?.path ?? "");
    }
    if (!diffRightPath || !carSetups.some((s) => s.path === diffRightPath)) {
      setDiffRightPath(carSetups[1]?.path ?? carSetups[0]?.path ?? "");
    }
  }, [visibleSetups, carSetups, selectedSetupPath, diffLeftPath, diffRightPath]);

  const activeSetup = useMemo(
    () => visibleSetups.find((s) => s.path === selectedSetupPath) ?? visibleSetups[0],
    [visibleSetups, selectedSetupPath]
  );

  // Load setup document
  useEffect(() => {
    if (!activeSetup || !selectedCar) {
      setDoc(null);
      return;
    }
    let cancelled = false;
    const variant = activeSetup.name.includes("quali")
      ? "quali"
      : activeSetup.name.includes("endurance") || activeSetup.name.includes("24h")
      ? "endurance"
      : "base";

    const cachedLocal = localSetupDocsRef.current[activeSetup.path];
    if (!isTauri() && cachedLocal) {
      setDoc(cachedLocal);
      const map: Record<string, number> = {};
      cachedLocal.parameters.forEach((p) => {
        map[p.key] = p.value;
      });
      setValues(map);
      setInitialValues(map);
      return;
    }

    api
      .readSetup(activeSetup.path, selectedCar.path)
      .then((loaded) => {
        if (cancelled) return;
        const resolved: SetupDocument =
          loaded && loaded.parameters?.length
            ? loaded
            : cachedLocal ?? {
                path: activeSetup.path,
                carId: activeSetup.carId,
                trackId: activeSetup.trackId,
                parameters: buildDefaultParameters(variant),
              };
        setDoc(resolved);
        const map: Record<string, number> = {};
        resolved.parameters.forEach((p) => {
          map[p.key] = p.value;
        });
        setValues(map);
        setInitialValues(map);
        const secs = Array.from(new Set(resolved.parameters.map((p) => p.section)));
        setActiveSection((prev) =>
          prev !== "all" && secs.length > 0 && !secs.includes(prev) ? "all" : prev
        );
      })
      .catch(() => {
        if (cancelled) return;
        const fallbackParams = buildDefaultParameters(variant);
        const resolved: SetupDocument = cachedLocal ?? {
          path: activeSetup.path,
          carId: activeSetup.carId,
          trackId: activeSetup.trackId,
          parameters: fallbackParams,
        };
        setDoc(resolved);
        const map: Record<string, number> = {};
        resolved.parameters.forEach((p) => (map[p.key] = p.value));
        setValues(map);
        setInitialValues(map);
      });
    return () => {
      cancelled = true;
    };
  }, [activeSetup, selectedCar]);

  // Compute diff when left/right changes
  useEffect(() => {
    if (!diffLeftPath || !diffRightPath) {
      setDiffEntries([]);
      return;
    }
    let cancelled = false;
    api
      .diffSetups(diffLeftPath, diffRightPath)
      .then((res) => {
        if (cancelled) return;
        if (res && res.length > 0) {
          setDiffEntries(res);
          return;
        }
        const leftParams = buildDefaultParameters(
          diffLeftPath.includes("quali") ? "quali" : "base"
        );
        const rightParams = buildDefaultParameters(
          diffRightPath.includes("endurance") || diffRightPath.includes("24h")
            ? "endurance"
            : diffRightPath.includes("quali")
            ? "quali"
            : "base"
        );
        const computed: SetupDiffEntry[] = [];
        leftParams.forEach((lp, idx) => {
          const rp = rightParams[idx];
          if (rp && lp.value !== rp.value) {
            computed.push({
              key: lp.key,
              label: lp.label,
              leftValue: lp.value,
              rightValue: rp.value,
              delta: Number((rp.value - lp.value).toFixed(2)),
            });
          }
        });
        setDiffEntries(computed);
      })
      .catch(() => {
        if (!cancelled) setDiffEntries([]);
      });
    return () => {
      cancelled = true;
    };
  }, [diffLeftPath, diffRightPath]);

  const sections = useMemo(() => {
    if (!doc) return [];
    return Array.from(new Set(doc.parameters.map((p) => p.section)));
  }, [doc]);

  const modifiedBySection = useMemo(() => {
    const counts: Record<string, number> = {};
    if (!doc) return counts;
    for (const p of doc.parameters) {
      if (values[p.key] !== initialValues[p.key]) {
        counts[p.section] = (counts[p.section] ?? 0) + 1;
      }
    }
    return counts;
  }, [doc, values, initialValues]);

  const dirtyCount = useMemo(() => {
    return Object.keys(values).filter((k) => values[k] !== initialValues[k]).length;
  }, [values, initialValues]);

  const resolveSetupPath = (carId: string, trackId: string, name: string): string => {
    const sameDirExample = setups.find(
      (s) =>
        s.carId === carId &&
        s.trackId === trackId &&
        s.path &&
        !s.path.startsWith("/setups/")
    );
    if (sameDirExample) {
      const sepIdx = Math.max(
        sameDirExample.path.lastIndexOf("/"),
        sameDirExample.path.lastIndexOf("\\")
      );
      if (sepIdx >= 0) {
        return `${sameDirExample.path.slice(0, sepIdx + 1)}${name}`;
      }
    }
    const realExample = setups.find(
      (s) => s.path && !s.path.startsWith("/setups/")
    );
    if (realExample) {
      const m = realExample.path.match(/^(.*?[/\\]setups)[/\\]/i);
      if (m) {
        const sep = realExample.path.includes("\\") ? "\\" : "/";
        return `${m[1]}${sep}${carId}${sep}${trackId}${sep}${name}`;
      }
    }
    return `/setups/${carId}/${trackId}/${name}`;
  };

  const handleSaveSetup = async () => {
    if (!activeSetup) return;
    setSaving(true);
    try {
      await api.saveSetup(activeSetup.path, values);
      if (doc) {
        localSetupDocsRef.current[activeSetup.path] = {
          ...doc,
          parameters: doc.parameters.map((p) => ({
            ...p,
            value: values[p.key] ?? p.value,
          })),
        };
      }
      setInitialValues({ ...values });
      notify("Setup Saved", "success", activeSetup.name);
    } catch (err) {
      notify("Failed to save setup", "error", String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleCreateSetup = async () => {
    if (!selectedCar) return;
    const trackId =
      selectedTrackFilter === "all" ? tracks[0]?.id ?? "spa" : selectedTrackFilter;
    const safeTrackId = trackId.replace(/[/\\:]/g, "_");
    const name = `${safeTrackId}_custom_${setups.length + 1}.ini`;
    const targetPath = resolveSetupPath(selectedCar.id, safeTrackId, name);
    const variant =
      trackId.includes("endurance") || trackId.includes("24h")
        ? "endurance"
        : "base";
    const baseParams = doc?.parameters?.length
      ? doc.parameters.map((p) => ({ ...p, value: values[p.key] ?? p.value }))
      : buildDefaultParameters(variant);
    const paramValues: Record<string, number> = {};
    baseParams.forEach((p) => {
      paramValues[p.key] = p.value;
    });

    let finalPath = targetPath;
    try {
      await api.saveSetup(targetPath, paramValues);
      if (isTauri() && targetPath.startsWith("/setups/")) {
        const refreshed = await api.listSetups().catch(() => []);
        const stem = name.replace(/\.ini$/i, "").toLowerCase();
        const created = refreshed.find(
          (s) =>
            s.carId === selectedCar.id &&
            s.name.replace(/\.ini$/i, "").toLowerCase() === stem
        );
        if (created?.path) {
          finalPath = created.path;
        }
      }
    } catch (err) {
      if (isTauri()) {
        notify("Failed to create setup", "error", String(err));
        return;
      }
    }

    const newDoc: SetupDocument = {
      path: finalPath,
      carId: selectedCar.id,
      trackId: safeTrackId,
      parameters: baseParams,
    };
    const newItem: SetupFileSummary = {
      name,
      carId: selectedCar.id,
      trackId: safeTrackId,
      path: finalPath,
      modifiedAt: "Just now",
    };

    localSetupDocsRef.current[finalPath] = newDoc;
    setSetups((prev) => [newItem, ...prev.filter((s) => s.path !== finalPath)]);
    setSelectedSetupPath(finalPath);
    setDoc(newDoc);
    setValues(paramValues);
    setInitialValues(paramValues);
    notify("Created new setup", "success", name);
  };

  const handleSaveMerged = async () => {
    if (!selectedCar) return;
    const name = `merged_${Date.now().toString().slice(-4)}.ini`;
    const leftSetup = carSetups.find((s) => s.path === diffLeftPath);
    const rawTrackId =
      leftSetup?.trackId ??
      (selectedTrackFilter !== "all" ? selectedTrackFilter : tracks[0]?.id ?? "spa");
    const trackId = rawTrackId.replace(/[/\\:]/g, "_");
    const targetPath = resolveSetupPath(selectedCar.id, trackId, name);

    let baseParams: SetupParameter[] | null = null;
    if (diffLeftPath) {
      if (!isTauri() && localSetupDocsRef.current[diffLeftPath]) {
        baseParams = localSetupDocsRef.current[diffLeftPath].parameters;
      } else {
        try {
          const loaded = await api.readSetup(diffLeftPath, selectedCar.path);
          if (loaded?.parameters?.length) baseParams = loaded.parameters;
        } catch {
          // fallback below
        }
      }
    }
    if (!baseParams || baseParams.length === 0) {
      baseParams = buildDefaultParameters(
        diffLeftPath.includes("quali") ? "quali" : "base"
      );
    }

    const resolveTargetParamKey = (diffKey: string): string => {
      const upper = diffKey.toUpperCase();
      const exact = baseParams!.find((p) => p.key.toUpperCase() === upper);
      if (exact) return exact.key;
      const dot = diffKey.indexOf(".");
      if (dot > 0) {
        const stemUpper = diffKey.slice(0, dot).toUpperCase();
        const byStem = baseParams!.find(
          (p) => !p.key.includes(".") && p.key.toUpperCase() === stemUpper
        );
        if (byStem) return byStem.key;
      } else {
        const withValueUpper = `${upper}.VALUE`;
        const byValue = baseParams!.find(
          (p) => p.key.toUpperCase() === withValueUpper
        );
        if (byValue) return byValue.key;
      }
      return diffKey;
    };

    const mergedValues: Record<string, number> = {};
    baseParams.forEach((p) => {
      mergedValues[p.key] = p.value;
    });

    const diffByKey = new Map<string, number>();
    for (const row of diffEntries) {
      const pick = mergeChoice[row.key] ?? "right";
      const leftNum = Number(row.leftValue);
      const rightNum = Number(row.rightValue);
      let resolvedVal: number | null = null;
      if (pick === "left" && Number.isFinite(leftNum)) {
        resolvedVal = leftNum;
      } else if (pick === "right" && Number.isFinite(rightNum)) {
        resolvedVal = rightNum;
      } else if (
        pick === "avg" &&
        Number.isFinite(leftNum) &&
        Number.isFinite(rightNum)
      ) {
        resolvedVal = Number(((leftNum + rightNum) / 2).toFixed(2));
      } else if (Number.isFinite(rightNum)) {
        resolvedVal = rightNum;
      } else if (Number.isFinite(leftNum)) {
        resolvedVal = leftNum;
      }
      if (resolvedVal !== null) {
        const normalizedKey = resolveTargetParamKey(row.key);
        mergedValues[normalizedKey] = resolvedVal;
        diffByKey.set(normalizedKey, resolvedVal);
      }
    }

    const mergedParams = baseParams.map((p) => ({
      ...p,
      value: diffByKey.get(p.key) ?? p.value,
    }));

    let finalPath = targetPath;
    try {
      await api.saveSetup(targetPath, mergedValues);
      if (isTauri() && targetPath.startsWith("/setups/")) {
        const refreshed = await api.listSetups().catch(() => []);
        const stem = name.replace(/\.ini$/i, "").toLowerCase();
        const created = refreshed.find(
          (s) =>
            s.carId === selectedCar.id &&
            s.name.replace(/\.ini$/i, "").toLowerCase() === stem
        );
        if (created?.path) {
          finalPath = created.path;
        }
      }
    } catch (err) {
      if (isTauri()) {
        notify("Failed to save merged setup", "error", String(err));
        return;
      }
    }

    const mergedDoc: SetupDocument = {
      path: finalPath,
      carId: selectedCar.id,
      trackId,
      parameters: mergedParams,
    };
    localSetupDocsRef.current[finalPath] = mergedDoc;
    const mergedItem: SetupFileSummary = {
      name,
      carId: selectedCar.id,
      trackId,
      path: finalPath,
      modifiedAt: "Just now",
    };
    setSetups((prev) => [mergedItem, ...prev.filter((s) => s.path !== finalPath)]);
    setSelectedSetupPath(finalPath);
    setDoc(mergedDoc);
    setValues(mergedValues);
    setInitialValues(mergedValues);
    setModeTab("editor");
    notify(
      "3-Way Merge Saved",
      "success",
      `Created ${name} with ${diffEntries.length} resolved parameters`
    );
  };

  const stintStats = (s: StintRecord) => {
    const best = Math.min(...s.laps);
    const avg = s.laps.reduce((a, b) => a + b, 0) / s.laps.length;
    const variance =
      s.laps.reduce((acc, v) => acc + Math.pow(v - avg, 2), 0) / s.laps.length;
    return { best, avg, sigma: Math.sqrt(variance) };
  };

  const statsA = stintStats(stintA);
  const statsB = stintStats(stintB);
  const deltaBest = statsB.best - statsA.best;

  return (
    <div className="h-full flex flex-col overflow-hidden animate-fade-in">
      <div className="p-6 pb-4 shrink-0">
        <PageHeader
          eyebrow="Garage · Race Engineering"
          title="Setups"
          subtitle="Car-aware schema editor, 3-way setup merge, and Stint A/B telemetry scorecard"
          actions={
            <SegmentedControl
              value={modeTab}
              onChange={setModeTab}
              options={[
                { value: "editor", label: "Schema Editor", icon: <Sliders className="w-3.5 h-3.5" /> },
                { value: "diff", label: "Diff & 3-Way Merge", icon: <GitCompare className="w-3.5 h-3.5" /> },
                { value: "stint", label: "Stint A/B Testing", icon: <Timer className="w-3.5 h-3.5" /> },
              ]}
            />
          }
        />
      </div>

      {/* 2-Column Visual Workspace */}
      <div className="flex-1 min-h-0 px-6 pb-6 grid grid-cols-12 gap-5 overflow-hidden">
        {/* LEFT COLUMN: Visual Car Card List + Horizontal Track Pills + Setup List */}
        <div className="col-span-12 lg:col-span-4 flex flex-col gap-3 min-h-0 overflow-hidden">
          {/* Searchable Visual Car Card List */}
          <div className="glass-panel rounded-2xl p-3 flex flex-col gap-2.5 max-h-[44%] min-h-[190px]">
            <div className="relative">
              <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
              <input
                type="text"
                value={carSearch}
                onChange={(e) => setCarSearch(e.target.value)}
                placeholder="Filter cars visually..."
                className="w-full h-8 pl-8 pr-3 rounded-lg bg-black/35 border border-white/[0.08] text-xs text-white placeholder:text-[var(--text-muted)] focus:border-[var(--accent-border)] outline-none"
              />
            </div>

            <div className="flex-1 overflow-y-auto space-y-1.5 pr-0.5">
              {filteredCars.map((car) => {
                const active = car.id === selectedCar?.id;
                const count = setups.filter((s) => s.carId === car.id).length;
                const thumb = assetUrl(car.previewPath);
                return (
                  <button
                    key={car.id}
                    type="button"
                    onClick={() => {
                      setSelectedCarId(car.id);
                      setSelectedTrackFilter("all");
                    }}
                    className={`w-full p-2 rounded-xl border text-left flex items-center gap-2.5 transition-all cursor-pointer ${
                      active
                        ? "bg-[var(--accent-soft)] border-[var(--accent-border)]"
                        : "bg-white/[0.02] border-white/[0.06] hover:bg-white/[0.05] hover:border-white/[0.12]"
                    }`}
                  >
                    <div className="w-14 h-9 rounded-lg overflow-hidden bg-[#080B12] border border-white/[0.08] shrink-0 flex items-center justify-center">
                      {thumb ? (
                        <img src={thumb} alt={car.name} className="w-full h-full object-cover" />
                      ) : (
                        <span className="font-mono text-[9px] uppercase text-[var(--accent)] font-bold">
                          {car.brand.slice(0, 3)}
                        </span>
                      )}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="text-xs font-semibold text-white truncate">{car.name}</div>
                      <div className="text-[10px] font-mono text-[var(--text-muted)] truncate">
                        {car.brand} · {car.class.toUpperCase()}
                      </div>
                    </div>
                    <Badge variant={count > 0 ? "accent" : "neutral"}>{count}</Badge>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Horizontal Track Pill Filter */}
          <div className="flex items-center gap-1.5 overflow-x-auto pb-1 shrink-0">
            <button
              type="button"
              onClick={() => setSelectedTrackFilter("all")}
              className={`h-7 px-2.5 rounded-lg text-[11px] font-mono border shrink-0 transition-colors cursor-pointer ${
                selectedTrackFilter === "all"
                  ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                  : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
              }`}
            >
              All Tracks ({carSetups.length})
            </button>
            {setupTracks.map((t) => (
              <button
                key={t.id}
                type="button"
                onClick={() => setSelectedTrackFilter(t.id)}
                className={`h-7 px-2.5 rounded-lg text-[11px] font-mono border shrink-0 transition-colors cursor-pointer ${
                  selectedTrackFilter === t.id
                    ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                    : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                }`}
              >
                {t.name.split(" ").slice(0, 2).join(" ")}
              </button>
            ))}
          </div>

          {/* Setup List for Selected Car & Track */}
          <div className="flex-1 glass-panel rounded-2xl p-3 flex flex-col gap-2 min-h-0">
            <div className="flex items-center justify-between px-1">
              <span className="text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                Saved Setups
              </span>
              <Button
                variant="secondary"
                size="sm"
                icon={<Plus className="w-3.5 h-3.5" />}
                onClick={handleCreateSetup}
              >
                New
              </Button>
            </div>

            <div className="flex-1 overflow-y-auto space-y-1.5">
              {visibleSetups.length === 0 ? (
                <div className="h-full flex flex-col items-center justify-center text-center p-4 gap-2">
                  <Wrench className="w-5 h-5 text-[var(--text-muted)]" />
                  <p className="text-xs text-[var(--text-muted)]">
                    No setups for this car yet. Click New to initialize one.
                  </p>
                </div>
              ) : (
                visibleSetups.map((s) => {
                  const active = s.path === activeSetup?.path;
                  return (
                    <button
                      key={s.path}
                      type="button"
                      onClick={() => setSelectedSetupPath(s.path)}
                      className={`w-full p-2.5 rounded-xl border text-left transition-all cursor-pointer flex items-center justify-between gap-2 ${
                        active
                          ? "bg-white/[0.07] border-[var(--accent-border)]"
                          : "bg-white/[0.02] border-white/[0.06] hover:bg-white/[0.04]"
                      }`}
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5">
                          <FileCode2 className="w-3.5 h-3.5 text-[var(--accent)] shrink-0" />
                          <span className="text-xs font-mono font-medium text-white truncate">
                            {s.name}
                          </span>
                        </div>
                        <div className="text-[10px] font-mono text-[var(--text-muted)] mt-0.5">
                          {s.trackId} · {s.modifiedAt ?? "Saved"}
                        </div>
                      </div>
                      {active && (
                        <span className="w-2 h-2 rounded-full bg-[var(--accent)] shrink-0" />
                      )}
                    </button>
                  );
                })
              )}
            </div>
          </div>
        </div>

        {/* RIGHT COLUMN: Active Engineering Workspace */}
        <div className="col-span-12 lg:col-span-8 glass-panel rounded-2xl p-5 flex flex-col min-h-0 overflow-hidden">
          {modeTab === "editor" && (
            <>
              {!doc || !activeSetup ? (
                <EmptyState
                  icon={<Sliders className="w-5 h-5" />}
                  title="Select or create a setup"
                  description="Pick a car and setup from the left column to tune tyres, aero, suspension, and drivetrain."
                  className="my-auto"
                />
              ) : (
                <>
                  <div className="flex flex-wrap items-center justify-between gap-3 pb-3.5 border-b border-white/[0.07] shrink-0">
                    <div>
                      <div className="flex items-center gap-2">
                        <h2 className="font-mono text-sm font-semibold text-white">
                          {activeSetup.name}
                        </h2>
                        {dirtyCount > 0 && (
                          <Badge variant="warning" dot>
                            {dirtyCount} modified
                          </Badge>
                        )}
                      </div>
                      <p className="text-[11px] text-[var(--text-muted)] mt-0.5">
                        {selectedCar?.name} · Track: {activeSetup.trackId}
                      </p>
                    </div>

                    <div className="flex items-center gap-2">
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={dirtyCount === 0}
                        icon={<RotateCcw className="w-3.5 h-3.5" />}
                        onClick={() => setValues({ ...initialValues })}
                      >
                        Reset
                      </Button>
                      <Button
                        variant="primary"
                        size="sm"
                        loading={saving}
                        icon={<Save className="w-3.5 h-3.5" />}
                        onClick={() => void handleSaveSetup()}
                      >
                        Save Setup
                      </Button>
                    </div>
                  </div>

                  {/* Section Filter Pills ("All" stacked view + quick filters with per-section modified badges) */}
                  <div className="flex flex-wrap items-center gap-1.5 py-3 shrink-0">
                    <button
                      type="button"
                      onClick={() => setActiveSection("all")}
                      className={`h-8 px-3 rounded-lg text-xs font-medium border transition-colors cursor-pointer flex items-center gap-1.5 ${
                        activeSection === "all"
                          ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                          : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                      }`}
                    >
                      <span>All Sections</span>
                      {dirtyCount > 0 && (
                        <span className="px-1.5 py-0.2 rounded-full bg-amber-500/20 text-amber-300 font-mono text-[10px]">
                          {dirtyCount}
                        </span>
                      )}
                    </button>
                    {sections.map((sec) => {
                      const secDirty = modifiedBySection[sec] ?? 0;
                      return (
                        <button
                          key={sec}
                          type="button"
                          onClick={() => setActiveSection(sec)}
                          className={`h-8 px-3 rounded-lg text-xs font-medium border transition-colors cursor-pointer flex items-center gap-1.5 ${
                            activeSection === sec
                              ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                              : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                          }`}
                        >
                          <span>{sec}</span>
                          {secDirty > 0 && (
                            <span className="px-1.5 py-0.2 rounded-full bg-amber-500/20 text-amber-300 font-mono text-[10px]">
                              {secDirty}
                            </span>
                          )}
                        </button>
                      );
                    })}
                  </div>

                  {/* Tactile Parameter Sliders Grid (Stacked Sections when "all" is active) */}
                  <div className="flex-1 overflow-y-auto pr-1 space-y-5">
                    {(activeSection === "all" ? sections : [activeSection]).map((sec) => {
                      const secParams = doc.parameters.filter((p) => p.section === sec);
                      if (secParams.length === 0) return null;
                      const secDirty = modifiedBySection[sec] ?? 0;
                      return (
                        <div key={sec} className="space-y-2.5">
                          <div className="flex items-center justify-between px-1 pb-1 border-b border-white/[0.06]">
                            <div className="flex items-center gap-2">
                              <span className="font-mono text-xs font-semibold text-[var(--accent)]">
                                [{sec}]
                              </span>
                              <span className="text-[11px] font-mono text-[var(--text-muted)]">
                                {secParams.length} parameter{secParams.length === 1 ? "" : "s"}
                              </span>
                            </div>
                            {secDirty > 0 && (
                              <Badge variant="warning" dot>
                                {secDirty} modified
                              </Badge>
                            )}
                          </div>
                          <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
                            {secParams.map((param) => {
                              const currentVal = values[param.key] ?? param.value;
                              const changed = currentVal !== initialValues[param.key];
                              return (
                                <div
                                  key={param.key}
                                  className={`p-3.5 rounded-xl border transition-colors ${
                                    changed
                                      ? "bg-[var(--accent-soft)]/30 border-[var(--accent-border)]"
                                      : "bg-white/[0.02] border-white/[0.06]"
                                  }`}
                                >
                                  <Slider
                                    label={param.label}
                                    value={currentVal}
                                    min={param.min}
                                    max={param.max}
                                    step={param.step}
                                    unit={param.unit ?? ""}
                                    onChange={(v) =>
                                      setValues((prev) => ({ ...prev, [param.key]: v }))
                                    }
                                  />
                                  <div className="flex items-center justify-between text-[10px] font-mono text-[var(--text-muted)] mt-1">
                                    <span>{param.key}</span>
                                    <span>
                                      Range: {param.min} – {param.max}
                                    </span>
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </>
              )}
            </>
          )}

          {modeTab === "diff" && (
            <div className="flex flex-col h-full gap-4 overflow-hidden">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 shrink-0">
                <div className="p-3 rounded-xl bg-white/[0.025] border border-white/[0.07] space-y-2">
                  <div className="text-[10px] font-mono uppercase tracking-wider text-cyan-400">
                    Setup A (Base / Left)
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {carSetups.map((s) => (
                      <button
                        key={s.path}
                        type="button"
                        onClick={() => setDiffLeftPath(s.path)}
                        className={`px-2.5 py-1 rounded-lg text-[11px] font-mono border cursor-pointer transition-colors ${
                          diffLeftPath === s.path
                            ? "bg-cyan-500/20 text-cyan-300 border-cyan-500/40"
                            : "bg-black/30 text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                        }`}
                      >
                        {s.name}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="p-3 rounded-xl bg-white/[0.025] border border-white/[0.07] space-y-2">
                  <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--accent)]">
                    Setup B (Candidate / Right)
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {carSetups.map((s) => (
                      <button
                        key={s.path}
                        type="button"
                        onClick={() => setDiffRightPath(s.path)}
                        className={`px-2.5 py-1 rounded-lg text-[11px] font-mono border cursor-pointer transition-colors ${
                          diffRightPath === s.path
                            ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                            : "bg-black/30 text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                        }`}
                      >
                        {s.name}
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              <div className="flex-1 overflow-y-auto rounded-xl border border-white/[0.07] bg-black/25">
                {diffEntries.length === 0 ? (
                  <div className="h-full flex items-center justify-center text-xs text-[var(--text-muted)] p-6">
                    Select two different setups above to inspect parameter deltas and cherry-pick a 3-way merge.
                  </div>
                ) : (
                  <table className="w-full text-left border-collapse">
                    <thead className="sticky top-0 bg-[#0D1118] border-b border-white/[0.08] text-[10px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                      <tr>
                        <th className="py-2.5 px-3.5">Parameter</th>
                        <th className="py-2.5 px-3">Setup A</th>
                        <th className="py-2.5 px-3">Setup B</th>
                        <th className="py-2.5 px-3">Delta</th>
                        <th className="py-2.5 px-3.5 text-right">3-Way Pick</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-white/[0.05] text-xs font-mono">
                      {diffEntries.map((row) => {
                        const pick = mergeChoice[row.key] ?? "right";
                        return (
                          <tr key={row.key} className="hover:bg-white/[0.02]">
                            <td className="py-2.5 px-3.5">
                              <div className="font-sans font-medium text-white">{row.label}</div>
                              <div className="text-[10px] text-[var(--text-muted)]">{row.key}</div>
                            </td>
                            <td className="py-2.5 px-3 text-cyan-300">{row.leftValue}</td>
                            <td className="py-2.5 px-3 text-[var(--accent)]">{row.rightValue}</td>
                            <td className="py-2.5 px-3">
                              {row.delta !== undefined && (
                                <Badge variant={row.delta > 0 ? "success" : "warning"}>
                                  {row.delta > 0 ? `+${row.delta}` : row.delta}
                                </Badge>
                              )}
                            </td>
                            <td className="py-2.5 px-3.5 text-right">
                              <div className="inline-flex rounded-lg bg-black/45 p-0.5 border border-white/[0.08]">
                                {(["left", "avg", "right"] as const).map((opt) => (
                                  <button
                                    key={opt}
                                    type="button"
                                    onClick={() =>
                                      setMergeChoice((prev) => ({ ...prev, [row.key]: opt }))
                                    }
                                    className={`px-2 py-0.5 rounded text-[10px] uppercase cursor-pointer transition-colors ${
                                      pick === opt
                                        ? "bg-[var(--accent)] text-white font-semibold"
                                        : "text-[var(--text-muted)] hover:text-white"
                                    }`}
                                  >
                                    {opt === "left" ? "A" : opt === "right" ? "B" : "Avg"}
                                  </button>
                                ))}
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                )}
              </div>

              <div className="flex items-center justify-between pt-1 shrink-0">
                <span className="text-xs text-[var(--text-muted)]">
                  {diffEntries.length} differing parameter(s) ready for merge
                </span>
                <Button
                  variant="primary"
                  size="sm"
                  disabled={diffEntries.length === 0}
                  icon={<CheckCircle2 className="w-3.5 h-3.5" />}
                  onClick={handleSaveMerged}
                >
                  Save Merged Setup
                </Button>
              </div>
            </div>
          )}

          {modeTab === "stint" && (
            <div className="flex flex-col h-full gap-4 overflow-y-auto pr-1">
              {/* Verdict Banner */}
              <div className="p-4 rounded-2xl bg-[linear-gradient(135deg,rgba(var(--accent-rgb),0.16),rgba(16,185,129,0.08))] border border-white/[0.1] flex flex-wrap items-center justify-between gap-3">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-xl bg-emerald-500/20 border border-emerald-500/35 flex items-center justify-center text-emerald-300">
                    <Trophy className="w-5 h-5" />
                  </div>
                  <div>
                    <div className="text-[10px] font-mono uppercase tracking-wider text-emerald-300">
                      Stint A/B Verdict
                    </div>
                    <div className="text-sm font-semibold text-white">
                      {deltaBest < 0 ? stintB.setupName : stintA.setupName} is faster by{" "}
                      <span className="font-mono text-emerald-300">
                        {Math.abs(deltaBest).toFixed(3)}s
                      </span>{" "}
                      on peak lap
                    </div>
                  </div>
                </div>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() =>
                    notify(
                      "Winner Promoted",
                      "success",
                      `${deltaBest < 0 ? stintB.setupName : stintA.setupName} marked as primary race setup`
                    )
                  }
                >
                  Promote Winner
                </Button>
              </div>

              {/* Side-by-side Scorecard */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {[
                  { rec: stintA, stats: statsA, setter: setStintA, accentColor: "text-cyan-300" },
                  { rec: stintB, stats: statsB, setter: setStintB, accentColor: "text-[var(--accent)]" },
                ].map(({ rec, stats, setter, accentColor }) => (
                  <div
                    key={rec.label}
                    className="p-4 rounded-2xl bg-white/[0.025] border border-white/[0.08] space-y-3"
                  >
                    <div className="flex items-center justify-between">
                      <div>
                        <div className={`text-[10px] font-mono uppercase tracking-wider ${accentColor}`}>
                          {rec.label}
                        </div>
                        <div className="font-mono text-xs text-white font-semibold mt-0.5">
                          {rec.setupName}
                        </div>
                      </div>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() =>
                          setter((prev) => ({
                            ...prev,
                            laps: [
                              ...prev.laps.slice(-4),
                              Number((stats.avg + (Math.random() - 0.5) * 0.45).toFixed(3)),
                            ],
                          }))
                        }
                      >
                        + Log Lap
                      </Button>
                    </div>

                    <div className="grid grid-cols-3 gap-2">
                      <KpiTile label="Best Lap" value={formatLapSec(stats.best)} className="p-2.5" />
                      <KpiTile label="Avg Pace" value={formatLapSec(stats.avg)} className="p-2.5" />
                      <KpiTile label="Sigma σ" value={`${stats.sigma.toFixed(3)}s`} className="p-2.5" />
                    </div>

                    <div className="grid grid-cols-3 gap-2 text-center font-mono text-[11px] bg-black/30 rounded-xl p-2.5 border border-white/[0.05]">
                      <div>
                        <div className="text-[9px] text-[var(--text-muted)]">SECTOR 1</div>
                        <div className="text-white">{rec.sector1.toFixed(2)}s</div>
                      </div>
                      <div>
                        <div className="text-[9px] text-[var(--text-muted)]">SECTOR 2</div>
                        <div className="text-white">{rec.sector2.toFixed(2)}s</div>
                      </div>
                      <div>
                        <div className="text-[9px] text-[var(--text-muted)]">SECTOR 3</div>
                        <div className="text-white">{rec.sector3.toFixed(2)}s</div>
                      </div>
                    </div>

                    <div className="flex items-center justify-between text-xs font-mono text-[var(--text-secondary)] pt-1">
                      <span>Core Temp: {rec.tyreTempC}°C</span>
                      <span>Tyre Life: {rec.tyreWearPct}%</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default SetupsPage;

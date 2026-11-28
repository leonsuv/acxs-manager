import { useEffect, useMemo, useState } from "react";
import {
  Check,
  CheckSquare,
  Eye,
  FolderOpen,
  Gauge,
  Heart,
  Layers,
  Play,
  PowerOff,
  Search,
  SlidersHorizontal,
  Sparkles,
  Star,
  Tag,
  X,
  Zap,
  Wind,
  Disc,
} from "lucide-react";
import { api, assetUrl, isTauri } from "../lib/api";
import type { CarEntry, CarPhysicsData, SkinEntry } from "../lib/types";
import { useAppStore } from "../store/appStore";
import { useContextMenu } from "../components/ContextMenu";
import {
  Badge,
  Button,
  EmptyState,
  KpiTile,
  PageHeader,
  SegmentedControl,
} from "../components/Primitives";

type CarFilter = "all" | "race" | "street" | "favorites" | "mods" | "disabled";
type SortKey = "name" | "bhp" | "weight" | "pw" | "year";

const FAVORITES_KEY = "acxs_favorite_cars_v1";
const CUSTOM_TAGS_KEY = "acxs_custom_car_tags_v1";

function parseNum(v?: string | number | null): number {
  if (v == null) return 0;
  if (typeof v === "number") return Number.isFinite(v) ? v : 0;
  const m = String(v).replace(/,/g, "").match(/[\d.]+/);
  return m ? parseFloat(m[0]) : 0;
}

function loadSet(key: string): Set<string> {
  try {
    const raw = localStorage.getItem(key);
    return raw ? new Set(JSON.parse(raw) as string[]) : new Set();
  } catch {
    return new Set();
  }
}

function loadCustomTags(): Record<string, string[]> {
  try {
    const raw = localStorage.getItem(CUSTOM_TAGS_KEY);
    return raw ? (JSON.parse(raw) as Record<string, string[]>) : {};
  } catch {
    return {};
  }
}

function matchesQueryClient(car: CarEntry, rawQuery: string, extraTags: string[]): boolean {
  const tokens = rawQuery
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  if (tokens.length === 0) return true;

  const allTags = [...car.tags, ...extraTags].map((t) => t.toLowerCase());
  const bhp = parseNum(car.bhp);
  const weight = parseNum(car.weight);
  const year = car.year ?? 0;

  return tokens.every((tok) => {
    if (tok.startsWith("brand:")) {
      return car.brand.toLowerCase().includes(tok.slice(6));
    }
    if (tok.startsWith("class:")) {
      return car.class.toLowerCase().includes(tok.slice(6));
    }
    if (tok.startsWith("country:")) {
      return (car.country ?? "").toLowerCase().includes(tok.slice(8));
    }
    if (tok.startsWith("tag:")) {
      const needle = tok.slice(4);
      return allTags.some((t) => t.includes(needle));
    }
    const cmp = tok.match(/^(bhp|weight|year)(>=|<=|>|<|=)(\d+(?:\.\d+)?)$/);
    if (cmp) {
      const [, field, op, numStr] = cmp;
      const target = parseFloat(numStr);
      const val = field === "bhp" ? bhp : field === "weight" ? weight : year;
      if (op === ">") return val > target;
      if (op === ">=") return val >= target;
      if (op === "<") return val < target;
      if (op === "<=") return val <= target;
      return val === target;
    }
    return (
      car.name.toLowerCase().includes(tok) ||
      car.brand.toLowerCase().includes(tok) ||
      car.class.toLowerCase().includes(tok) ||
      (car.country ?? "").toLowerCase().includes(tok) ||
      allTags.some((t) => t.includes(tok))
    );
  });
}

function synthesizePhysicsFallback(car: CarEntry): CarPhysicsData {
  const peakHp = parseNum(car.bhp) || 500;
  const peakTq = parseNum(car.torque) || 480;
  const massKg = parseNum(car.weight) || 1280;
  const topSpeed = parseNum(car.topSpeed) || 295;
  const redline = car.class === "race" ? 8500 : 7600;
  const rpmCurve = Array.from({ length: 12 }, (_, i) =>
    Math.round(1500 + ((redline - 1500) * i) / 11)
  );
  const torqueNm = rpmCurve.map((rpm) => {
    const f = rpm / redline;
    const bell = Math.exp(-Math.pow((f - 0.65) / 0.36, 2));
    return Math.round(peakTq * (0.58 + 0.42 * bell));
  });
  const powerHp = rpmCurve.map((rpm, i) =>
    Math.min(peakHp, Math.round((torqueNm[i] * rpm) / 7127))
  );
  const gearRatios = [3.15, 2.24, 1.72, 1.38, 1.14, 0.94];
  const gearTopSpeedsKmh = gearRatios.map((_, i) =>
    Math.round(topSpeed * ((i + 1) / gearRatios.length) * (0.72 + i * 0.056))
  );
  return {
    rpmCurve,
    torqueNm,
    powerHp,
    gearRatios,
    gearTopSpeedsKmh,
    redlineRpm: redline,
    massKg,
    aeroCd: car.class === "race" ? 0.38 : 0.31,
    aeroCl: car.class === "race" ? 1.42 : 0.34,
    tyreCompounds:
      car.class === "race"
        ? ["Slick Soft (S)", "Slick Medium (M)", "Slick Hard (H)", "Wet (W)"]
        : ["Street (ST)", "Semislick (SM)"],
    hasPackedAcd: true,
    hasUnpackedData: false,
  };
}

function DynoChart({ physics }: { physics: CarPhysicsData }) {
  const w = 360;
  const h = 150;
  const padL = 32;
  const padR = 14;
  const padT = 14;
  const padB = 24;

  const maxRpm = Math.max(...physics.rpmCurve, physics.redlineRpm, 7000);
  const minRpm = Math.min(...physics.rpmCurve, 1000);
  const maxVal = Math.max(...physics.powerHp, ...physics.torqueNm, 300) * 1.1;

  const xOf = (rpm: number) =>
    padL + ((rpm - minRpm) / Math.max(maxRpm - minRpm, 1)) * (w - padL - padR);
  const yOf = (val: number) =>
    h - padB - (val / Math.max(maxVal, 1)) * (h - padT - padB);

  const hpPath = physics.rpmCurve
    .map((rpm, i) => `${i === 0 ? "M" : "L"}${xOf(rpm).toFixed(1)},${yOf(physics.powerHp[i] ?? 0).toFixed(1)}`)
    .join(" ");
  const tqPath = physics.rpmCurve
    .map((rpm, i) => `${i === 0 ? "M" : "L"}${xOf(rpm).toFixed(1)},${yOf(physics.torqueNm[i] ?? 0).toFixed(1)}`)
    .join(" ");

  return (
    <svg viewBox={`0 0 ${w} ${h}`} className="w-full h-auto">
      {[0.25, 0.5, 0.75, 1].map((f) => {
        const y = padT + (h - padT - padB) * (1 - f);
        return (
          <g key={f}>
            <line
              x1={padL}
              y1={y}
              x2={w - padR}
              y2={y}
              stroke="rgba(255,255,255,0.06)"
              strokeDasharray="3 3"
            />
            <text
              x={padL - 5}
              y={y + 3}
              textAnchor="end"
              fill="rgba(148,163,184,0.6)"
              fontSize="8"
              fontFamily="JetBrains Mono, monospace"
            >
              {Math.round(maxVal * f)}
            </text>
          </g>
        );
      })}

      {/* Torque curve */}
      <path d={tqPath} fill="none" stroke="#00D1FF" strokeWidth="2" strokeLinecap="round" />
      {/* Power curve */}
      <path d={hpPath} fill="none" stroke="var(--accent)" strokeWidth="2.2" strokeLinecap="round" />

      {/* Redline marker */}
      <line
        x1={xOf(physics.redlineRpm)}
        y1={padT}
        x2={xOf(physics.redlineRpm)}
        y2={h - padB}
        stroke="#EF4444"
        strokeWidth="1"
        strokeDasharray="3 2"
        opacity="0.7"
      />
      <text
        x={xOf(physics.redlineRpm) - 3}
        y={padT + 9}
        textAnchor="end"
        fill="#F87171"
        fontSize="8"
        fontFamily="JetBrains Mono, monospace"
      >
        {physics.redlineRpm} RPM
      </text>

      {/* X axis labels */}
      {[minRpm, Math.round((minRpm + maxRpm) / 2), maxRpm].map((rpm) => (
        <text
          key={rpm}
          x={xOf(rpm)}
          y={h - 7}
          textAnchor="middle"
          fill="rgba(148,163,184,0.65)"
          fontSize="8"
          fontFamily="JetBrains Mono, monospace"
        >
          {(rpm / 1000).toFixed(1)}k
        </text>
      ))}
    </svg>
  );
}

export function CarsPage() {
  const { cars, armedSession, armCar, navigate, notify, rescan } = useAppStore();
  const { openContextMenu } = useContextMenu();

  const [query, setQuery] = useState("");
  const [serverResults, setServerResults] = useState<CarEntry[] | null>(null);
  const [filter, setFilter] = useState<CarFilter>("all");
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [favorites, setFavorites] = useState<Set<string>>(() => loadSet(FAVORITES_KEY));
  const [customTags, setCustomTags] = useState<Record<string, string[]>>(() => loadCustomTags());
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [batchTagInput, setBatchTagInput] = useState("");
  const [activeCar, setActiveCar] = useState<CarEntry | null>(null);
  const [physics, setPhysics] = useState<CarPhysicsData | null>(null);
  const [loadingPhysics, setLoadingPhysics] = useState(false);
  const [skins, setSkins] = useState<SkinEntry[]>([]);
  const [selectedSkinId, setSelectedSkinId] = useState<string>("");

  // Query backend search_cars in Tauri mode
  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed || !isTauri()) {
      setServerResults(null);
      return;
    }
    let cancelled = false;
    const t = window.setTimeout(() => {
      api
        .searchCars(trimmed)
        .then((res) => {
          if (!cancelled) setServerResults(res ?? null);
        })
        .catch(() => {
          if (!cancelled) setServerResults(null);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [query]);

  // Close inspector on Escape
  useEffect(() => {
    if (!activeCar) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setActiveCar(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeCar]);

  // Load physics & skins when inspector opens or switches car
  useEffect(() => {
    if (!activeCar) {
      setPhysics(null);
      setSkins([]);
      setSelectedSkinId("");
      return;
    }
    let cancelled = false;
    setLoadingPhysics(true);
    setSelectedSkinId(
      armedSession.carId === activeCar.id ? armedSession.skinId : ""
    );
    api
      .getCarPhysics(activeCar.path)
      .then((res) => {
        if (!cancelled) {
          setPhysics(res && res.rpmCurve?.length ? res : synthesizePhysicsFallback(activeCar));
        }
      })
      .catch(() => {
        if (!cancelled) setPhysics(synthesizePhysicsFallback(activeCar));
      })
      .finally(() => {
        if (!cancelled) setLoadingPhysics(false);
      });

    api
      .getCarSkins(activeCar.path)
      .then((list) => {
        if (cancelled) return;
        const resolved = list ?? [];
        setSkins(resolved);
        setSelectedSkinId((prev) =>
          prev && resolved.some((s) => s.id === prev)
            ? prev
            : resolved[0]?.id ?? ""
        );
      })
      .catch(() => {
        if (!cancelled) {
          setSkins([]);
          setSelectedSkinId("");
        }
      });

    return () => {
      cancelled = true;
    };
  }, [activeCar, armedSession.carId, armedSession.skinId]);

  const toggleFavorite = (id: string) => {
    setFavorites((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      localStorage.setItem(FAVORITES_KEY, JSON.stringify(Array.from(next)));
      return next;
    });
  };

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const filteredCars = useMemo(() => {
    const base = serverResults ?? cars;
    const list = base.filter((car) => {
      if (!serverResults && query.trim()) {
        if (!matchesQueryClient(car, query, customTags[car.id] ?? [])) return false;
      }
      if (filter === "race") return car.class.toLowerCase().includes("race");
      if (filter === "street") return car.class.toLowerCase().includes("street");
      if (filter === "favorites") return favorites.has(car.id);
      if (filter === "mods") return Boolean(car.isMod) || !car.id.startsWith("ks_");
      if (filter === "disabled") return car.enabled === false;
      return true;
    });

    return [...list].sort((a, b) => {
      if (sortKey === "bhp") return parseNum(b.bhp) - parseNum(a.bhp);
      if (sortKey === "weight") return (parseNum(a.weight) || 9999) - (parseNum(b.weight) || 9999);
      if (sortKey === "pw") return (parseNum(a.pwRatio) || 99) - (parseNum(b.pwRatio) || 99);
      if (sortKey === "year") return (b.year ?? 0) - (a.year ?? 0);
      return a.name.localeCompare(b.name);
    });
  }, [cars, serverResults, query, filter, sortKey, favorites, customTags]);

  const handleBatchDisable = async (enable: boolean) => {
    const ids = Array.from(selectedIds);
    try {
      await Promise.all(
        ids.map((id) => api.toggleContentEnabled("car", id, enable))
      );
      notify(
        enable ? "Vehicles Enabled" : "Vehicles Disabled",
        "success",
        `${ids.length} car(s) updated`
      );
      setSelectedIds(new Set());
      void rescan(true);
    } catch (err) {
      notify("Batch update failed", "error", String(err));
    }
  };

  const handleBatchAddTag = () => {
    const tag = batchTagInput.trim();
    if (!tag) return;
    const normalized = tag.startsWith("#") ? tag : `#${tag}`;
    setCustomTags((prev) => {
      const next = { ...prev };
      selectedIds.forEach((id) => {
        const cur = next[id] ?? [];
        if (!cur.includes(normalized)) next[id] = [...cur, normalized];
      });
      localStorage.setItem(CUSTOM_TAGS_KEY, JSON.stringify(next));
      return next;
    });
    notify("Tag applied", "success", `Added ${normalized} to ${selectedIds.size} cars`);
    setBatchTagInput("");
  };

  const filterPills: Array<{ id: CarFilter; label: string; count?: number }> = [
    { id: "all", label: "All", count: cars.length },
    { id: "race", label: "Race" },
    { id: "street", label: "Street" },
    { id: "favorites", label: "Favorites", count: favorites.size },
    { id: "mods", label: "Upgrades & Mods" },
    { id: "disabled", label: "Disabled" },
  ];

  return (
    <div className="h-full flex flex-col lg:flex-row overflow-hidden animate-fade-in">
      {/* Main Car Grid Pane */}
      <div className="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden relative">
        <div className="p-6 pb-4 space-y-4 shrink-0">
        <PageHeader
          eyebrow="Garage · Fleet Index"
          title="Cars"
          subtitle={`${filteredCars.length} of ${cars.length} vehicles · Supports query syntax like brand:ferrari bhp>500`}
          actions={
            <SegmentedControl
              size="sm"
              value={sortKey}
              onChange={setSortKey}
              options={[
                { value: "name", label: "Name" },
                { value: "bhp", label: "BHP" },
                { value: "weight", label: "Weight" },
                { value: "pw", label: "P/W" },
                { value: "year", label: "Year" },
              ]}
            />
          }
        />

        {/* Search + Filter Pills */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="relative flex-1 min-w-[260px] max-w-xl">
            <Search className="w-4 h-4 text-[var(--text-muted)] absolute left-3.5 top-1/2 -translate-y-1/2 pointer-events-none" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder='Search cars or filter: brand:ferrari bhp>500 weight<1300'
              className="w-full h-10 pl-10 pr-9 rounded-xl bg-white/[0.04] border border-white/[0.09] text-[13px] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-[var(--accent-border)] focus:bg-white/[0.06] transition-colors outline-none"
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery("")}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 p-1 rounded-md text-[var(--text-muted)] hover:text-white cursor-pointer"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            {filterPills.map((p) => {
              const active = filter === p.id;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setFilter(p.id)}
                  className={`h-8 px-3 rounded-lg text-xs font-medium border transition-all cursor-pointer flex items-center gap-1.5 ${
                    active
                      ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                      : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white hover:border-white/[0.16]"
                  }`}
                >
                  <span>{p.label}</span>
                  {p.count !== undefined && (
                    <span className="font-mono text-[10px] opacity-75">{p.count}</span>
                  )}
                </button>
              );
            })}
          </div>
        </div>
      </div>

      {/* Scrollable Visual Card Grid */}
      <div className="flex-1 overflow-y-auto px-6 pb-24">
        {filteredCars.length === 0 ? (
          <EmptyState
            icon={<SlidersHorizontal className="w-5 h-5" />}
            title="No matching vehicles"
            description="Adjust your search query or filter pills to view more cars in your garage."
            action={
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  setQuery("");
                  setFilter("all");
                }}
              >
                Reset Filters
              </Button>
            }
          />
        ) : (
          <div
            className={`grid gap-4 ${
              activeCar
                ? "grid-cols-1 xl:grid-cols-2"
                : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
            }`}
          >
            {filteredCars.map((car) => {
              const img = assetUrl(car.previewPath);
              const isSelected = selectedIds.has(car.id);
              const isInspecting = activeCar?.id === car.id;
              const isFav = favorites.has(car.id);
              const isArmed = armedSession.carId === car.id;
              const tags = [...car.tags, ...(customTags[car.id] ?? [])];

              return (
                <div
                  key={car.id}
                  onClick={() => setActiveCar(car)}
                  onContextMenu={(e) => {
                    openContextMenu(e, [
                      {
                        id: "car-arm",
                        label: "Arm Car for Drive",
                        icon: <Check className="w-3.5 h-3.5" />,
                        onSelect: () => {
                          armCar(car);
                          notify("Armed Car", "success", car.name);
                        },
                      },
                      {
                        id: "car-showroom",
                        label: "Open in 3D Showroom",
                        icon: <Sparkles className="w-3.5 h-3.5" />,
                        onSelect: () => {
                          armCar(car);
                          navigate("garage", "showroom");
                        },
                      },
                      {
                        id: "car-favorite",
                        label: isFav ? "Remove from Favorites" : "Add to Favorites",
                        icon: <Star className="w-3.5 h-3.5" />,
                        divider: true,
                        onSelect: () => toggleFavorite(car.id),
                      },
                      {
                        id: "car-folder",
                        label: "Open Car Folder",
                        icon: <FolderOpen className="w-3.5 h-3.5" />,
                        onSelect: () => void api.openPathInExplorer(car.path),
                      },
                    ]);
                  }}
                  className={`group glass-panel rounded-2xl overflow-hidden transition-all duration-150 cursor-pointer flex flex-col ${
                    isInspecting || isSelected
                      ? "ring-2 ring-[var(--accent)] border-[var(--accent)]"
                      : isArmed
                      ? "border-[var(--accent-border)]"
                      : "hover:border-white/[0.18] hover:-translate-y-0.5"
                  }`}
                >
                  {/* 16:9 Preview Header */}
                  <div className="relative aspect-[16/9] bg-[#080B12] overflow-hidden">
                    {img ? (
                      <img
                        src={img}
                        alt={car.name}
                        className="w-full h-full object-cover group-hover:scale-[1.03] transition-transform duration-300"
                      />
                    ) : (
                      <div className="w-full h-full flex flex-col items-center justify-center bg-[radial-gradient(ellipse_at_50%_30%,rgba(var(--accent-rgb),0.18),#080B12_75%)] px-4 text-center">
                        <span className="font-mono text-[10px] uppercase tracking-[0.2em] text-[var(--accent)]">
                          {car.brand}
                        </span>
                        <span className="font-display text-xl font-bold uppercase tracking-wider text-white/80 mt-0.5 line-clamp-1">
                          {car.name}
                        </span>
                      </div>
                    )}
                    <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-transparent to-black/35" />

                    {/* Top-left Select Checkbox */}
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleSelect(car.id);
                      }}
                      title="Select for batch action"
                      className={`absolute top-2.5 left-2.5 h-6 w-6 rounded-md border flex items-center justify-center transition-all cursor-pointer ${
                        isSelected
                          ? "bg-[var(--accent)] border-[var(--accent)] text-white"
                          : "bg-black/55 border-white/20 text-transparent group-hover:text-white/50 hover:border-white/50"
                      }`}
                    >
                      <Check className="w-3.5 h-3.5" />
                    </button>

                    {/* Top-right Fav + Armed */}
                    <div className="absolute top-2.5 right-2.5 flex items-center gap-1.5">
                      {isArmed && <Badge variant="accent" dot>Armed</Badge>}
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          toggleFavorite(car.id);
                        }}
                        className={`h-7 w-7 rounded-lg backdrop-blur-md border flex items-center justify-center transition-colors cursor-pointer ${
                          isFav
                            ? "bg-red-500/25 border-red-500/40 text-red-400"
                            : "bg-black/50 border-white/10 text-white/60 hover:text-white"
                        }`}
                      >
                        <Heart className={`w-3.5 h-3.5 ${isFav ? "fill-current" : ""}`} />
                      </button>
                    </div>

                    {/* Bottom-left Brand/Class */}
                    <div className="absolute bottom-2 left-3 right-3 flex items-center justify-between">
                      <span className="text-[10px] font-mono uppercase tracking-widest text-white/80">
                        {car.brand} {car.year ? `· ${car.year}` : ""}
                      </span>
                      <Badge variant="neutral" className="uppercase text-[10px]">
                        {car.class}
                      </Badge>
                    </div>
                  </div>

                  {/* Card Body */}
                  <div className="p-3.5 flex-1 flex flex-col justify-between gap-3">
                    <div>
                      <h3 className="text-[14px] font-semibold text-[var(--text-primary)] truncate">
                        {car.name}
                      </h3>
                      <div className="flex flex-wrap gap-1 mt-1.5">
                        {tags.slice(0, 3).map((t) => (
                          <span
                            key={t}
                            className="px-1.5 py-0.5 rounded bg-white/[0.04] text-[10px] font-mono text-[var(--text-muted)]"
                          >
                            {t}
                          </span>
                        ))}
                      </div>
                    </div>

                    <div className="grid grid-cols-3 gap-1.5 pt-2.5 border-t border-white/[0.06] font-mono text-[11px]">
                      <div>
                        <div className="text-[9px] uppercase text-[var(--text-muted)]">Power</div>
                        <div className="text-[var(--text-primary)] font-semibold truncate">
                          {car.bhp || "—"}
                        </div>
                      </div>
                      <div>
                        <div className="text-[9px] uppercase text-[var(--text-muted)]">Weight</div>
                        <div className="text-[var(--text-primary)] font-semibold truncate">
                          {car.weight || "—"}
                        </div>
                      </div>
                      <div>
                        <div className="text-[9px] uppercase text-[var(--text-muted)]">P/W</div>
                        <div className="text-[var(--accent)] font-semibold truncate">
                          {car.pwRatio || "—"}
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Multi-Select Floating Batch Action Bar */}
      {selectedIds.size > 0 && (
        <div className="absolute inset-x-4 bottom-5 z-30 flex justify-center pointer-events-none">
          <div className="pointer-events-auto w-max max-w-full glass-panel rounded-2xl px-3.5 py-2 border border-white/[0.15] shadow-2xl flex flex-wrap items-center justify-center gap-2.5 animate-scale-in">
            <div className="flex items-center gap-2 pr-2 border-r border-white/[0.1]">
              <CheckSquare className="w-4 h-4 text-[var(--accent)]" />
              <span className="text-xs font-mono font-semibold text-white">
                {selectedIds.size} selected
              </span>
            </div>

            <Button
              variant="danger"
              size="sm"
              icon={<PowerOff className="w-3.5 h-3.5" />}
              onClick={() => void handleBatchDisable(false)}
            >
              Disable
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void handleBatchDisable(true)}
            >
              Enable
            </Button>

            <div className="flex items-center gap-1.5 pl-1 border-l border-white/[0.1]">
              <input
                type="text"
                value={batchTagInput}
                onChange={(e) => setBatchTagInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleBatchAddTag()}
                placeholder="#endurance"
                className="h-8 w-28 px-2.5 rounded-lg bg-black/40 border border-white/[0.1] text-xs font-mono text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
              />
              <Button
                variant="secondary"
                size="sm"
                icon={<Tag className="w-3.5 h-3.5" />}
                onClick={handleBatchAddTag}
              >
                Add Tag
              </Button>
            </div>

            <button
              type="button"
              onClick={() => setSelectedIds(new Set())}
              className="text-xs text-[var(--text-muted)] hover:text-white px-1.5 cursor-pointer"
            >
              Clear
            </button>
          </div>
        </div>
      )}
      </div>

      {/* Non-Blocking Split Side Car Inspector */}
      {activeCar && (
        <aside className="w-full lg:w-[420px] xl:w-[480px] max-h-[50vh] lg:max-h-none shrink-0 lg:h-full bg-[#0A0E15] border-t lg:border-t-0 lg:border-l border-white/[0.09] flex flex-col overflow-hidden animate-fade-in">
          {/* Inspector Hero */}
          <div className="relative h-44 shrink-0 bg-[#070A10] overflow-hidden border-b border-white/[0.08]">
            {assetUrl(activeCar.previewPath) ? (
              <img
                src={assetUrl(activeCar.previewPath)!}
                alt={activeCar.name}
                className="w-full h-full object-cover"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center bg-[radial-gradient(ellipse_at_50%_30%,rgba(var(--accent-rgb),0.22),#070A10_75%)]">
                <Sparkles className="w-10 h-10 text-[var(--accent)] opacity-70" />
              </div>
            )}
            <div className="absolute inset-0 bg-gradient-to-t from-[#0A0E15] via-[#0A0E15]/30 to-black/40" />

            <button
              type="button"
              onClick={() => setActiveCar(null)}
              aria-label="Close car inspector"
              className="absolute top-3 right-3 h-8 w-8 rounded-lg bg-black/60 border border-white/[0.12] flex items-center justify-center text-white/80 hover:text-white cursor-pointer"
            >
              <X className="w-4 h-4" />
            </button>

            <div className="absolute bottom-3 left-5 right-5 flex items-end justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-[11px] font-mono uppercase tracking-[0.14em] text-[var(--accent)]">
                    {activeCar.brand}
                  </span>
                  {activeCar.year && <Badge variant="neutral">{activeCar.year}</Badge>}
                  <Badge variant="accent" className="uppercase">
                    {activeCar.class}
                  </Badge>
                </div>
                <h2 className="font-display text-2xl xl:text-3xl font-bold uppercase tracking-wide text-white leading-none truncate">
                  {activeCar.name}
                </h2>
              </div>
            </div>
          </div>

          {/* Action Bar */}
          <div className="px-5 py-3 border-b border-white/[0.06] flex items-center gap-2 shrink-0 bg-white/[0.015]">
            <Button
              variant="primary"
              size="sm"
              icon={<Play className="w-3.5 h-3.5 fill-current" />}
              onClick={() => {
                armCar(activeCar, selectedSkinId || undefined);
                notify("Armed for Drive", "success", activeCar.name);
                navigate("drive", "quick-race");
              }}
            >
              Arm for Drive
            </Button>
            <Button
              variant="secondary"
              size="sm"
              icon={<Eye className="w-3.5 h-3.5" />}
              onClick={() => {
                armCar(activeCar, selectedSkinId || undefined);
                navigate("garage", "showroom");
              }}
            >
              Showroom
            </Button>
            <Button
              variant="ghost"
              size="sm"
              icon={<FolderOpen className="w-3.5 h-3.5" />}
              onClick={() => void api.openPathInExplorer(activeCar.path)}
            >
              Folder
            </Button>
          </div>

          {/* Scrollable Telemetry, Liveries & Physics Content */}
          <div className="flex-1 overflow-y-auto p-5 space-y-5">
            {/* Clickable Livery Selector */}
            <div className="glass-panel rounded-xl p-3.5 space-y-2">
              <div className="flex items-center justify-between text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                <span>Livery Selection ({skins.length})</span>
                {selectedSkinId && (
                  <span className="text-[var(--accent)] truncate max-w-[160px]">
                    {skins.find((s) => s.id === selectedSkinId)?.name ?? selectedSkinId}
                  </span>
                )}
              </div>
              {skins.length === 0 ? (
                <div className="text-xs text-[var(--text-muted)]">
                  Default factory livery
                </div>
              ) : (
                <div className="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto">
                  {skins.map((skin) => {
                    const active = selectedSkinId === skin.id;
                    return (
                      <button
                        key={skin.id}
                        type="button"
                        onClick={() => setSelectedSkinId(skin.id)}
                        className={`px-2.5 py-1 rounded-lg text-[11px] border transition-colors cursor-pointer ${
                          active
                            ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)] font-semibold"
                            : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                        }`}
                      >
                        {skin.number ? `#${skin.number} ` : ""}
                        {skin.name}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Specs Grid */}
            <div className="grid grid-cols-3 gap-2.5">
              <KpiTile label="Power" value={activeCar.bhp || "—"} accent />
              <KpiTile
                label="Torque"
                value={
                  activeCar.torque ||
                  (physics && physics.torqueNm.length > 0
                    ? `${Math.round(Math.max(...physics.torqueNm))} Nm`
                    : "—")
                }
              />
              <KpiTile
                label="Weight"
                value={
                  activeCar.weight ||
                  (physics && physics.massKg > 0 ? `${physics.massKg} kg` : "—")
                }
              />
              <KpiTile
                label="Top Speed"
                value={
                  activeCar.topSpeed ||
                  (physics && physics.gearTopSpeedsKmh.length > 0
                    ? `${Math.round(Math.max(...physics.gearTopSpeedsKmh))} km/h`
                    : "—")
                }
              />
              <KpiTile label="P/W Ratio" value={activeCar.pwRatio || "—"} />
              <KpiTile label="Liveries" value={activeCar.skinsCount ?? skins.length} />
            </div>

            {/* Physics Dyno Preview */}
            <div className="glass-panel rounded-2xl p-4 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Gauge className="w-4 h-4 text-[var(--accent)]" />
                  <span className="text-xs font-semibold uppercase tracking-wider">
                    Engine Dyno Curve
                  </span>
                </div>
                <div className="flex items-center gap-3 text-[10px] font-mono">
                  <span className="flex items-center gap-1 text-[var(--accent)]">
                    <span className="w-2 h-0.5 bg-[var(--accent)]" /> HP
                  </span>
                  <span className="flex items-center gap-1 text-cyan-400">
                    <span className="w-2 h-0.5 bg-cyan-400" /> Nm
                  </span>
                </div>
              </div>
              {loadingPhysics || !physics ? (
                <div className="h-36 rounded-xl acxs-skeleton" />
              ) : (
                <DynoChart physics={physics} />
              )}
            </div>

            {/* Aero, Tyres & Drivetrain */}
            {physics && (
              <div className="grid grid-cols-2 gap-3">
                <div className="glass-panel rounded-xl p-3.5 space-y-2">
                  <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                    <Wind className="w-3.5 h-3.5 text-cyan-400" />
                    Aero Profile
                  </div>
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-[var(--text-secondary)]">Drag (Cd)</span>
                    <span className="font-mono font-semibold text-white">
                      {physics.aeroCd?.toFixed(2) ?? "—"}
                    </span>
                  </div>
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-[var(--text-secondary)]">Downforce (Cl)</span>
                    <span className="font-mono font-semibold text-white">
                      {physics.aeroCl?.toFixed(2) ?? "—"}
                    </span>
                  </div>
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-[var(--text-secondary)]">Data ACD</span>
                    <Badge variant={physics.hasUnpackedData ? "warning" : "success"}>
                      {physics.hasUnpackedData ? "Unpacked" : "Packed"}
                    </Badge>
                  </div>
                </div>

                <div className="glass-panel rounded-xl p-3.5 space-y-2">
                  <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                    <Disc className="w-3.5 h-3.5 text-[var(--accent)]" />
                    Tyre Compounds
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {(physics.tyreCompounds ?? ["Default"]).map((comp) => (
                      <Badge key={comp} variant="neutral">
                        {comp}
                      </Badge>
                    ))}
                  </div>
                </div>
              </div>
            )}

            {/* Gear Top Speeds */}
            {physics && physics.gearTopSpeedsKmh.length > 0 && (
              <div className="glass-panel rounded-xl p-3.5 space-y-2.5">
                <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                  <Zap className="w-3.5 h-3.5 text-amber-400" />
                  Gearbox V-Max Projection
                </div>
                <div className="space-y-1.5">
                  {physics.gearTopSpeedsKmh.map((kmh, i) => {
                    const maxKmh = Math.max(...physics.gearTopSpeedsKmh, 300);
                    return (
                      <div key={i} className="flex items-center gap-2.5 text-xs font-mono">
                        <span className="w-7 text-[var(--text-muted)]">G{i + 1}</span>
                        <div className="flex-1 h-2 rounded-full bg-white/[0.06] overflow-hidden">
                          <div
                            className="h-full rounded-full bg-gradient-to-r from-cyan-500/70 to-[var(--accent)]"
                            style={{ width: `${(kmh / maxKmh) * 100}%` }}
                          />
                        </div>
                        <span className="w-20 text-right text-[var(--text-primary)] tabular-nums whitespace-nowrap shrink-0">
                          {Math.round(kmh)} km/h
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {/* Tags */}
            <div className="space-y-1.5">
              <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                <Layers className="w-3.5 h-3.5" />
                Tags &amp; Path
              </div>
              <div className="flex flex-wrap gap-1.5">
                {[...activeCar.tags, ...(customTags[activeCar.id] ?? [])].map((t) => (
                  <Badge key={t} variant="neutral">
                    {t}
                  </Badge>
                ))}
              </div>
              <div className="text-[11px] font-mono text-[var(--text-muted)] break-all pt-1">
                {activeCar.path}
              </div>
            </div>
          </div>
        </aside>
      )}
    </div>
  );
}

export default CarsPage;

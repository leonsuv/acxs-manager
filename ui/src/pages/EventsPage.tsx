import { useEffect, useMemo, useState } from "react";
import {
  Award,
  Car,
  Compass,
  Flame,
  Medal,
  Play,
  RefreshCw,
  Search,
  Trophy,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import type { SpecialEvent } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  KpiTile,
  PageHeader,
  Skeleton,
} from "../components/Primitives";

type MedalFilter = "All" | "Gold" | "Uncompleted";

function medalStyle(medal?: SpecialEvent["earnedMedal"]): {
  badge: "warning" | "neutral" | "accent" | "info";
  label: string;
  ring: string;
} {
  switch (medal) {
    case "Gold":
      return {
        badge: "warning",
        label: "GOLD",
        ring: "border-amber-400/45 shadow-[0_0_20px_-8px_rgba(251,191,36,0.5)]",
      };
    case "Silver":
      return {
        badge: "info",
        label: "SILVER",
        ring: "border-slate-300/35",
      };
    case "Bronze":
      return {
        badge: "accent",
        label: "BRONZE",
        ring: "border-orange-500/35",
      };
    default:
      return {
        badge: "neutral",
        label: "UNCLAIMED",
        ring: "border-white/[0.08]",
      };
  }
}

export function EventsPage() {
  const { cars, tracks, gameRunning, navigate, notify } = useAppStore();
  const [events, setEvents] = useState<SpecialEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [category, setCategory] = useState("All");
  const [medalFilter, setMedalFilter] = useState<MedalFilter>("All");
  const [query, setQuery] = useState("");

  const loadEvents = async () => {
    setLoading(true);
    try {
      const list = await api.listEvents();
      setEvents(list ?? []);
    } catch (err) {
      notify("Failed to load special events", "error", String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadEvents();
  }, []);

  const categories = useMemo(() => {
    const unique = Array.from(new Set(events.map((e) => e.category))).filter(
      Boolean
    );
    return ["All", ...unique];
  }, [events]);

  const stats = useMemo(() => {
    const gold = events.filter((e) => e.earnedMedal === "Gold").length;
    const silver = events.filter((e) => e.earnedMedal === "Silver").length;
    const bronze = events.filter((e) => e.earnedMedal === "Bronze").length;
    const completed = gold + silver + bronze;
    const pct =
      events.length > 0 ? Math.round((completed / events.length) * 100) : 0;
    return { gold, silver, bronze, completed, total: events.length, pct };
  }, [events]);

  const filteredEvents = useMemo(() => {
    const q = query.trim().toLowerCase();
    return events.filter((ev) => {
      if (category !== "All" && ev.category !== category) return false;
      if (medalFilter === "Gold" && ev.earnedMedal !== "Gold") return false;
      if (
        medalFilter === "Uncompleted" &&
        ev.earnedMedal &&
        ev.earnedMedal !== "None"
      ) {
        return false;
      }
      if (!q) return true;
      return (
        ev.name.toLowerCase().includes(q) ||
        ev.carId.toLowerCase().includes(q) ||
        ev.trackId.toLowerCase().includes(q) ||
        ev.category.toLowerCase().includes(q)
      );
    });
  }, [events, category, medalFilter, query]);

  const handleLaunchEvent = async (ev: SpecialEvent) => {
    if (gameRunning) {
      notify("Assetto Corsa is already running", "warning");
      return;
    }
    setLaunchingId(ev.id);
    try {
      await api.launchEvent(ev.id);
      notify("Special Event launched", "success", ev.name);
    } catch (err) {
      notify("Could not launch event", "error", String(err));
    } finally {
      setLaunchingId(null);
    }
  };

  return (
    <div className="h-full overflow-y-auto px-6 py-5 space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="Drive · Career & Challenges"
        title="Special Events"
        subtitle="Official Kunos challenges, hotlap targets, drift trials, and career milestones."
        actions={
          <Button
            variant="secondary"
            size="sm"
            loading={loading}
            icon={<RefreshCw className="w-3.5 h-3.5" />}
            onClick={loadEvents}
          >
            Refresh
          </Button>
        }
      />

      {/* Medal Progress Summary Strip */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3.5">
        <KpiTile
          label="Gold Medals"
          value={stats.gold}
          unit={`/ ${stats.total}`}
          sublabel="Top podium targets cleared"
          icon={<Trophy className="w-4 h-4 text-amber-400" />}
          accent
        />
        <KpiTile
          label="Silver Medals"
          value={stats.silver}
          unit="earned"
          sublabel="Competitive benchmark"
          icon={<Medal className="w-4 h-4 text-slate-300" />}
        />
        <KpiTile
          label="Bronze Medals"
          value={stats.bronze}
          unit="earned"
          sublabel="Qualification cleared"
          icon={<Award className="w-4 h-4 text-orange-400" />}
        />
        <Card variant="glass" padding="md" className="flex flex-col justify-between">
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-medium uppercase tracking-[0.08em] text-[var(--text-muted)]">
              Career Completion
            </span>
            <span className="font-mono text-sm font-semibold text-[var(--accent)] tabular-nums">
              {stats.pct}%
            </span>
          </div>
          <div className="space-y-1.5 mt-2">
            <div className="h-2 rounded-full bg-white/[0.07] overflow-hidden">
              <div
                className="h-full rounded-full transition-all duration-500"
                style={{
                  width: `${stats.pct}%`,
                  background:
                    "linear-gradient(90deg, #F59E0B 0%, var(--accent) 100%)",
                }}
              />
            </div>
            <div className="text-[11px] text-[var(--text-muted)] font-mono tabular-nums">
              {stats.completed} of {stats.total} events completed
            </div>
          </div>
        </Card>
      </div>

      {/* Category & Search Filter Bar */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-1.5">
          {categories.map((cat) => (
            <button
              key={cat}
              type="button"
              onClick={() => setCategory(cat)}
              className={`px-3 py-1.5 rounded-lg text-xs font-medium border transition-all cursor-pointer ${
                category === cat
                  ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                  : "bg-white/[0.04] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
              }`}
            >
              {cat}
            </button>
          ))}
        </div>

        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1 bg-black/40 p-1 rounded-lg border border-white/[0.07]">
            {(["All", "Uncompleted", "Gold"] as MedalFilter[]).map((mf) => (
              <button
                key={mf}
                type="button"
                onClick={() => setMedalFilter(mf)}
                className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors cursor-pointer ${
                  medalFilter === mf
                    ? "bg-[var(--bg-elevated)] text-white"
                    : "text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                }`}
              >
                {mf}
              </button>
            ))}
          </div>

          <div className="relative w-56">
            <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter events..."
              className="w-full h-8 pl-8 pr-3 rounded-lg bg-black/45 border border-white/[0.09] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
            />
          </div>
        </div>
      </div>

      {/* Events Grid */}
      {loading ? (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-60 rounded-xl" />
          ))}
        </div>
      ) : filteredEvents.length === 0 ? (
        <EmptyState
          icon={<Flame className="w-5 h-5" />}
          title={
            events.length === 0
              ? "No Special Events Found"
              : "No Events Match Your Filter"
          }
          description={
            events.length === 0
              ? "Connect your Assetto Corsa folder in Setup to load official special events and career challenges from content/specialevents."
              : "Try clearing your search query or selecting a different category."
          }
          action={
            events.length === 0 ? (
              <Button
                variant="primary"
                size="sm"
                onClick={() => navigate("drive", "quick-race")}
              >
                Go to Quick Race
              </Button>
            ) : (
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  setCategory("All");
                  setMedalFilter("All");
                  setQuery("");
                }}
              >
                Reset Filters
              </Button>
            )
          }
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {filteredEvents.map((ev) => {
            const car = cars.find((c) => c.id === ev.carId);
            const track = tracks.find((t) => t.id === ev.trackId);
            const previewUrl = assetUrl(car?.previewPath ?? track?.previewPath);
            const ms = medalStyle(ev.earnedMedal);

            return (
              <div
                key={ev.id}
                className={`glass-panel rounded-xl overflow-hidden flex flex-col border transition-all duration-150 hover:-translate-y-[1px] ${ms.ring}`}
              >
                {/* Card Hero */}
                <div className="relative h-32 bg-[#0A0E16] overflow-hidden">
                  {previewUrl ? (
                    <img
                      src={previewUrl}
                      alt={ev.name}
                      className="w-full h-full object-cover opacity-80"
                    />
                  ) : (
                    <div
                      className="w-full h-full"
                      style={{
                        background:
                          "radial-gradient(280px 130px at 80% 20%, rgba(var(--accent-rgb), 0.2), transparent 70%), linear-gradient(135deg, #121A28 0%, #090C12 100%)",
                      }}
                    />
                  )}
                  <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-[#0D1118]/45 to-transparent" />
                  <div className="absolute top-2.5 left-3 flex items-center gap-1.5">
                    <Badge variant="neutral">{ev.category}</Badge>
                    <Badge variant="info">{ev.mode}</Badge>
                  </div>
                  <div className="absolute top-2.5 right-3">
                    <Badge variant={ms.badge}>{ms.label}</Badge>
                  </div>
                  <div className="absolute bottom-2.5 inset-x-3">
                    <h3 className="font-display text-xl font-bold uppercase tracking-wide text-white truncate">
                      {ev.name}
                    </h3>
                  </div>
                </div>

                {/* Card Body */}
                <div className="p-3.5 flex-1 flex flex-col justify-between gap-3">
                  <div className="space-y-2">
                    <div className="flex items-center justify-between text-xs text-[var(--text-secondary)]">
                      <span className="flex items-center gap-1.5 truncate">
                        <Car className="w-3.5 h-3.5 text-[var(--accent)] shrink-0" />
                        <span className="truncate">{car?.name ?? ev.carId}</span>
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                      <span className="flex items-center gap-1.5 truncate">
                        <Compass className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
                        <span className="truncate">
                          {track?.name ?? ev.trackId}
                        </span>
                      </span>
                    </div>

                    {/* Medal Targets */}
                    {(ev.goldTarget || ev.silverTarget || ev.bronzeTarget) && (
                      <div className="grid grid-cols-3 gap-1.5 pt-1">
                        <div className="rounded-md bg-amber-500/10 border border-amber-500/20 px-2 py-1 text-center">
                          <div className="text-[9px] font-mono uppercase text-amber-300">
                            Gold
                          </div>
                          <div className="text-[11px] font-mono font-semibold text-white tabular-nums truncate">
                            {ev.goldTarget ?? "—"}
                          </div>
                        </div>
                        <div className="rounded-md bg-white/[0.04] border border-white/[0.08] px-2 py-1 text-center">
                          <div className="text-[9px] font-mono uppercase text-slate-300">
                            Silver
                          </div>
                          <div className="text-[11px] font-mono font-semibold text-white tabular-nums truncate">
                            {ev.silverTarget ?? "—"}
                          </div>
                        </div>
                        <div className="rounded-md bg-orange-500/10 border border-orange-500/20 px-2 py-1 text-center">
                          <div className="text-[9px] font-mono uppercase text-orange-300">
                            Bronze
                          </div>
                          <div className="text-[11px] font-mono font-semibold text-white tabular-nums truncate">
                            {ev.bronzeTarget ?? "—"}
                          </div>
                        </div>
                      </div>
                    )}
                  </div>

                  <div className="flex items-center justify-between gap-2 pt-2 border-t border-white/[0.06]">
                    <div className="text-[11px] font-mono text-[var(--text-muted)] truncate">
                      {ev.bestResult ? `Best: ${ev.bestResult}` : "No attempt"}
                    </div>
                    <Button
                      variant="primary"
                      size="sm"
                      loading={launchingId === ev.id}
                      disabled={gameRunning}
                      icon={<Play className="w-3 h-3 fill-current" />}
                      onClick={() => handleLaunchEvent(ev)}
                    >
                      Launch
                    </Button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

export default EventsPage;

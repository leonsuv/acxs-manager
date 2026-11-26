import { useEffect, useMemo, useState } from "react";
import {
  Award,
  CheckCircle2,
  Download,
  Flag,
  Play,
  Plus,
  RefreshCw,
  Scale,
  Trophy,
  X,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import type { Championship, ChampionshipRound } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  PageHeader,
  Skeleton,
} from "../components/Primitives";

type ScoringPresetKey = "f1" | "gt3" | "classic" | "custom";

const SCORING_PRESETS: Array<{
  key: ScoringPresetKey;
  label: string;
  points: number[];
}> = [
  { key: "f1", label: "F1 25-18-15", points: [25, 18, 15, 12, 10, 8, 6, 4, 2, 1] },
  { key: "gt3", label: "GT3 15-12-10", points: [15, 12, 10, 8, 6, 5, 4, 3, 2, 1] },
  { key: "classic", label: "Classic 10-6-4", points: [10, 6, 4, 3, 2, 1] },
  { key: "custom", label: "Custom", points: [20, 15, 12, 10, 8, 6, 4, 2, 1] },
];

function successBallastKg(positionIndex: number): number {
  if (positionIndex === 0) return 30;
  if (positionIndex === 1) return 20;
  if (positionIndex === 2) return 10;
  return 0;
}

export function ChampionshipsPage() {
  const { cars, tracks, gameRunning, notify } = useAppStore();

  const [championships, setChampionships] = useState<Championship[]>([]);
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [launchingRound, setLaunchingRound] = useState<number | null>(null);
  const [importing, setImporting] = useState(false);

  // Inline create bar state
  const [newName, setNewName] = useState("");
  const [newClass, setNewClass] = useState("GT3");
  const [newCarId, setNewCarId] = useState("");
  const [scoringKey, setScoringKey] = useState<ScoringPresetKey>("f1");
  const [customPointsStr, setCustomPointsStr] = useState("20,15,12,10,8,6,4,2,1");

  // Add round inline form
  const [addingRound, setAddingRound] = useState(false);
  const [roundTrackId, setRoundTrackId] = useState("");
  const [roundLaps, setRoundLaps] = useState(10);
  const [roundWeather, setRoundWeather] = useState("3_clear");

  const loadChampionships = async () => {
    setLoading(true);
    try {
      const list = await api.listChampionships();
      const safe = list ?? [];
      setChampionships(safe);
      if (safe.length > 0 && !selectedName) {
        setSelectedName(safe[0].name);
      }
    } catch (err) {
      notify("Failed to load championships", "error", String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadChampionships();
  }, []);

  useEffect(() => {
    if (!newCarId && cars.length > 0) setNewCarId(cars[0].id);
    if (!roundTrackId && tracks.length > 0) setRoundTrackId(tracks[0].id);
  }, [cars, tracks, newCarId, roundTrackId]);

  const selectedChamp = useMemo(
    () =>
      championships.find((c) => c.name === selectedName) ??
      championships[0] ??
      null,
    [championships, selectedName]
  );

  const parsedPoints = useMemo(() => {
    if (scoringKey !== "custom") {
      return (
        SCORING_PRESETS.find((p) => p.key === scoringKey)?.points ?? [
          25, 18, 15, 12, 10, 8, 6, 4, 2, 1,
        ]
      );
    }
    const nums = customPointsStr
      .split(",")
      .map((s) => Number(s.trim()))
      .filter((n) => Number.isFinite(n) && n >= 0);
    return nums.length > 0 ? nums : [10, 6, 4, 3, 2, 1];
  }, [scoringKey, customPointsStr]);

  const handleCreateChampionship = async () => {
    const trimmed = newName.trim();
    if (!trimmed) {
      notify("Enter a championship name", "warning");
      return;
    }
    const defaultRounds: ChampionshipRound[] = tracks.slice(0, 4).map((t, idx) => ({
      name: `Round ${idx + 1} · ${t.name}`,
      trackId: t.id,
      layoutId: t.layouts[0]?.id ?? "default",
      laps: 8,
      weatherId: "3_clear",
      completed: false,
    }));

    const champ: Championship = {
      name: trimmed,
      carClass: newClass.trim() || "GT3",
      playerCarId: newCarId || cars[0]?.id || "ks_porsche_911_gt3_r_2016",
      rounds: defaultRounds,
      pointsSystem: parsedPoints,
      standings: [
        { driver: "You", points: 0, wins: 0 },
        { driver: "M. Rossi", points: 0, wins: 0 },
        { driver: "L. Weber", points: 0, wins: 0 },
        { driver: "K. Tanaka", points: 0, wins: 0 },
      ],
    };

    setSaving(true);
    try {
      await api.saveChampionship(champ);
      setChampionships((prev) => [
        champ,
        ...prev.filter((c) => c.name !== champ.name),
      ]);
      setSelectedName(champ.name);
      setNewName("");
      notify("Championship created", "success", champ.name);
    } catch (err) {
      notify("Failed to save championship", "error", String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleAddRound = async () => {
    if (!selectedChamp) return;
    const track = tracks.find((t) => t.id === roundTrackId) ?? tracks[0];
    if (!track) return;
    const nextRound: ChampionshipRound = {
      name: `Round ${selectedChamp.rounds.length + 1} · ${track.name}`,
      trackId: track.id,
      layoutId: track.layouts[0]?.id ?? "default",
      laps: roundLaps,
      weatherId: roundWeather,
      completed: false,
    };
    const updated: Championship = {
      ...selectedChamp,
      rounds: [...selectedChamp.rounds, nextRound],
    };
    try {
      await api.saveChampionship(updated);
      setChampionships((prev) =>
        prev.map((c) => (c.name === updated.name ? updated : c))
      );
      setAddingRound(false);
      notify("Round added", "success", nextRound.name);
    } catch (err) {
      notify("Could not add round", "error", String(err));
    }
  };

  const handleLaunchRound = async (roundIdx: number) => {
    if (!selectedChamp) return;
    if (gameRunning) {
      notify("Assetto Corsa is already running", "warning");
      return;
    }
    setLaunchingRound(roundIdx);
    try {
      await api.launchChampionshipRound(selectedChamp.name, roundIdx);
      notify(
        "Championship round launched",
        "success",
        `${selectedChamp.name} · Round ${roundIdx + 1}`
      );
    } catch (err) {
      notify("Failed to launch round", "error", String(err));
    } finally {
      setLaunchingRound(null);
    }
  };

  const handleImportResults = async () => {
    setImporting(true);
    try {
      await api.importRaceOutResults();
      await loadChampionships();
      notify("Imported race_out.json results", "success");
    } catch (err) {
      notify("Could not import race_out.json", "error", String(err));
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="h-full flex flex-col overflow-hidden px-6 py-5 gap-4 animate-fade-in">
      <PageHeader
        eyebrow="Drive · Season Manager"
        title="Championships"
        subtitle="Build custom multi-round series, track standings with success ballast, and import race results."
        actions={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              loading={importing}
              icon={<Download className="w-3.5 h-3.5" />}
              onClick={handleImportResults}
            >
              Import race_out.json
            </Button>
            <Button
              variant="ghost"
              size="sm"
              loading={loading}
              icon={<RefreshCw className="w-3.5 h-3.5" />}
              onClick={loadChampionships}
            >
              Refresh
            </Button>
          </div>
        }
      />

      {/* Inline Create Bar with Tactile Scoring Preset Pills */}
      <Card variant="glass" padding="sm" className="shrink-0">
        <div className="flex flex-wrap items-center gap-2.5">
          <input
            type="text"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleCreateChampionship()}
            placeholder="New championship name (e.g. GT World Challenge EU)"
            className="h-9 flex-1 min-w-[200px] px-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
          />

          <input
            type="text"
            value={newClass}
            onChange={(e) => setNewClass(e.target.value)}
            placeholder="Class (GT3)"
            className="h-9 w-24 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-[var(--text-primary)] outline-none focus:border-[var(--accent-border)]"
          />

          <select
            value={newCarId}
            onChange={(e) => setNewCarId(e.target.value)}
            className="h-9 max-w-[200px] px-2.5 rounded-lg bg-black/60 border border-white/[0.1] text-xs text-[var(--text-primary)] outline-none focus:border-[var(--accent-border)]"
          >
            {cars.map((c) => (
              <option key={c.id} value={c.id} className="bg-[#0D1118]">
                {c.name}
              </option>
            ))}
          </select>

          <div className="flex items-center gap-1 bg-black/40 p-1 rounded-lg border border-white/[0.07]">
            {SCORING_PRESETS.map((preset) => (
              <button
                key={preset.key}
                type="button"
                onClick={() => setScoringKey(preset.key)}
                className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-all cursor-pointer ${
                  scoringKey === preset.key
                    ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
                    : "text-[var(--text-secondary)] hover:text-white border border-transparent"
                }`}
              >
                {preset.label}
              </button>
            ))}
          </div>

          {scoringKey === "custom" && (
            <input
              type="text"
              value={customPointsStr}
              onChange={(e) => setCustomPointsStr(e.target.value)}
              placeholder="25,18,15..."
              className="h-9 w-36 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-[var(--text-primary)] outline-none"
            />
          )}

          <Button
            variant="primary"
            size="sm"
            loading={saving}
            icon={<Plus className="w-3.5 h-3.5" />}
            onClick={handleCreateChampionship}
            className="h-9"
          >
            Create Series
          </Button>
        </div>
      </Card>

      {/* 2-Column Master-Detail Layout */}
      {loading ? (
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-4 flex-1 min-h-0">
          <Skeleton className="lg:col-span-4 h-full rounded-xl" />
          <Skeleton className="lg:col-span-8 h-full rounded-xl" />
        </div>
      ) : championships.length === 0 ? (
        <EmptyState
          icon={<Trophy className="w-5 h-5" />}
          title="No Championships Yet"
          description="Use the bar above to create your first custom season with F1, GT3, Classic, or custom points scoring."
        />
      ) : (
        <div className="flex flex-col lg:flex-row gap-4 flex-1 min-h-0">
          {/* Left Master List */}
          <div className="w-full lg:w-[320px] shrink-0 overflow-y-auto space-y-2.5 pr-1">
            {championships.map((champ) => {
              const completedCount = champ.rounds.filter((r) => r.completed).length;
              const active = selectedChamp?.name === champ.name;
              const leader = champ.standings?.[0];
              return (
                <button
                  key={champ.name}
                  type="button"
                  onClick={() => setSelectedName(champ.name)}
                  className={`w-full text-left rounded-xl p-3.5 border transition-all cursor-pointer ${
                    active
                      ? "bg-[var(--accent-soft)] border-[var(--accent-border)] shadow-md"
                      : "glass-panel hover:border-white/[0.16]"
                  }`}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <div className="font-display text-lg font-bold uppercase tracking-wide text-white truncate">
                        {champ.name}
                      </div>
                      <div className="text-[11px] font-mono text-[var(--text-muted)] mt-0.5">
                        {champ.carClass} · {completedCount}/{champ.rounds.length} rounds
                      </div>
                    </div>
                    <Badge variant={active ? "accent" : "neutral"}>
                      {champ.rounds.length}R
                    </Badge>
                  </div>
                  {leader && (
                    <div className="mt-2.5 pt-2 border-t border-white/[0.06] flex items-center justify-between text-[11px]">
                      <span className="text-[var(--text-secondary)]">
                        Leader: <strong className="text-white">{leader.driver}</strong>
                      </span>
                      <span className="font-mono text-[var(--accent)] tabular-nums">
                        {leader.points} pts
                      </span>
                    </div>
                  )}
                </button>
              );
            })}
          </div>

          {/* Right Detail Pane */}
          {selectedChamp && (
            <div className="flex-1 min-w-0 overflow-y-auto space-y-4 pr-1">
              {/* Series Header */}
              <Card variant="glass" padding="md" className="space-y-3">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <div className="flex items-center gap-2">
                      <Badge variant="accent">{selectedChamp.carClass}</Badge>
                      <span className="text-xs font-mono text-[var(--text-muted)]">
                        Car:{" "}
                        {cars.find((c) => c.id === selectedChamp.playerCarId)
                          ?.name ?? selectedChamp.playerCarId}
                      </span>
                    </div>
                    <h2 className="font-display text-2xl font-bold uppercase tracking-wider text-white mt-1">
                      {selectedChamp.name}
                    </h2>
                  </div>
                  <div className="flex flex-wrap items-center gap-1">
                    {selectedChamp.pointsSystem.slice(0, 6).map((pt, i) => (
                      <span
                        key={i}
                        className="px-1.5 py-0.5 rounded bg-white/[0.05] border border-white/[0.08] text-[10px] font-mono text-[var(--text-secondary)] tabular-nums"
                      >
                        P{i + 1}:{pt}
                      </span>
                    ))}
                  </div>
                </div>
              </Card>

              {/* Standings Table */}
              <Card variant="glass" padding="none" className="overflow-hidden">
                <div className="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Award className="w-4 h-4 text-amber-400" />
                    <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                      Championship Standings & Success Ballast
                    </span>
                  </div>
                  <span className="text-[11px] font-mono text-[var(--text-muted)]">
                    Top-3 BoP: +30 / +20 / +10 kg
                  </span>
                </div>
                <div className="overflow-x-auto">
                  <table className="w-full text-left text-xs">
                    <thead>
                      <tr className="border-b border-white/[0.06] text-[10px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                        <th className="py-2.5 px-4 w-16">Pos</th>
                        <th className="py-2.5 px-4">Driver</th>
                        <th className="py-2.5 px-4 text-right">Wins</th>
                        <th className="py-2.5 px-4 text-right">Ballast</th>
                        <th className="py-2.5 px-4 text-right">Points</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-white/[0.05]">
                      {(selectedChamp.standings ?? []).map((row, idx) => {
                        const ballast = successBallastKg(idx);
                        const posBadge =
                          idx === 0
                            ? "bg-amber-400/20 text-amber-300 border-amber-400/40"
                            : idx === 1
                            ? "bg-slate-300/20 text-slate-200 border-slate-300/35"
                            : idx === 2
                            ? "bg-orange-500/20 text-orange-300 border-orange-500/35"
                            : "bg-white/[0.05] text-[var(--text-muted)] border-white/[0.08]";
                        return (
                          <tr
                            key={row.driver}
                            className="hover:bg-white/[0.02] transition-colors"
                          >
                            <td className="py-2.5 px-4">
                              <span
                                className={`inline-flex items-center justify-center w-6 h-6 rounded-md border font-mono text-[11px] font-semibold ${posBadge}`}
                              >
                                {idx + 1}
                              </span>
                            </td>
                            <td className="py-2.5 px-4 font-medium text-[var(--text-primary)]">
                              {row.driver}
                            </td>
                            <td className="py-2.5 px-4 text-right font-mono tabular-nums text-[var(--text-secondary)]">
                              {row.wins}
                            </td>
                            <td className="py-2.5 px-4 text-right">
                              {ballast > 0 ? (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-amber-500/12 border border-amber-500/25 text-amber-300 font-mono text-[11px] tabular-nums">
                                  <Scale className="w-3 h-3" />+{ballast} kg
                                </span>
                              ) : (
                                <span className="font-mono text-[var(--text-muted)]">
                                  0 kg
                                </span>
                              )}
                            </td>
                            <td className="py-2.5 px-4 text-right font-mono font-semibold text-white tabular-nums">
                              {row.points}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </Card>

              {/* Playable Rounds Grid */}
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Flag className="w-4 h-4 text-[var(--accent)]" />
                    <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text-primary)]">
                      Season Calendar ({selectedChamp.rounds.length} Rounds)
                    </span>
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={
                      addingRound ? (
                        <X className="w-3.5 h-3.5" />
                      ) : (
                        <Plus className="w-3.5 h-3.5" />
                      )
                    }
                    onClick={() => setAddingRound((v) => !v)}
                  >
                    {addingRound ? "Cancel" : "Add Round"}
                  </Button>
                </div>

                {addingRound && (
                  <Card variant="elevated" padding="sm">
                    <div className="flex flex-wrap items-center gap-2">
                      <select
                        value={roundTrackId}
                        onChange={(e) => setRoundTrackId(e.target.value)}
                        className="h-8 flex-1 min-w-[180px] px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white"
                      >
                        {tracks.map((t) => (
                          <option key={t.id} value={t.id} className="bg-[#0D1118]">
                            {t.name}
                          </option>
                        ))}
                      </select>
                      <input
                        type="number"
                        min={1}
                        max={100}
                        value={roundLaps}
                        onChange={(e) => setRoundLaps(Number(e.target.value))}
                        className="h-8 w-20 px-2 rounded-lg bg-black/50 border border-white/[0.1] text-xs font-mono text-white"
                      />
                      <span className="text-[11px] text-[var(--text-muted)]">
                        laps
                      </span>
                      <select
                        value={roundWeather}
                        onChange={(e) => setRoundWeather(e.target.value)}
                        className="h-8 px-2.5 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-white"
                      >
                        <option value="3_clear" className="bg-[#0D1118]">
                          Clear
                        </option>
                        <option value="5_light_clouds" className="bg-[#0D1118]">
                          Cloudy
                        </option>
                        <option value="7_heavy_clouds" className="bg-[#0D1118]">
                          Rain
                        </option>
                      </select>
                      <Button variant="primary" size="sm" onClick={handleAddRound}>
                        Save Round
                      </Button>
                    </div>
                  </Card>
                )}

                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  {selectedChamp.rounds.map((round, idx) => {
                    const track = tracks.find((t) => t.id === round.trackId);
                    const img = assetUrl(track?.previewPath);
                    return (
                      <div
                        key={`${round.name}-${idx}`}
                        className="glass-panel rounded-xl overflow-hidden flex flex-col border border-white/[0.08]"
                      >
                        <div className="relative h-24 bg-[#0A0E16]">
                          {img ? (
                            <img
                              src={img}
                              alt={round.name}
                              className="w-full h-full object-cover opacity-75"
                            />
                          ) : (
                            <div
                              className="w-full h-full"
                              style={{
                                background:
                                  "radial-gradient(240px 100px at 85% 20%, rgba(0, 209, 255, 0.18), transparent 70%), linear-gradient(135deg, #111824 0%, #090C12 100%)",
                              }}
                            />
                          )}
                          <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-[#0D1118]/45 to-transparent" />
                          <div className="absolute top-2.5 left-3 flex items-center gap-1.5">
                            <Badge variant="neutral">R{idx + 1}</Badge>
                            <Badge variant="info">{round.laps} Laps</Badge>
                          </div>
                          {round.completed && (
                            <div className="absolute top-2.5 right-3">
                              <Badge variant="success">
                                <CheckCircle2 className="w-3 h-3" />
                                Completed
                              </Badge>
                            </div>
                          )}
                          <div className="absolute bottom-2 inset-x-3">
                            <div className="font-display text-lg font-bold uppercase tracking-wide text-white truncate">
                              {track?.name ?? round.trackId}
                            </div>
                          </div>
                        </div>

                        <div className="p-3 flex items-center justify-between gap-2">
                          <div className="text-[11px] font-mono text-[var(--text-muted)] truncate">
                            {round.completed && round.playerPosition
                              ? `Finished P${round.playerPosition} (+${
                                  round.playerPoints ?? 0
                                } pts)`
                              : `Layout: ${round.layoutId} · ${round.weatherId}`}
                          </div>
                          <Button
                            variant={round.completed ? "secondary" : "primary"}
                            size="sm"
                            loading={launchingRound === idx}
                            disabled={gameRunning}
                            icon={<Play className="w-3 h-3 fill-current" />}
                            onClick={() => handleLaunchRound(idx)}
                          >
                            Launch Round
                          </Button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default ChampionshipsPage;

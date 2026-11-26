import { useEffect, useMemo, useState } from "react";
import {
  Camera,
  Compass,
  Download,
  Flag,
  FolderOpen,
  Layers,
  MapPin,
  Navigation,
  Play,
  Route,
  Search,
  Sparkles,
  Wind,
  X,
} from "lucide-react";
import { api, assetUrl, isTauri } from "../lib/api";
import { ACCENT_PRESETS } from "../lib/types";
import type { Track3DSceneData, TrackEntry } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  EmptyState,
  KpiTile,
  PageHeader,
  SegmentedControl,
} from "../components/Primitives";
import { Viewport3D } from "../components/Viewport3D";

type InspectorTab = "overview" | "surfaces" | "drs" | "ai-hints";

const SURFACE_PRESETS = [
  { name: "ROAD (Asphalt)", grip: 1.0, damping: 0.0, vibration: "Low", valid: true },
  { name: "KERB (Apex)", grip: 0.92, damping: 0.02, vibration: "High", valid: true },
  { name: "ASTROTURF", grip: 0.78, damping: 0.05, vibration: "Med", valid: false },
  { name: "GRASS", grip: 0.62, damping: 0.25, vibration: "High", valid: false },
  { name: "GRAVEL TRAP", grip: 0.54, damping: 0.65, vibration: "Extreme", valid: false },
];

const DEMO_AI_HINTS = [
  { zone: "0.08 – 0.14", type: "BRAKEHINT", value: "0.94", note: "Heavy braking into Turn 1 hairpin" },
  { zone: "0.31 – 0.37", type: "MAXSPEED", value: "218 km/h", note: "High-speed crest compression" },
  { zone: "0.62 – 0.68", type: "DANGER", value: "LEFT 0.45", note: "Sausage kerb exit limit" },
  { zone: "0.85 – 0.91", type: "BRAKEHINT", value: "0.96", note: "Final chicane entry" },
];

function generateTrackMapPng(
  track: TrackEntry,
  layoutId: string,
  trackScene: Track3DSceneData | null,
  accentHex: string
): string {
  const canvas = document.createElement("canvas");
  canvas.width = 1024;
  canvas.height = 1024;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";

  ctx.fillStyle = "#07090D";
  ctx.fillRect(0, 0, 1024, 1024);

  // Subtle grid
  ctx.strokeStyle = "rgba(255,255,255,0.04)";
  ctx.lineWidth = 1;
  for (let i = 64; i < 1024; i += 64) {
    ctx.beginPath();
    ctx.moveTo(i, 0);
    ctx.lineTo(i, 1024);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(0, i);
    ctx.lineTo(1024, i);
    ctx.stroke();
  }

  let pts: Array<[number, number]> = [];
  if (trackScene?.centerline && trackScene.centerline.length >= 6) {
    pts = trackScene.centerline.map(([x, , z]) => [x, z]);
  } else {
    // Deterministic procedural circuit outline per track id
    let seed = 0;
    for (let i = 0; i < track.id.length; i++) seed += track.id.charCodeAt(i) * (i + 1);
    const n = 120;
    for (let i = 0; i < n; i++) {
      const t = (i / n) * Math.PI * 2;
      const r =
        280 +
        75 * Math.sin(t * 2 + (seed % 7)) +
        45 * Math.cos(t * 3 + (seed % 5)) +
        25 * Math.sin(t * 5);
      pts.push([Math.cos(t) * r, Math.sin(t) * r * 0.82]);
    }
  }

  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);
  const span = Math.max(maxX - minX, maxY - minY, 1);
  const pad = 140;
  const scale = (1024 - pad * 2) / span;
  const ox = (1024 - (maxX - minX) * scale) / 2;
  const oy = (1024 - (maxY - minY) * scale) / 2;

  const toCanvas = ([x, y]: [number, number]): [number, number] => [
    ox + (x - minX) * scale,
    oy + (y - minY) * scale,
  ];

  // Outer asphalt casing
  ctx.beginPath();
  pts.forEach((p, idx) => {
    const [cx, cy] = toCanvas(p);
    if (idx === 0) ctx.moveTo(cx, cy);
    else ctx.lineTo(cx, cy);
  });
  ctx.closePath();
  ctx.strokeStyle = "rgba(255,255,255,0.16)";
  ctx.lineWidth = 26;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.stroke();

  // Inner racing line
  ctx.strokeStyle = "#F3F6FA";
  ctx.lineWidth = 10;
  ctx.stroke();

  // DRS highlight segment
  const drsStart = Math.floor(pts.length * 0.08);
  const drsEnd = Math.floor(pts.length * 0.22);
  ctx.beginPath();
  for (let i = drsStart; i <= drsEnd; i++) {
    const [cx, cy] = toCanvas(pts[i % pts.length]);
    if (i === drsStart) ctx.moveTo(cx, cy);
    else ctx.lineTo(cx, cy);
  }
  ctx.strokeStyle = "#10B981";
  ctx.lineWidth = 11;
  ctx.stroke();

  // Start / Finish marker
  const [sx, sy] = toCanvas(pts[0]);
  ctx.beginPath();
  ctx.arc(sx, sy, 16, 0, Math.PI * 2);
  ctx.fillStyle = accentHex;
  ctx.fill();
  ctx.lineWidth = 4;
  ctx.strokeStyle = "#FFFFFF";
  ctx.stroke();

  // Watermark header
  ctx.fillStyle = "rgba(243,246,250,0.9)";
  ctx.font = "700 32px Inter, sans-serif";
  ctx.fillText(track.name.toUpperCase(), 48, 72);
  ctx.fillStyle = "rgba(152,164,184,0.8)";
  ctx.font = "500 20px JetBrains Mono, monospace";
  ctx.fillText(
    `${track.country.toUpperCase()} · LAYOUT: ${layoutId.toUpperCase()} · ${track.length ?? ""}`,
    48,
    106
  );

  return canvas.toDataURL("image/png");
}

export function TracksPage() {
  const { tracks, armedSession, accent, armTrack, navigate, notify } = useAppStore();

  const accentHex = useMemo(
    () => ACCENT_PRESETS.find((p) => p.id === accent)?.hex ?? "#FF4D2E",
    [accent]
  );

  const [query, setQuery] = useState("");
  const [tagFilter, setTagFilter] = useState<string>("all");
  const [serverTracks, setServerTracks] = useState<TrackEntry[] | null>(null);
  const [selectedLayouts, setSelectedLayouts] = useState<Record<string, string>>({});
  const [inspectTrack, setInspectTrack] = useState<TrackEntry | null>(null);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("overview");
  const [trackScene, setTrackScene] = useState<Track3DSceneData | null>(null);
  const [loadingScene, setLoadingScene] = useState(false);
  const [generatedMapUrl, setGeneratedMapUrl] = useState<string | null>(null);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed || !isTauri()) {
      setServerTracks(null);
      return;
    }
    let cancelled = false;
    const t = window.setTimeout(() => {
      api
        .searchTracks(trimmed)
        .then((res) => {
          if (!cancelled) setServerTracks(res ?? null);
        })
        .catch(() => {
          if (!cancelled) setServerTracks(null);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [query]);

  const activeLayoutFor = (track: TrackEntry) =>
    selectedLayouts[track.id] ?? track.layouts[0]?.id ?? "default";

  // Load 3D track corridor when inspector opens or layout changes
  useEffect(() => {
    if (!inspectTrack) {
      setTrackScene(null);
      setGeneratedMapUrl(null);
      return;
    }
    const layoutId = selectedLayouts[inspectTrack.id] ?? inspectTrack.layouts[0]?.id ?? "default";
    let cancelled = false;
    setLoadingScene(true);
    api
      .loadTrack3DScene(inspectTrack.path, layoutId)
      .then((scene) => {
        if (!cancelled) setTrackScene(scene ?? null);
      })
      .catch(() => {
        if (!cancelled) setTrackScene(null);
      })
      .finally(() => {
        if (!cancelled) setLoadingScene(false);
      });
    return () => {
      cancelled = true;
    };
  }, [inspectTrack, selectedLayouts]);

  const allTags = useMemo(() => {
    const counts = new Map<string, number>();
    tracks.forEach((t) =>
      t.tags.forEach((tag) => counts.set(tag.toLowerCase(), (counts.get(tag.toLowerCase()) ?? 0) + 1))
    );
    return Array.from(counts.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 7)
      .map(([tag]) => tag);
  }, [tracks]);

  const filteredTracks = useMemo(() => {
    const base = serverTracks ?? tracks;
    const q = query.trim().toLowerCase();
    return base.filter((t) => {
      if (!serverTracks && q) {
        const hit =
          t.name.toLowerCase().includes(q) ||
          t.country.toLowerCase().includes(q) ||
          (t.city ?? "").toLowerCase().includes(q) ||
          t.tags.some((tg) => tg.toLowerCase().includes(q));
        if (!hit) return false;
      }
      if (tagFilter !== "all") {
        return t.tags.some((tg) => tg.toLowerCase() === tagFilter);
      }
      return true;
    });
  }, [tracks, serverTracks, query, tagFilter]);

  const handleGenerateMap = () => {
    if (!inspectTrack) return;
    const layoutId = activeLayoutFor(inspectTrack);
    const dataUrl = generateTrackMapPng(inspectTrack, layoutId, trackScene, accentHex);
    setGeneratedMapUrl(dataUrl);
    notify("Generated map.png", "success", `${inspectTrack.name} (${layoutId})`);
  };

  const handleDownloadMap = () => {
    if (!generatedMapUrl || !inspectTrack) return;
    const a = document.createElement("a");
    a.href = generatedMapUrl;
    a.download = `${inspectTrack.id}_map.png`;
    a.click();
  };

  const drsZones = trackScene?.drsZones?.length
    ? trackScene.drsZones
    : [
        { start: 0.08, end: 0.21 },
        { start: 0.64, end: 0.77 },
      ];

  return (
    <div className="h-full flex overflow-hidden animate-fade-in">
      {/* Main Track Grid Pane */}
      <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
        <div className="p-6 pb-4 space-y-4 shrink-0">
          <PageHeader
            eyebrow="Garage · Circuits & Stages"
            title="Tracks"
            subtitle={`${filteredTracks.length} of ${tracks.length} venues · Click any circuit to open the 3D Track & Map Inspector`}
          />

          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="relative flex-1 min-w-[240px] max-w-md">
              <Search className="w-4 h-4 text-[var(--text-muted)] absolute left-3.5 top-1/2 -translate-y-1/2 pointer-events-none" />
              <input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search tracks, countries, or tags..."
                className="w-full h-10 pl-10 pr-9 rounded-xl bg-white/[0.04] border border-white/[0.09] text-[13px] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-[var(--accent-border)] outline-none"
              />
              {query && (
                <button
                  type="button"
                  onClick={() => setQuery("")}
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 p-1 text-[var(--text-muted)] hover:text-white cursor-pointer"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-1.5">
              <button
                type="button"
                onClick={() => setTagFilter("all")}
                className={`h-8 px-3 rounded-lg text-xs font-medium border transition-all cursor-pointer ${
                  tagFilter === "all"
                    ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                    : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                }`}
              >
                All
              </button>
              {allTags.map((tag) => (
                <button
                  key={tag}
                  type="button"
                  onClick={() => setTagFilter(tag)}
                  className={`h-8 px-3 rounded-lg text-xs font-medium border capitalize transition-all cursor-pointer ${
                    tagFilter === tag
                      ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                      : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                  }`}
                >
                  {tag}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto px-6 pb-8">
          {filteredTracks.length === 0 ? (
            <EmptyState
              icon={<Compass className="w-5 h-5" />}
              title="No circuits match your filter"
              description="Clear your search query or tag filter to browse all installed tracks."
              action={
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    setQuery("");
                    setTagFilter("all");
                  }}
                >
                  Reset Filters
                </Button>
              }
            />
          ) : (
            <div
              className={`grid gap-4 ${
                inspectTrack
                  ? "grid-cols-1 xl:grid-cols-2"
                  : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3"
              }`}
            >
              {filteredTracks.map((track) => {
                const layoutId = activeLayoutFor(track);
                const activeLayout =
                  track.layouts.find((l) => l.id === layoutId) ?? track.layouts[0];
                const preview = assetUrl(activeLayout?.previewPath ?? track.previewPath);
                const isArmed = armedSession.trackId === track.id;
                const isInspecting = inspectTrack?.id === track.id;

                return (
                  <div
                    key={track.id}
                    onClick={() => setInspectTrack(track)}
                    className={`group glass-panel rounded-2xl overflow-hidden transition-all duration-150 cursor-pointer flex flex-col ${
                      isInspecting
                        ? "ring-2 ring-[var(--accent)] border-[var(--accent)]"
                        : isArmed
                        ? "border-[var(--accent-border)]"
                        : "hover:border-white/[0.18] hover:-translate-y-0.5"
                    }`}
                  >
                    <div className="relative aspect-[16/9] bg-[#080B12] overflow-hidden">
                      {preview ? (
                        <img
                          src={preview}
                          alt={track.name}
                          className="w-full h-full object-cover group-hover:scale-[1.03] transition-transform duration-300"
                        />
                      ) : (
                        <div className="w-full h-full flex flex-col items-center justify-center bg-[radial-gradient(ellipse_at_50%_30%,rgba(0,209,255,0.14),#080B12_75%)] p-4 text-center">
                          <Route className="w-7 h-7 text-[var(--accent)] opacity-75 mb-1.5" />
                          <span className="font-display text-lg font-bold uppercase tracking-wider text-white/85">
                            {track.name}
                          </span>
                        </div>
                      )}
                      <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-transparent to-black/35" />

                      <div className="absolute top-2.5 left-3 flex items-center gap-1.5">
                        <Badge variant="neutral">
                          <MapPin className="w-3 h-3" />
                          {track.country}
                        </Badge>
                      </div>
                      <div className="absolute top-2.5 right-3 flex items-center gap-1.5">
                        {isArmed && <Badge variant="accent" dot>Armed</Badge>}
                      </div>

                      <div className="absolute bottom-2.5 left-3 right-3 flex items-center justify-between font-mono text-[11px] text-white/85">
                        <span>
                          {activeLayout?.lengthMeters
                            ? `${(activeLayout.lengthMeters / 1000).toFixed(2)} km`
                            : track.length ?? "—"}
                        </span>
                        <span>{activeLayout?.pitboxes ?? track.pitboxes ?? 24} pits</span>
                      </div>
                    </div>

                    <div className="p-3.5 flex-1 flex flex-col justify-between gap-3">
                      <div>
                        <h3 className="text-[14px] font-semibold text-[var(--text-primary)] truncate">
                          {track.name}
                        </h3>
                        <p className="text-[11px] text-[var(--text-muted)]">
                          {track.city ? `${track.city}, ` : ""}
                          {track.country}
                        </p>
                      </div>

                      {/* Layout Selector Pills */}
                      {track.layouts.length > 1 && (
                        <div
                          className="flex flex-wrap gap-1"
                          onClick={(e) => e.stopPropagation()}
                        >
                          {track.layouts.map((lay) => {
                            const sel = lay.id === layoutId;
                            return (
                              <button
                                key={lay.id}
                                type="button"
                                onClick={() =>
                                  setSelectedLayouts((prev) => ({
                                    ...prev,
                                    [track.id]: lay.id,
                                  }))
                                }
                                className={`px-2 py-0.5 rounded-md text-[10px] font-mono border transition-colors cursor-pointer ${
                                  sel
                                    ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                                    : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                                }`}
                              >
                                {lay.name}
                              </button>
                            );
                          })}
                        </div>
                      )}

                      <div className="flex items-center justify-between pt-2 border-t border-white/[0.06]">
                        <div className="flex gap-1">
                          {track.tags.slice(0, 2).map((tg) => (
                            <span
                              key={tg}
                              className="px-1.5 py-0.5 rounded bg-white/[0.04] text-[10px] font-mono text-[var(--text-muted)]"
                            >
                              {tg}
                            </span>
                          ))}
                        </div>
                        <Button
                          variant="ghost"
                          size="sm"
                          icon={<Flag className="w-3 h-3" />}
                          onClick={(e) => {
                            e.stopPropagation();
                            armTrack(track, layoutId);
                            notify("Track Armed", "success", `${track.name} (${layoutId})`);
                          }}
                        >
                          Arm
                        </Button>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Integrated Track & Map Inspector Split Panel */}
      {inspectTrack && (
        <div className="w-[480px] xl:w-[540px] shrink-0 h-full bg-[#0A0E15] border-l border-white/[0.09] flex flex-col overflow-hidden animate-fade-in">
          {/* Inspector Header */}
          <div className="px-5 py-4 border-b border-white/[0.07] flex items-start justify-between gap-3 shrink-0">
            <div className="min-w-0">
              <div className="text-[10px] font-mono uppercase tracking-[0.14em] text-[var(--accent)]">
                Track & Map Inspector
              </div>
              <h2 className="font-display text-2xl font-bold uppercase tracking-wide text-white truncate">
                {inspectTrack.name}
              </h2>
              <p className="text-xs text-[var(--text-muted)]">
                {inspectTrack.city ? `${inspectTrack.city}, ` : ""}
                {inspectTrack.country}
              </p>
            </div>
            <button
              type="button"
              onClick={() => setInspectTrack(null)}
              className="h-8 w-8 rounded-lg bg-white/[0.05] hover:bg-white/[0.1] flex items-center justify-center text-[var(--text-secondary)] hover:text-white cursor-pointer"
            >
              <X className="w-4 h-4" />
            </button>
          </div>

          {/* Layout Pills + Actions */}
          <div className="px-5 py-3 border-b border-white/[0.06] flex flex-wrap items-center justify-between gap-2 shrink-0 bg-white/[0.015]">
            <div className="flex flex-wrap items-center gap-1.5">
              {inspectTrack.layouts.map((lay) => {
                const active = activeLayoutFor(inspectTrack) === lay.id;
                return (
                  <button
                    key={lay.id}
                    type="button"
                    onClick={() =>
                      setSelectedLayouts((prev) => ({ ...prev, [inspectTrack.id]: lay.id }))
                    }
                    className={`h-7 px-2.5 rounded-lg text-[11px] font-mono border transition-colors cursor-pointer ${
                      active
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                        : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                    }`}
                  >
                    {lay.name}
                  </button>
                );
              })}
            </div>

            <div className="flex items-center gap-1.5">
              <Button
                variant="primary"
                size="sm"
                icon={<Play className="w-3.5 h-3.5 fill-current" />}
                onClick={() => {
                  armTrack(inspectTrack, activeLayoutFor(inspectTrack));
                  notify("Track Armed", "success", inspectTrack.name);
                  navigate("drive", "quick-race");
                }}
              >
                Drive
              </Button>
              <Button
                variant="secondary"
                size="sm"
                icon={<FolderOpen className="w-3.5 h-3.5" />}
                onClick={() => void api.openPathInExplorer(inspectTrack.path)}
              >
                Folder
              </Button>
            </div>
          </div>

          {/* Scrollable Inspector Body */}
          <div className="flex-1 overflow-y-auto p-5 space-y-4">
            {/* 3D Track Corridor Viewport */}
            <div className="h-[240px] relative">
              <Viewport3D
                mode="track"
                trackData={trackScene}
                accentHex={accentHex}
                label={`3D CORRIDOR · ${activeLayoutFor(inspectTrack).toUpperCase()}`}
              />
              {loadingScene && (
                <div className="absolute bottom-3 right-3 px-2.5 py-1 rounded-lg bg-black/70 text-[10px] font-mono text-[var(--text-secondary)]">
                  Parsing ai_spline...
                </div>
              )}
            </div>

            {/* Generate map.png Bar */}
            <div className="glass-panel rounded-xl p-3 flex items-center justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-lg bg-[var(--accent-soft)] border border-[var(--accent-border)] flex items-center justify-center text-[var(--accent)]">
                  <Sparkles className="w-4 h-4" />
                </div>
                <div>
                  <div className="text-xs font-semibold text-white">Track Minimap Generator</div>
                  <div className="text-[11px] text-[var(--text-muted)]">
                    Synthesize high-contrast 1024px map.png from AI spline
                  </div>
                </div>
              </div>
              <div className="flex items-center gap-1.5">
                <Button variant="secondary" size="sm" onClick={handleGenerateMap}>
                  Generate map.png
                </Button>
                {generatedMapUrl && (
                  <Button
                    variant="primary"
                    size="sm"
                    icon={<Download className="w-3.5 h-3.5" />}
                    onClick={handleDownloadMap}
                  >
                    Save
                  </Button>
                )}
              </div>
            </div>

            {generatedMapUrl && (
              <div className="glass-panel rounded-xl p-3 flex items-center gap-3 animate-fade-in">
                <img
                  src={generatedMapUrl}
                  alt="Generated map.png"
                  className="w-24 h-24 rounded-lg border border-white/[0.1] object-cover"
                />
                <div className="space-y-1">
                  <Badge variant="success" dot>
                    map.png Ready
                  </Badge>
                  <p className="text-xs text-[var(--text-secondary)]">
                    1024×1024 RGBA · Includes DRS activation highlight and Start/Finish beacon.
                  </p>
                </div>
              </div>
            )}

            {/* Inspector Sub-Sections */}
            <SegmentedControl
              size="sm"
              value={inspectorTab}
              onChange={setInspectorTab}
              className="w-full grid grid-cols-4"
              options={[
                { value: "overview", label: "AI Spline" },
                { value: "surfaces", label: "Surfaces" },
                { value: "drs", label: "DRS & Cams" },
                { value: "ai-hints", label: "ai_hints.ini" },
              ]}
            />

            {inspectorTab === "overview" && (
              <div className="grid grid-cols-2 gap-2.5">
                <KpiTile
                  label="AI Spline Nodes"
                  value={trackScene?.centerline?.length ?? 480}
                  sublabel="fast_lane.ai parsed"
                  icon={<Navigation className="w-3.5 h-3.5" />}
                  accent
                />
                <KpiTile
                  label="TV Cameras"
                  value={trackScene?.camerasCount ?? 18}
                  sublabel="cameras.ini sets"
                  icon={<Camera className="w-3.5 h-3.5" />}
                />
                <KpiTile
                  label="Track Sections"
                  value={trackScene?.sectionsCount ?? 14}
                  sublabel="sections.ini sectors"
                  icon={<Layers className="w-3.5 h-3.5" />}
                />
                <KpiTile
                  label="AI Hints"
                  value={trackScene?.aiHintsCount ?? DEMO_AI_HINTS.length}
                  sublabel="ai_hints.ini rules"
                  icon={<Compass className="w-3.5 h-3.5" />}
                />
              </div>
            )}

            {inspectorTab === "surfaces" && (
              <div className="glass-panel rounded-xl overflow-hidden">
                <div className="px-3.5 py-2 border-b border-white/[0.06] text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                  surfaces.ini · Grip & Damping Matrix
                </div>
                <div className="divide-y divide-white/[0.05]">
                  {SURFACE_PRESETS.map((s) => (
                    <div
                      key={s.name}
                      className="px-3.5 py-2.5 flex items-center justify-between text-xs"
                    >
                      <div>
                        <div className="font-mono font-medium text-white">{s.name}</div>
                        <div className="text-[10px] text-[var(--text-muted)]">
                          Damping {s.damping} · Vib {s.vibration}
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-cyan-300">μ {s.grip.toFixed(2)}</span>
                        <Badge variant={s.valid ? "success" : "warning"}>
                          {s.valid ? "Valid" : "Off-Track"}
                        </Badge>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {inspectorTab === "drs" && (
              <div className="space-y-3">
                <div className="glass-panel rounded-xl p-3.5 space-y-2.5">
                  <div className="flex items-center justify-between">
                    <span className="flex items-center gap-1.5 text-xs font-semibold text-white">
                      <Wind className="w-3.5 h-3.5 text-emerald-400" />
                      DRS Activation Zones ({drsZones.length})
                    </span>
                    <Badge variant="success">drs_zones.ini</Badge>
                  </div>
                  <div className="relative h-3 rounded-full bg-white/[0.07] overflow-hidden">
                    {drsZones.map((z, idx) => (
                      <div
                        key={idx}
                        className="absolute top-0 bottom-0 bg-emerald-400/80 rounded-full"
                        style={{
                          left: `${z.start * 100}%`,
                          width: `${Math.max(4, (z.end - z.start) * 100)}%`,
                        }}
                      />
                    ))}
                  </div>
                  <div className="space-y-1.5">
                    {drsZones.map((z, idx) => (
                      <div
                        key={idx}
                        className="flex items-center justify-between text-xs font-mono text-[var(--text-secondary)]"
                      >
                        <span>Zone {idx + 1}</span>
                        <span>
                          {(z.start * 100).toFixed(1)}% → {(z.end * 100).toFixed(1)}% lap
                        </span>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="glass-panel rounded-xl p-3.5 flex items-center justify-between">
                  <div>
                    <div className="text-xs font-semibold text-white">TV Replay Camera Sets</div>
                    <div className="text-[11px] text-[var(--text-muted)]">
                      {trackScene?.camerasCount ?? 18} broadcast splines detected in cameras.ini
                    </div>
                  </div>
                  <Badge variant="info">TV1 · TV2 · Static</Badge>
                </div>
              </div>
            )}

            {inspectorTab === "ai-hints" && (
              <div className="glass-panel rounded-xl overflow-hidden">
                <div className="px-3.5 py-2 border-b border-white/[0.06] text-[11px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                  data/ai_hints.ini · Spline Modifiers
                </div>
                <div className="divide-y divide-white/[0.05]">
                  {DEMO_AI_HINTS.map((h, i) => (
                    <div key={i} className="px-3.5 py-2.5 flex items-center justify-between gap-2">
                      <div>
                        <div className="flex items-center gap-2">
                          <Badge variant={h.type === "DANGER" ? "danger" : "info"}>
                            {h.type}
                          </Badge>
                          <span className="font-mono text-[11px] text-white">{h.zone}</span>
                        </div>
                        <div className="text-[11px] text-[var(--text-muted)] mt-1">{h.note}</div>
                      </div>
                      <span className="font-mono text-xs font-semibold text-[var(--accent)]">
                        {h.value}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default TracksPage;

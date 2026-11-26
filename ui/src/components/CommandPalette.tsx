import React, { useEffect, useMemo, useRef, useState } from "react";
import {
  Car,
  Compass,
  MapPin,
  Palette,
  Play,
  RefreshCw,
  Search,
  CornerDownLeft,
} from "lucide-react";
import { ACCENT_PRESETS } from "../lib/types";
import { PILLAR_NAV, useAppStore } from "../store/appStore";

interface PaletteItem {
  id: string;
  group: "Pages" | "Cars" | "Tracks" | "Actions";
  title: string;
  subtitle?: string;
  keywords: string;
  icon: React.ReactNode;
  run: () => void;
}

export function CommandPalette() {
  const {
    commandPaletteOpen,
    setCommandPaletteOpen,
    cars,
    tracks,
    navigate,
    armCar,
    armTrack,
    rescan,
    launchArmed,
    setAccent,
    notify,
  } = useAppStore();

  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (commandPaletteOpen) {
      setQuery("");
      setSelectedIndex(0);
      window.setTimeout(() => inputRef.current?.focus(), 10);
    }
  }, [commandPaletteOpen]);

  const allItems = useMemo<PaletteItem[]>(() => {
    const list: PaletteItem[] = [];

    // Pages (all 6 pillars + sub-tabs)
    for (const p of PILLAR_NAV) {
      if (p.subTabs.length === 0) {
        list.push({
          id: `page-${p.id}`,
          group: "Pages",
          title: p.label,
          subtitle: "Pillar Home",
          keywords: `${p.label} ${p.id} home`,
          icon: <Compass className="w-4 h-4 text-[var(--accent)]" />,
          run: () => navigate(p.id),
        });
      } else {
        for (const sub of p.subTabs) {
          list.push({
            id: `page-${p.id}-${sub.id}`,
            group: "Pages",
            title: `${p.label} · ${sub.label}`,
            subtitle: `Go to ${sub.label}`,
            keywords: `${p.label} ${sub.label} ${p.id} ${sub.id}`,
            icon: <Compass className="w-4 h-4 text-[var(--accent)]" />,
            run: () => navigate(p.id, sub.id),
          });
        }
      }
    }

    // Actions
    list.push({
      id: "action-drive",
      group: "Actions",
      title: "Launch Armed Session (DRIVE)",
      subtitle: "Start Assetto Corsa with current car & track",
      keywords: "drive launch play start race",
      icon: <Play className="w-4 h-4 text-emerald-400" />,
      run: () => void launchArmed(),
    });
    list.push({
      id: "action-rescan",
      group: "Actions",
      title: "Rescan Assetto Corsa Content",
      subtitle: "Re-index cars, tracks, CSP & driving stats",
      keywords: "rescan refresh scan content reload",
      icon: <RefreshCw className="w-4 h-4 text-cyan-400" />,
      run: () => void rescan(false),
    });

    for (const preset of ACCENT_PRESETS) {
      list.push({
        id: `action-accent-${preset.id}`,
        group: "Actions",
        title: `Theme: ${preset.label}`,
        subtitle: `Switch accent preset to ${preset.hex}`,
        keywords: `theme accent color ${preset.label} ${preset.id}`,
        icon: (
          <Palette className="w-4 h-4" style={{ color: preset.hex }} />
        ),
        run: () => {
          setAccent(preset.id);
          notify(`Accent set to ${preset.label}`, "info");
        },
      });
    }

    // Cars (cap to keep instant)
    for (const car of cars.slice(0, 60)) {
      list.push({
        id: `car-${car.id}`,
        group: "Cars",
        title: car.name,
        subtitle: [car.brand, car.bhp, car.class].filter(Boolean).join(" · "),
        keywords: `${car.name} ${car.brand} ${car.id} ${car.tags.join(" ")}`,
        icon: <Car className="w-4 h-4 text-[var(--text-secondary)]" />,
        run: () => {
          armCar(car);
          navigate("garage", "showroom");
          notify(`Armed ${car.name}`, "info");
        },
      });
    }

    // Tracks
    for (const track of tracks.slice(0, 60)) {
      list.push({
        id: `track-${track.id}`,
        group: "Tracks",
        title: track.name,
        subtitle: [track.country, track.length].filter(Boolean).join(" · "),
        keywords: `${track.name} ${track.country} ${track.id} ${track.tags.join(" ")}`,
        icon: <MapPin className="w-4 h-4 text-[var(--text-secondary)]" />,
        run: () => {
          armTrack(track);
          navigate("drive", "quick-race");
          notify(`Armed ${track.name}`, "info");
        },
      });
    }

    return list;
  }, [cars, tracks, navigate, armCar, armTrack, rescan, launchArmed, setAccent, notify]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      // Show pages & actions first, plus top 4 cars & tracks
      const pages = allItems.filter((i) => i.group === "Pages").slice(0, 8);
      const actions = allItems.filter((i) => i.group === "Actions").slice(0, 4);
      const topCars = allItems.filter((i) => i.group === "Cars").slice(0, 4);
      const topTracks = allItems.filter((i) => i.group === "Tracks").slice(0, 4);
      return [...pages, ...actions, ...topCars, ...topTracks];
    }
    const tokens = q.split(/\s+/).filter(Boolean);
    return allItems
      .filter((item) => {
        const hay = `${item.title} ${item.subtitle ?? ""} ${item.keywords}`.toLowerCase();
        return tokens.every((tok) => hay.includes(tok));
      })
      .slice(0, 30);
  }, [allItems, query]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  if (!commandPaletteOpen) return null;

  const execute = (item?: PaletteItem) => {
    if (!item) return;
    setCommandPaletteOpen(false);
    item.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(filtered.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      execute(filtered[selectedIndex]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setCommandPaletteOpen(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Command Palette"
      className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] px-4 bg-black/65 backdrop-blur-sm animate-fade-in"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) setCommandPaletteOpen(false);
      }}
    >
      <div
        className="w-full max-w-[600px] glass-panel rounded-2xl border border-white/[0.13] shadow-[0_28px_80px_-16px_rgba(0,0,0,0.85)] overflow-hidden animate-scale-in"
        onKeyDown={onKeyDown}
      >
        {/* Search Input */}
        <div className="flex items-center gap-3 px-4 h-13 border-b border-white/[0.08]">
          <Search className="w-4 h-4 text-[var(--accent)] shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Jump to page, car, circuit, or action..."
            className="flex-1 bg-transparent text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none"
          />
          <kbd className="font-mono text-[10px] px-1.5 py-0.5 rounded bg-white/[0.06] border border-white/[0.09] text-[var(--text-muted)]">
            ESC
          </kbd>
        </div>

        {/* Results List */}
        <div className="max-h-[420px] overflow-y-auto p-2 space-y-0.5">
          {filtered.length === 0 ? (
            <div className="py-10 text-center text-xs text-[var(--text-muted)]">
              No matching pages, cars, circuits, or actions.
            </div>
          ) : (
            filtered.map((item, idx) => {
              const active = idx === selectedIndex;
              return (
                <button
                  key={item.id}
                  type="button"
                  onMouseEnter={() => setSelectedIndex(idx)}
                  onClick={() => execute(item)}
                  className={`w-full flex items-center justify-between gap-3 px-3 py-2.5 rounded-xl text-left transition-colors cursor-pointer ${
                    active
                      ? "bg-white/[0.09] text-[var(--text-primary)]"
                      : "text-[var(--text-secondary)] hover:bg-white/[0.04]"
                  }`}
                >
                  <div className="flex items-center gap-3 min-w-0">
                    <div className="w-7 h-7 rounded-lg bg-white/[0.04] border border-white/[0.07] flex items-center justify-center shrink-0">
                      {item.icon}
                    </div>
                    <div className="min-w-0">
                      <div className="text-[13px] font-medium text-[var(--text-primary)] truncate">
                        {item.title}
                      </div>
                      {item.subtitle && (
                        <div className="text-[11px] text-[var(--text-muted)] truncate">
                          {item.subtitle}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <span className="text-[10px] font-mono uppercase tracking-wider text-[var(--text-muted)]">
                      {item.group}
                    </span>
                    {active && (
                      <CornerDownLeft className="w-3.5 h-3.5 text-[var(--accent)]" />
                    )}
                  </div>
                </button>
              );
            })
          )}
        </div>

        {/* Footer Hint */}
        <div className="px-4 py-2 border-t border-white/[0.06] bg-black/25 flex items-center justify-between text-[11px] text-[var(--text-muted)] font-mono">
          <span>↑↓ Navigate · ↵ Select</span>
          <span>ACXS Command</span>
        </div>
      </div>
    </div>
  );
}

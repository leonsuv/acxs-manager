import { useEffect, useRef } from "react";
import {
  Check,
  Palette,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Sparkles,
} from "lucide-react";
import { ACCENT_PRESETS } from "../lib/types";
import { PILLAR_NAV, useAppStore } from "../store/appStore";
import { Toggle } from "./Primitives";

export function TopBar() {
  const {
    pillar,
    navigate,
    gameRunning,
    cspBuild,
    scanning,
    rescan,
    accent,
    setAccent,
    settings,
    updateSettings,
    setCommandPaletteOpen,
    quickSwitchesOpen,
    setQuickSwitchesOpen,
  } = useAppStore();

  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!quickSwitchesOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setQuickSwitchesOpen(false);
      }
    };
    window.addEventListener("mousedown", onPointerDown);
    return () => window.removeEventListener("mousedown", onPointerDown);
  }, [quickSwitchesOpen, setQuickSwitchesOpen]);

  return (
    <header className="shrink-0 z-30 select-none">
      {/* Primary 56px Glass Top Bar — Brand + Left-Aligned Pillars + Right Utility Cluster */}
      <div className="h-14 px-4 glass-bar border-b flex items-center justify-between gap-4">
        <div className="flex items-center h-full gap-4 min-w-0">
          {/* Glowing A Motorsport Mark + ACXS */}
          <button
            type="button"
            onClick={() => navigate("paddock")}
            className="flex items-center gap-2.5 group cursor-pointer shrink-0"
          >
            <div
              className="w-8 h-8 rounded-lg flex items-center justify-center relative overflow-hidden border border-white/15 transition-transform duration-150 group-hover:scale-[1.04]"
              style={{
                background:
                  "linear-gradient(135deg, var(--accent) 0%, rgba(var(--accent-rgb), 0.65) 100%)",
                boxShadow: "0 0 18px var(--accent-glow)",
              }}
            >
              <span className="font-display text-xl font-bold text-white leading-none tracking-tight">
                A
              </span>
            </div>
            <div className="flex flex-col items-start leading-none">
              <span className="font-display text-[19px] font-bold tracking-[0.14em] text-[var(--text-primary)]">
                ACXS
              </span>
              <span className="text-[9px] font-mono uppercase tracking-[0.18em] text-[var(--text-muted)]">
                MANAGER
              </span>
            </div>
          </button>

          <div className="h-5 w-px bg-white/[0.08] shrink-0" />

          {/* Left-Aligned Primary Pillars — Visually Grounded Above the Left Rail */}
          <nav aria-label="Primary pillars" className="flex items-center h-full gap-0.5">
            {PILLAR_NAV.map((item) => {
              const active = item.id === pillar;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => navigate(item.id)}
                  className={`relative h-full px-3.5 flex items-center font-display text-[15px] font-bold uppercase tracking-[0.12em] transition-colors duration-150 cursor-pointer ${
                    active
                      ? "text-[var(--text-primary)]"
                      : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                  }`}
                >
                  <span>{item.label}</span>
                  {active && (
                    <span
                      className="absolute inset-x-2.5 bottom-0 h-[2px] rounded-full"
                      style={{
                        background: "var(--accent)",
                        boxShadow: "0 0 12px var(--accent-glow), 0 -1px 8px var(--accent-glow)",
                      }}
                    />
                  )}
                </button>
              );
            })}
          </nav>
        </div>

        {/* Right: Status Pills, Cmd+K, Rescan, Quick Switches & Accent Popover */}
        <div className="flex items-center gap-2 shrink-0">
          {/* IDLE / IN GAME pill */}
          <div
            className={`h-7 px-2.5 rounded-full border flex items-center gap-1.5 text-[11px] font-mono tracking-wider ${
              gameRunning
                ? "bg-emerald-500/15 border-emerald-500/35 text-emerald-300"
                : "bg-white/[0.04] border-white/[0.08] text-[var(--text-secondary)]"
            }`}
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                gameRunning
                  ? "bg-emerald-400 animate-pulse"
                  : "bg-slate-500"
              }`}
            />
            <span>{gameRunning ? "IN GAME" : "IDLE"}</span>
          </div>

          {/* CSP build pill */}
          <button
            type="button"
            onClick={() => navigate("setup", "csp")}
            title="Custom Shaders Patch status"
            className="hidden xl:flex h-7 px-2.5 rounded-full border bg-white/[0.04] border-white/[0.08] hover:border-white/[0.16] items-center gap-1.5 text-[11px] font-mono text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors cursor-pointer"
          >
            <Sparkles className="w-3 h-3 text-[var(--accent)]" />
            <span>{cspBuild ? `CSP ${cspBuild}` : "CSP N/A"}</span>
          </button>

          {/* Cmd+K Search Pill */}
          <button
            type="button"
            onClick={() => setCommandPaletteOpen(true)}
            className="h-8 px-2.5 rounded-lg bg-white/[0.04] hover:bg-white/[0.08] border border-white/[0.08] hover:border-white/[0.15] flex items-center gap-2 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-all cursor-pointer"
          >
            <Search className="w-3.5 h-3.5" />
            <span className="hidden lg:inline">Search</span>
            <kbd className="font-mono text-[10px] px-1.5 py-0.5 rounded bg-black/40 border border-white/[0.1] text-[var(--text-muted)]">
              ⌘K
            </kbd>
          </button>

          {/* Rescan Button */}
          <button
            type="button"
            onClick={() => void rescan(false)}
            disabled={scanning}
            title="Rescan Assetto Corsa content"
            aria-label="Rescan content"
            className="w-8 h-8 rounded-lg bg-white/[0.04] hover:bg-white/[0.09] border border-white/[0.08] flex items-center justify-center text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-all cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${scanning ? "animate-spin" : ""}`} />
          </button>

          {/* Quick Switches + Accent Theme Popover */}
          <div className="relative" ref={popoverRef}>
            <button
              type="button"
              onClick={() => setQuickSwitchesOpen(!quickSwitchesOpen)}
              title="Quick Switches & Accent Theme"
              aria-label="Quick Switches and Accent Theme"
              className={`w-8 h-8 rounded-lg border flex items-center justify-center transition-all cursor-pointer ${
                quickSwitchesOpen
                  ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent)]"
                  : "bg-white/[0.04] hover:bg-white/[0.09] border-white/[0.08] text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              }`}
            >
              <SlidersHorizontal className="w-3.5 h-3.5" />
            </button>

            {quickSwitchesOpen && (
              <div className="absolute right-0 mt-2 w-72 glass-panel rounded-xl p-4 shadow-2xl border border-white/[0.12] animate-scale-in z-50 space-y-4">
                {/* Accent Theme Section */}
                <div>
                  <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)] mb-2.5">
                    <Palette className="w-3.5 h-3.5 text-[var(--accent)]" />
                    <span>Accent Theme</span>
                  </div>
                  <div className="grid grid-cols-2 gap-1.5">
                    {ACCENT_PRESETS.map((preset) => {
                      const active = preset.id === accent;
                      return (
                        <button
                          key={preset.id}
                          type="button"
                          onClick={() => setAccent(preset.id)}
                          className={`flex items-center justify-between gap-2 px-2.5 py-2 rounded-lg border text-xs font-medium transition-all cursor-pointer ${
                            active
                              ? "bg-white/[0.09] border-white/[0.2] text-[var(--text-primary)]"
                              : "bg-white/[0.02] border-white/[0.06] text-[var(--text-secondary)] hover:bg-white/[0.06]"
                          }`}
                        >
                          <span className="flex items-center gap-2 truncate">
                            <span
                              className="w-2.5 h-2.5 rounded-full shrink-0"
                              style={{
                                backgroundColor: preset.hex,
                                boxShadow: active ? `0 0 8px ${preset.hex}` : undefined,
                              }}
                            />
                            <span className="truncate">{preset.label.split(" ")[0]}</span>
                          </span>
                          {active && <Check className="w-3 h-3 text-[var(--accent)] shrink-0" />}
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className="h-px bg-white/[0.07]" />

                {/* Quick Switches */}
                <div className="space-y-3">
                  <div className="text-[11px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                    Quick Switches
                  </div>
                  <Toggle
                    label="Steam Launch"
                    description="Start AC via Steam protocol"
                    checked={settings.steamLaunch}
                    onChange={(v) => void updateSettings({ steamLaunch: v })}
                  />
                  <Toggle
                    label="Close on Launch"
                    description="Minimize manager when driving"
                    checked={settings.closeOnLaunch}
                    onChange={(v) => void updateSettings({ closeOnLaunch: v })}
                  />
                  <Toggle
                    label="CSP Auto-Sync"
                    description="Sync shader configs on scan"
                    checked={settings.cspAutoSync}
                    onChange={(v) => void updateSettings({ cspAutoSync: v })}
                  />
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </header>
  );
}

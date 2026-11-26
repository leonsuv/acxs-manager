import { Car, Flag, MapPin, Play } from "lucide-react";
import { assetUrl } from "../lib/api";
import { useAppStore } from "../store/appStore";
import { Button } from "./Primitives";

const MODE_LABELS: Record<string, string> = {
  Practice: "PRACTICE",
  QuickRace: "QUICK RACE",
  Hotlap: "HOTLAP",
  TimeAttack: "TIME ATTACK",
  Drift: "DRIFT",
  TrackDay: "TRACK DAY",
  Drag: "DRAG",
  Weekend: "WEEKEND",
};

export function BottomDock() {
  const { armedSession, navigate, launchArmed, launching, gameRunning } =
    useAppStore();

  const carImg = assetUrl(armedSession.carPreviewPath);
  const trackImg = assetUrl(armedSession.trackPreviewPath);
  const modeLabel = MODE_LABELS[armedSession.mode] ?? armedSession.mode.toUpperCase();

  return (
    <footer className="h-16 shrink-0 px-4 glass-bar border-t flex items-center justify-between gap-4 z-30 select-none">
      {/* Left: Armed Car & Track Cards */}
      <div className="flex items-center gap-3 min-w-0 flex-1">
        {/* Armed Car Card */}
        <button
          type="button"
          onClick={() => navigate("garage", "cars")}
          className="group flex items-center gap-3 h-11 pl-1.5 pr-3.5 rounded-xl bg-white/[0.03] hover:bg-white/[0.07] border border-white/[0.07] hover:border-white/[0.15] transition-all cursor-pointer min-w-0 max-w-[290px]"
        >
          <div className="w-14 h-8 rounded-lg overflow-hidden shrink-0 border border-white/[0.08] bg-gradient-to-br from-white/[0.08] via-[#141B27] to-black flex items-center justify-center relative">
            {carImg ? (
              <img
                src={carImg}
                alt={armedSession.carName}
                className="w-full h-full object-cover"
              />
            ) : (
              <Car className="w-4 h-4 text-[var(--accent)] opacity-85" />
            )}
          </div>
          <div className="flex flex-col items-start min-w-0">
            <span className="text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)] leading-none">
              Car
            </span>
            <span className="text-[13px] font-semibold text-[var(--text-primary)] truncate max-w-[195px] leading-tight mt-0.5 group-hover:text-white">
              {armedSession.carName || "Select Car"}
            </span>
          </div>
        </button>

        {/* Armed Track Card */}
        <button
          type="button"
          onClick={() => navigate("garage", "tracks")}
          className="group flex items-center gap-3 h-11 pl-1.5 pr-3.5 rounded-xl bg-white/[0.03] hover:bg-white/[0.07] border border-white/[0.07] hover:border-white/[0.15] transition-all cursor-pointer min-w-0 max-w-[290px]"
        >
          <div className="w-14 h-8 rounded-lg overflow-hidden shrink-0 border border-white/[0.08] bg-gradient-to-br from-cyan-500/15 via-[#111926] to-black flex items-center justify-center relative">
            {trackImg ? (
              <img
                src={trackImg}
                alt={armedSession.trackName}
                className="w-full h-full object-cover"
              />
            ) : (
              <MapPin className="w-4 h-4 text-cyan-400 opacity-85" />
            )}
          </div>
          <div className="flex flex-col items-start min-w-0">
            <span className="text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)] leading-none">
              Circuit
            </span>
            <span className="text-[13px] font-semibold text-[var(--text-primary)] truncate max-w-[195px] leading-tight mt-0.5 group-hover:text-white">
              {armedSession.trackName || "Select Track"}
            </span>
          </div>
        </button>

        {/* Crisp Rectangular Mode Badge */}
        <button
          type="button"
          onClick={() => navigate("drive", "quick-race")}
          title="Change session mode"
          className="hidden md:flex items-center gap-1.5 h-8 px-3 rounded-md bg-white/[0.05] hover:bg-white/[0.09] border border-white/[0.1] text-[11px] font-mono font-medium tracking-[0.12em] text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors cursor-pointer shrink-0"
        >
          <Flag className="w-3 h-3 text-[var(--accent)]" />
          <span>{modeLabel}</span>
        </button>
      </div>

      {/* Right: Bold Glowing DRIVE Primary Button */}
      <div className="flex items-center gap-3 shrink-0">
        <Button
          variant="primary"
          size="lg"
          loading={launching}
          disabled={gameRunning}
          onClick={() => void launchArmed()}
          icon={<Play className="w-4 h-4 fill-current" />}
          className="h-11 px-7 font-display text-lg tracking-[0.14em] uppercase rounded-xl"
        >
          {gameRunning ? "IN SESSION" : "DRIVE"}
        </Button>
      </div>
    </footer>
  );
}

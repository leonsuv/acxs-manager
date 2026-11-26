import { Component, Suspense, lazy, type ReactNode } from "react";
import {
  Activity,
  Archive,
  Award,
  Box,
  Brush,
  Car,
  CloudRain,
  CreditCard,
  Film,
  Flag,
  Flame,
  Fuel,
  Gauge,
  Globe,
  Layers,
  Map as MapIcon,
  Package,
  Palette,
  Route,
  Server,
  Settings,
  ShieldAlert,
  Sliders,
  Sparkles,
  Trophy,
  Volume2,
  Wrench,
} from "lucide-react";
import { AppProvider, PILLAR_NAV, useAppStore } from "./store/appStore";
import { TopBar } from "./components/TopBar";
import { BottomDock } from "./components/BottomDock";
import { CommandPalette } from "./components/CommandPalette";
import { ToastContainer } from "./components/ToastContainer";
import { Button, Card, Skeleton } from "./components/Primitives";
import type { SubTab } from "./lib/types";

interface ErrorBoundaryProps {
  resetKey: string;
  children: ReactNode;
}
interface ErrorBoundaryState {
  error: Error | null;
}

class PageErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps) {
    if (prevProps.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null });
    }
  }

  render() {
    if (this.state.error) {
      return (
        <div className="p-8 max-w-2xl mx-auto">
          <Card variant="elevated" className="p-6 space-y-4 border-red-500/30">
            <div className="text-xs font-mono uppercase tracking-widest text-red-400">
              View Error
            </div>
            <h2 className="font-display text-2xl font-bold tracking-wide">
              Something failed while rendering this page
            </h2>
            <pre className="p-3 rounded-lg bg-black/40 border border-white/10 text-xs font-mono text-red-300 overflow-x-auto whitespace-pre-wrap">
              {this.state.error.message}
            </pre>
            <Button onClick={() => this.setState({ error: null })}>Retry</Button>
          </Card>
        </div>
      );
    }
    return this.props.children;
  }
}

const PaddockPage = lazy(() => import("./pages/PaddockPage"));
const DrivePage = lazy(() => import("./pages/DrivePage"));
const EventsPage = lazy(() => import("./pages/EventsPage"));
const ChampionshipsPage = lazy(() => import("./pages/ChampionshipsPage"));
const ServersPage = lazy(() => import("./pages/ServersPage"));
const DedicatedServerPage = lazy(() => import("./pages/DedicatedServerPage"));
const ShowroomPage = lazy(() => import("./pages/ShowroomPage"));
const CarsPage = lazy(() => import("./pages/CarsPage"));
const TracksPage = lazy(() => import("./pages/TracksPage"));
const SetupsPage = lazy(() => import("./pages/SetupsPage"));
const StudioPage = lazy(() => import("./pages/StudioPage"));
const LibraryPage = lazy(() => import("./pages/LibraryPage"));
const SessionsPage = lazy(() => import("./pages/SessionsPage"));
const LiveTelemetryPage = lazy(() => import("./pages/LiveTelemetryPage"));
const ReplayPage = lazy(() => import("./pages/ReplayPage"));
const TrajectoryPage = lazy(() => import("./pages/TrajectoryPage"));
const StrategyTimingPage = lazy(() => import("./pages/StrategyTimingPage"));
const GameSettingsPage = lazy(() => import("./pages/GameSettingsPage"));
const CspPage = lazy(() => import("./pages/CspPage"));
const AppSettingsPage = lazy(() => import("./pages/AppSettingsPage"));

function subTabIcon(id: SubTab) {
  switch (id) {
    case "quick-race":
      return Flame;
    case "events":
      return Award;
    case "championships":
      return Trophy;
    case "showroom":
      return Sparkles;
    case "cars":
      return Car;
    case "tracks":
      return MapIcon;
    case "setups":
      return Sliders;
    case "skins":
    case "studio":
      return Palette;
    case "painter":
      return Brush;
    case "plates":
      return CreditCard;
    case "models":
      return Box;
    case "installer":
    case "library":
      return Package;
    case "jsgme":
      return Archive;
    case "health":
      return ShieldAlert;
    case "tuning":
      return Gauge;
    case "sfx":
      return Volume2;
    case "servers":
      return Globe;
    case "dedicated":
      return Server;
    case "sessions":
      return Activity;
    case "live":
      return Gauge;
    case "replay":
      return Film;
    case "trajectory":
      return Route;
    case "strategy":
      return Fuel;
    case "timing":
      return Flag;
    case "laptimes":
      return Trophy;
    case "game":
      return Sliders;
    case "csp":
      return Wrench;
    case "csp-patches":
      return Layers;
    case "csp-weather":
      return CloudRain;
    case "app":
      return Settings;
    default:
      return Sparkles;
  }
}

function PageFallback() {
  return (
    <div className="p-6 max-w-[1600px] mx-auto space-y-4">
      <Skeleton className="h-14 w-72" />
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Skeleton className="h-44 w-full" />
        <Skeleton className="h-44 w-full" />
        <Skeleton className="h-44 w-full" />
      </div>
    </div>
  );
}

function ActiveView() {
  const { pillar, subTab } = useAppStore();

  switch (pillar) {
    case "paddock":
      return <PaddockPage />;
    case "drive":
      switch (subTab) {
        case "events":
          return <EventsPage />;
        case "championships":
          return <ChampionshipsPage />;
        case "quick-race":
        default:
          return <DrivePage />;
      }
    case "garage":
      switch (subTab) {
        case "cars":
          return <CarsPage />;
        case "tracks":
          return <TracksPage />;
        case "setups":
          return <SetupsPage />;
        case "skins":
        case "studio":
          return <StudioPage view="skins" />;
        case "painter":
          return <StudioPage view="painter" />;
        case "plates":
          return <StudioPage view="plates" />;
        case "models":
          return <StudioPage view="models" />;
        case "installer":
        case "library":
          return <LibraryPage view="installer" />;
        case "jsgme":
          return <LibraryPage view="jsgme" />;
        case "health":
          return <LibraryPage view="health" />;
        case "tuning":
          return <LibraryPage view="tuning" />;
        case "sfx":
          return <LibraryPage view="sfx" />;
        case "showroom":
        default:
          return <ShowroomPage />;
      }
    case "online":
      return <ServersPage />;
    case "server":
      return <DedicatedServerPage />;
    case "telemetry":
      switch (subTab) {
        case "live":
          return <LiveTelemetryPage />;
        case "replay":
          return <ReplayPage />;
        case "trajectory":
          return <TrajectoryPage />;
        case "strategy":
          return <StrategyTimingPage view="strategy" />;
        case "timing":
          return <StrategyTimingPage view="timing" />;
        case "laptimes":
          return <StrategyTimingPage view="laptimes" />;
        case "sessions":
        default:
          return <SessionsPage />;
      }
    case "setup":
      switch (subTab) {
        case "csp":
          return <CspPage view="editor" />;
        case "csp-patches":
          return <CspPage view="patches" />;
        case "csp-weather":
          return <CspPage view="weather" />;
        case "app":
          return <AppSettingsPage />;
        case "game":
        default:
          return <GameSettingsPage />;
      }
    default:
      return <PaddockPage />;
  }
}

function MainContent() {
  const { pillar, subTab, navigate } = useAppStore();
  const activePillar = PILLAR_NAV.find((p) => p.id === pillar);
  const subTabs = activePillar?.subTabs ?? [];

  // Group sub-tabs cleanly by their optional group header
  const groupedTabs = subTabs.reduce<Array<{ group: string; items: typeof subTabs }>>(
    (acc, tab) => {
      const g = tab.group ?? activePillar?.label ?? "";
      const last = acc[acc.length - 1];
      if (last && last.group === g) {
        last.items.push(tab);
      } else {
        acc.push({ group: g, items: [tab] });
      }
      return acc;
    },
    []
  );

  return (
    <div className="flex-1 min-h-0 flex overflow-hidden">
      {/* Premium Unified Left Rail — grouped, icon-led, glassmorphic active pill */}
      {subTabs.length > 0 && (
        <aside className="w-[222px] shrink-0 border-r border-white/[0.06] bg-gradient-to-b from-[#0A0E16]/95 to-[#07090F]/95 px-3 py-4 flex flex-col gap-5 select-none overflow-y-auto">
          {groupedTabs.map(({ group, items }, gIdx) => (
            <div key={group} className={gIdx > 0 ? "pt-4 border-t border-white/[0.05] space-y-1" : "space-y-1"}>
              <div className="px-2.5 pb-1.5 text-[10px] font-mono font-semibold uppercase tracking-[0.18em] text-[var(--text-muted)]">
                {group}
              </div>
              {items.map((tab) => {
                const active = tab.id === subTab;
                const Icon = subTabIcon(tab.id);
                return (
                  <button
                    key={tab.id}
                    type="button"
                    onClick={() => navigate(pillar, tab.id)}
                    className={`group relative w-full text-left px-2.5 py-2 rounded-xl text-[13px] font-medium transition-all duration-150 cursor-pointer flex items-center gap-2.5 ${
                      active
                        ? "text-white bg-white/[0.07] shadow-[inset_0_1px_0_rgba(255,255,255,0.08)]"
                        : "text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-white/[0.035]"
                    }`}
                  >
                    {active && (
                      <span
                        className="absolute left-0 top-2 bottom-2 w-[3px] rounded-r-full"
                        style={{
                          background: "var(--accent)",
                          boxShadow: "0 0 10px var(--accent-glow)",
                        }}
                      />
                    )}
                    <span
                      className={`w-6 h-6 rounded-lg flex items-center justify-center shrink-0 transition-colors ${
                        active
                          ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                          : "bg-white/[0.03] text-[var(--text-muted)] group-hover:text-[var(--text-secondary)]"
                      }`}
                    >
                      <Icon className="w-3.5 h-3.5" />
                    </span>
                    <span className="truncate">{tab.label}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </aside>
      )}

      <main className="flex-1 min-w-0 min-h-0 overflow-y-auto overflow-x-hidden">
        <PageErrorBoundary resetKey={`${pillar}:${subTab}`}>
          <Suspense fallback={<PageFallback />}>
            <ActiveView />
          </Suspense>
        </PageErrorBoundary>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <AppProvider>
      <div className="h-screen w-screen flex flex-col overflow-hidden acxs-atmosphere text-[var(--text-primary)]">
        <TopBar />
        <MainContent />
        <BottomDock />
        <CommandPalette />
        <ToastContainer />
      </div>
    </AppProvider>
  );
}

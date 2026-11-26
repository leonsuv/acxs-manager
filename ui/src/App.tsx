import { Component, Suspense, lazy, type ReactNode } from "react";
import { AppProvider, PILLAR_NAV, useAppStore } from "./store/appStore";
import { TopBar } from "./components/TopBar";
import { BottomDock } from "./components/BottomDock";
import { CommandPalette } from "./components/CommandPalette";
import { ToastContainer } from "./components/ToastContainer";
import { Button, Card, Skeleton } from "./components/Primitives";

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
            <Button onClick={() => this.setState({ error: null })}>
              Retry
            </Button>
          </Card>
        </div>
      );
    }
    return this.props.children;
  }
}

// Wave T2A pages
const PaddockPage = lazy(() => import("./pages/PaddockPage"));
const DrivePage = lazy(() => import("./pages/DrivePage"));
const EventsPage = lazy(() => import("./pages/EventsPage"));
const ChampionshipsPage = lazy(() => import("./pages/ChampionshipsPage"));
const ServersPage = lazy(() => import("./pages/ServersPage"));
const DedicatedServerPage = lazy(() => import("./pages/DedicatedServerPage"));

// Wave T2B pages
const ShowroomPage = lazy(() => import("./pages/ShowroomPage"));
const CarsPage = lazy(() => import("./pages/CarsPage"));
const TracksPage = lazy(() => import("./pages/TracksPage"));
const SetupsPage = lazy(() => import("./pages/SetupsPage"));
const StudioPage = lazy(() => import("./pages/StudioPage"));
const LibraryPage = lazy(() => import("./pages/LibraryPage"));

// Wave T2C pages
const SessionsPage = lazy(() => import("./pages/SessionsPage"));
const LiveTelemetryPage = lazy(() => import("./pages/LiveTelemetryPage"));
const ReplayPage = lazy(() => import("./pages/ReplayPage"));
const TrajectoryPage = lazy(() => import("./pages/TrajectoryPage"));
const StrategyTimingPage = lazy(() => import("./pages/StrategyTimingPage"));
const GameSettingsPage = lazy(() => import("./pages/GameSettingsPage"));
const CspPage = lazy(() => import("./pages/CspPage"));
const AppSettingsPage = lazy(() => import("./pages/AppSettingsPage"));

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
        case "studio":
          return <StudioPage />;
        case "library":
          return <LibraryPage />;
        case "showroom":
        default:
          return <ShowroomPage />;
      }
    case "online":
      switch (subTab) {
        case "dedicated":
          return <DedicatedServerPage />;
        case "servers":
        default:
          return <ServersPage />;
      }
    case "telemetry":
      switch (subTab) {
        case "live":
          return <LiveTelemetryPage />;
        case "replay":
          return <ReplayPage />;
        case "trajectory":
          return <TrajectoryPage />;
        case "strategy":
          return <StrategyTimingPage />;
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

  return (
    <div className="flex-1 min-h-0 flex overflow-hidden">
      {/* Unified Level-2 Left Navigation Rail — anchored directly under TopBar */}
      {subTabs.length > 0 && (
        <aside className="w-[196px] shrink-0 border-r border-white/[0.06] bg-[#080B11]/85 backdrop-blur-md px-2.5 py-4 flex flex-col gap-1 select-none overflow-y-auto">
          <div className="px-2.5 pb-2 text-[10px] font-mono uppercase tracking-[0.18em] text-[var(--text-muted)]">
            {activePillar?.label}
          </div>
          {subTabs.map((tab) => {
            const active = tab.id === subTab;
            return (
              <button
                key={tab.id}
                type="button"
                onClick={() => navigate(pillar, tab.id)}
                className={`relative w-full text-left px-3 py-2 rounded-lg text-[13px] font-medium transition-all duration-150 cursor-pointer flex items-center ${
                  active
                    ? "bg-[var(--accent-soft)] text-[var(--text-primary)] border border-[var(--accent-border)]"
                    : "text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-white/[0.04] border border-transparent"
                }`}
              >
                {active && (
                  <span
                    className="absolute left-0 top-2 bottom-2 w-[3px] rounded-r-full"
                    style={{ background: "var(--accent)" }}
                  />
                )}
                <span className="truncate">{tab.label}</span>
              </button>
            );
          })}
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

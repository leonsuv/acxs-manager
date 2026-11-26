import { Suspense, lazy } from "react";
import { AppProvider, useAppStore } from "./store/appStore";
import { TopBar } from "./components/TopBar";
import { BottomDock } from "./components/BottomDock";
import { CommandPalette } from "./components/CommandPalette";
import { ToastContainer } from "./components/ToastContainer";
import { Skeleton } from "./components/Primitives";

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
          return <CspPage />;
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

export default function App() {
  return (
    <AppProvider>
      <div className="h-screen w-screen flex flex-col overflow-hidden acxs-atmosphere text-[var(--text-primary)]">
        <TopBar />
        <main className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
          <Suspense fallback={<PageFallback />}>
            <ActiveView />
          </Suspense>
        </main>
        <BottomDock />
        <CommandPalette />
        <ToastContainer />
      </div>
    </AppProvider>
  );
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  Bell,
  BellRing,
  Car,
  Check,
  CheckCircle2,
  Clock,
  Copy,
  Download,
  ExternalLink,
  Filter,
  Globe,
  Info,
  Link2,
  Lock,
  Play,
  RefreshCw,
  RotateCcw,
  Search,
  ShieldAlert,
  Star,
  User,
  Users,
  Wifi,
  X,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import type {
  ContentOwnershipFilter,
  CspRequirementFilter,
  OnlineServerSummary,
  ServerCarSlot,
  ServerSortKey,
  ServerTab,
  SkinEntry,
} from "../lib/types";
import {
  buildCarOwnershipIndex,
  buildTrackOwnershipIndex,
  evaluateServerOwnership,
  isCarOwnedInIndex,
  resolveTrackFromIndex,
  type ServerOwnershipEvaluation,
} from "./serverOwnership";
import { useAppStore } from "../store/appStore";
import { useContextMenu } from "../components/ContextMenu";
import {
  Badge,
  Button,
  EmptyState,
  SegmentedControl,
  Skeleton,
} from "../components/Primitives";

type CarFilter = "all" | "with-drivers" | "free" | "installed";

const DRIVER_NAME_KEY = "acxs_driver_name_v1";
const BARE_HOST_PORT_RE = /^(\[[0-9a-fA-F:]+\]|[a-zA-Z0-9.-]+):(\d{1,5})$/;

interface GroupedCarSlot {
  carId: string;
  carName: string;
  previewPath?: string | null;
  totalSlots: number;
  freeSlots: number;
  owned: boolean;
  skins: string[];
  slots: ServerCarSlot[];
}

function pingStyle(ping?: number | null): string {
  if (ping == null) return "bg-white/[0.06] text-[var(--text-muted)] border-white/[0.08]";
  if (ping < 55) return "bg-emerald-500/15 text-emerald-300 border-emerald-500/30";
  if (ping < 115) return "bg-amber-500/15 text-amber-300 border-amber-500/30";
  return "bg-red-500/15 text-red-300 border-red-500/30";
}

function capacityBarColor(clients: number, max: number): string {
  const ratio = max > 0 ? clients / max : 0;
  if (ratio >= 0.95) return "bg-red-400";
  if (ratio >= 0.72) return "bg-amber-400";
  return "bg-emerald-400";
}

function parseCspNumber(cspBuild: string | number | null | undefined): number | null {
  if (cspBuild == null) return null;
  if (typeof cspBuild === "number") return Number.isFinite(cspBuild) ? cspBuild : null;
  const str = String(cspBuild);
  const paren = str.match(/\((\d{4,})\)/);
  if (paren) return Number(paren[1]);
  const match = str.match(/(\d{4,})/);
  if (match) return Number(match[1]);
  if (str.startsWith("0.3")) return 4000;
  if (str.startsWith("0.2")) return 2800;
  if (str.startsWith("0.1.79")) return 2144;
  return 2000;
}

function formatRelativeAge(unixSeconds: number | null): string {
  if (!unixSeconds) return "";
  const diffSec = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
  if (diffSec < 60) return "just now";
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

export function ServersPage() {
  const {
    cars,
    tracks,
    cspBuild,
    gameRunning,
    notify,
    onlineTab: tab,
    setOnlineTab: setTab,
    onlineServers,
    onlineCacheSavedUnix,
    onlineStreamProgress,
    remoteSearchResults,
    remoteSearchLoading,
    watchlist,
    recentServers,
    lanServers,
    onlineTabLoading,
    onlineSearchInput: searchInput,
    setOnlineSearchInput: setSearchInput,
    onlineContentFilter: contentFilter,
    setOnlineContentFilter: setContentFilter,
    onlineHideEmpty: hideEmpty,
    setOnlineHideEmpty: setHideEmpty,
    onlineHideFull: hideFull,
    setOnlineHideFull: setHideFull,
    onlineHidePassworded: hidePassworded,
    setOnlineHidePassworded: setHidePassworded,
    onlineRegionFilter: regionFilter,
    setOnlineRegionFilter: setRegionFilter,
    onlineCspFilter: cspFilter,
    setOnlineCspFilter: setCspFilter,
    onlineSessionFilter: sessionFilter,
    setOnlineSessionFilter: setSessionFilter,
    onlineMinPlayers: minPlayers,
    setOnlineMinPlayers: setMinPlayers,
    resetOnlineFilters,
    probedOwnership,
    onlineSort: sort,
    setOnlineSort: setSort,
    selectedOnlineServer: selectedServer,
    selectOnlineServer: setSelectedServer,
    selectedOnlineDetail: detail,
    loadingOnlineDetail: loadingDetail,
    watchedSlots,
    toggleWatchSlot,
    toggleWatchlistServer,
    ensureOnlineInitialized,
    refreshOnlineDirectory,
    runRemoteDirectorySearch,
    clearRemoteDirectorySearch,
    loadWatchlist,
    loadRecentServers,
    scanLanServers,
  } = useAppStore();

  const [carSearch, setCarSearch] = useState("");
  const [carFilter, setCarFilter] = useState<CarFilter>("all");
  const [selectedCarId, setSelectedCarId] = useState<string | null>(null);
  const [selectedSkinId, setSelectedSkinId] = useState<string>("");
  const [localSkins, setLocalSkins] = useState<SkinEntry[]>([]);
  const [password, setPassword] = useState("");
  const [driverName, setDriverName] = useState<string>(() => {
    try {
      return localStorage.getItem(DRIVER_NAME_KEY) ?? "";
    } catch {
      return "";
    }
  });
  const { openContextMenu } = useContextMenu();
  const [joining, setJoining] = useState(false);

  const selectedCarIdRef = useRef<string | null>(selectedCarId);
  selectedCarIdRef.current = selectedCarId;
  const pendingInviteRef = useRef<{
    serverId: string;
    password?: string;
    car?: string;
    skin?: string;
  } | null>(null);

  const trackIndex = useMemo(() => buildTrackOwnershipIndex(tracks), [tracks]);
  const carIndex = useMemo(() => buildCarOwnershipIndex(cars), [cars]);

  const findTrackMeta = useCallback(
    (trackIdOrName: string | undefined | null) =>
      resolveTrackFromIndex(trackIdOrName, trackIndex),
    [trackIndex]
  );

  const isCarOwnedSummary = useCallback(
    (carIdOrName: string) => isCarOwnedInIndex(carIdOrName, carIndex),
    [carIndex]
  );

  const localCspBuildNum = useMemo(() => parseCspNumber(cspBuild), [cspBuild]);

  const activeDetail =
    !loadingDetail &&
    detail &&
    selectedServer &&
    detail.summary.id === selectedServer.id
      ? detail
      : null;

  const liveSummary = activeDetail?.summary ?? selectedServer;

  // Initialize lifted Online state once without resetting across pillar switches
  useEffect(() => {
    ensureOnlineInitialized();
  }, [ensureOnlineInitialized]);

  // Handle acmanager:// or host:port paste
  const isInviteUri = useMemo(() => {
    const t = searchInput.trim();
    const lower = t.toLowerCase();
    if (lower.startsWith("acmanager://") || lower.startsWith("acxs://")) {
      return true;
    }
    const bare = t.match(BARE_HOST_PORT_RE);
    if (!bare) return false;
    const port = Number(bare[2]);
    return Number.isInteger(port) && port >= 1 && port <= 65535;
  }, [searchInput]);

  // Debounced remote directory search for q >= 2 on Online tab (keeps onlineServers intact)
  useEffect(() => {
    if (tab !== "online" || isInviteUri) {
      clearRemoteDirectorySearch();
      return;
    }
    const q = searchInput.trim();
    if (q.length < 2) {
      clearRemoteDirectorySearch();
      return;
    }
    const timer = window.setTimeout(() => {
      void runRemoteDirectorySearch(q);
    }, 350);
    return () => window.clearTimeout(timer);
  }, [tab, searchInput, isInviteUri, runRemoteDirectorySearch, clearRemoteDirectorySearch]);

  // Reset password, carSearch, and selection only when switching to a different server
  useEffect(() => {
    const pending = pendingInviteRef.current;
    if (pending && pending.serverId === selectedServer?.id) {
      setPassword(pending.password ?? "");
    } else {
      pendingInviteRef.current = null;
      setPassword("");
    }
    setCarSearch("");
    setCarFilter("all");
    selectedCarIdRef.current = null;
    setSelectedCarId(null);
    setSelectedSkinId("");
  }, [selectedServer?.id]);

  // Sync car/skin defaults when activeDetail loads or refreshes, preserving valid selection
  useEffect(() => {
    if (!activeDetail) return;
    const slots = activeDetail.slots ?? [];
    const currentCarId = selectedCarIdRef.current;
    if (currentCarId && slots.some((s) => s.carId === currentCarId)) {
      return;
    }
    const pending = pendingInviteRef.current;
    if (pending && pending.serverId === activeDetail.summary.id) {
      pendingInviteRef.current = null;
      if (pending.car) {
        const invitedCarSlots = slots.filter((s) => s.carId === pending.car);
        if (invitedCarSlots.length > 0) {
          const freeInvited =
            invitedCarSlots.find((s) => !s.connected && s.skinId)?.skinId ??
            invitedCarSlots[0]?.skinId ??
            "";
          setSelectedCarId(pending.car);
          setSelectedSkinId(pending.skin || freeInvited);
          return;
        }
      }
    }
    const firstOwned =
      slots.find((s) => s.owned && !s.connected) ??
      slots.find((s) => s.owned) ??
      slots[0];
    if (firstOwned) {
      const carSlots = slots.filter((s) => s.carId === firstOwned.carId);
      const freeSkin =
        carSlots.find((s) => !s.connected && s.skinId)?.skinId ??
        carSlots.find((s) => s.skinId)?.skinId ??
        "";
      setSelectedCarId(firstOwned.carId);
      setSelectedSkinId(freeSkin);
    } else {
      setSelectedCarId(null);
      setSelectedSkinId("");
    }
  }, [activeDetail]);

  // Load local skins for selected car
  useEffect(() => {
    if (!selectedCarId) {
      setLocalSkins([]);
      return;
    }
    const car = cars.find((c) => c.id === selectedCarId);
    if (!car?.path) {
      setLocalSkins([]);
      return;
    }
    let cancelled = false;
    void api
      .getCarSkins(car.path)
      .then((list) => {
        if (cancelled) return;
        const resolved = list ?? [];
        setLocalSkins(resolved);
        setSelectedSkinId((prev) => prev || resolved[0]?.id || "");
      })
      .catch(() => {
        if (!cancelled) setLocalSkins([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedCarId, cars]);

  const handleTabChange = (nextTab: ServerTab) => {
    setTab(nextTab);
    if (nextTab === "watchlist") {
      void loadWatchlist();
    } else if (nextTab === "recent") {
      void loadRecentServers();
    } else if (nextTab === "lan" && lanServers.length === 0) {
      void scanLanServers();
    }
  };

  const handleRefreshActiveTab = () => {
    if (tab === "online") {
      void refreshOnlineDirectory();
    } else if (tab === "watchlist") {
      void loadWatchlist();
    } else if (tab === "recent") {
      void loadRecentServers();
    } else {
      void scanLanServers();
    }
  };

  // Index known live servers by host:port for hydrating Watchlist & Recent rows
  const liveByHostPort = useMemo(() => {
    const map = new Map<string, OnlineServerSummary>();
    for (const s of remoteSearchResults) {
      map.set(`${s.host}:${s.httpPort ?? s.port}`, s);
      map.set(`${s.host}:${s.port}`, s);
    }
    for (const s of onlineServers) {
      map.set(`${s.host}:${s.httpPort ?? s.port}`, s);
      map.set(`${s.host}:${s.port}`, s);
    }
    return map;
  }, [onlineServers, remoteSearchResults]);

  // Raw per-tab server list (hydrated where applicable)
  const rawTabServers = useMemo<OnlineServerSummary[]>(() => {
    if (tab === "online") {
      const q = isInviteUri ? "" : searchInput.trim();
      if (q.length >= 2 && remoteSearchResults.length > 0) {
        const merged = new Map<string, OnlineServerSummary>();
        for (const s of onlineServers) merged.set(s.id, s);
        for (const s of remoteSearchResults) {
          if (!merged.has(s.id)) merged.set(s.id, s);
        }
        return Array.from(merged.values());
      }
      return onlineServers;
    }
    if (tab === "watchlist") {
      return watchlist.map((w) => {
        const key = `${w.host}:${w.port}`;
        const match = liveByHostPort.get(key);
        if (match) {
          return {
            ...match,
            name: match.name || w.name || key,
          };
        }
        return {
          id: key,
          name: w.name || key,
          host: w.host,
          port: w.port,
          httpPort: w.port,
          trackId: "unknown",
          clients: 0,
          maxClients: 24,
          cars: [],
          passworded: false,
        };
      });
    }
    if (tab === "recent") {
      return recentServers.map((r) => {
        const match =
          liveByHostPort.get(`${r.host}:${r.httpPort ?? r.port}`) ??
          liveByHostPort.get(`${r.host}:${r.port}`);
        if (!match) return r;
        return {
          ...match,
          name: match.name || r.name,
          trackId: match.trackId !== "unknown" ? match.trackId : r.trackId,
        };
      });
    }
    return lanServers;
  }, [
    tab,
    isInviteUri,
    searchInput,
    onlineServers,
    remoteSearchResults,
    watchlist,
    recentServers,
    lanServers,
    liveByHostPort,
  ]);

  // Compute immediate Track & Car ownership evaluation per server row
  const ownershipById = useMemo(() => {
    const map = new Map<string, ServerOwnershipEvaluation>();
    for (const s of rawTabServers) {
      const probed = probedOwnership[s.id];
      const syntheticDetail = probed
        ? {
            summary: s,
            resolvedTrackId: probed.resolvedTrackId ?? s.trackId,
            trackOwned: probed.trackOwned,
            slots: [],
            missingContent: [],
          }
        : null;
      const ev = evaluateServerOwnership(s, trackIndex, carIndex, syntheticDetail);
      if (probed && probed.totalCars > 0) {
        ev.ownedCars = probed.ownedCars;
        ev.totalCars = probed.totalCars;
        ev.carState =
          probed.ownedCars === probed.totalCars
            ? "all"
            : probed.ownedCars > 0
              ? "partial"
              : "none";
      }
      map.set(s.id, ev);
    }
    return map;
  }, [rawTabServers, probedOwnership, trackIndex, carIndex]);

  // Dynamically available Region options from current server directory
  const availableRegions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of rawTabServers) {
      const reg = s.region?.trim();
      if (reg) counts.set(reg, (counts.get(reg) ?? 0) + 1);
    }
    return Array.from(counts.entries())
      .sort((a, b) => b[1] - a[1])
      .map(([name]) => name);
  }, [rawTabServers]);

  const tabCounts = useMemo(
    () => ({
      online: onlineServers.length,
      watchlist: watchlist.length,
      recent: recentServers.length,
      lan: lanServers.length,
    }),
    [onlineServers.length, watchlist.length, recentServers.length, lanServers.length]
  );

  // Fast client-side filtering across ALL tabs (including "online") + deterministic sorting
  const filteredServers = useMemo(() => {
    const q = isInviteUri ? "" : searchInput.trim().toLowerCase();
    const effectiveMinPlayers = Math.max(hideEmpty ? 1 : 0, minPlayers);
    const list = rawTabServers.filter((s) => {
      if (effectiveMinPlayers > 0 && s.clients < effectiveMinPlayers) return false;
      if (hideFull && s.maxClients > 0 && s.clients >= s.maxClients) return false;
      if (hidePassworded && s.passworded) return false;
      if (regionFilter !== "all") {
        const reg = (s.region ?? "").toLowerCase();
        const cnt = (s.country ?? "").toLowerCase();
        if (reg !== regionFilter.toLowerCase() && cnt !== regionFilter.toLowerCase()) {
          return false;
        }
      }
      if (sessionFilter !== "all") {
        if ((s.sessionType ?? "").toLowerCase() !== sessionFilter.toLowerCase()) {
          return false;
        }
      }
      if (cspFilter !== "all") {
        if (cspFilter === "required" && !s.cspRequired) return false;
        if (cspFilter === "vanilla" && Boolean(s.cspRequired)) return false;
        if (
          cspFilter === "compatible" &&
          s.cspRequired &&
          localCspBuildNum !== null &&
          localCspBuildNum < s.cspRequired
        ) {
          return false;
        }
      }
      const ev = ownershipById.get(s.id);
      if (contentFilter !== "all" && ev) {
        if (contentFilter === "ready") {
          if (ev.trackState !== "owned" || ev.carState === "none") return false;
        } else if (contentFilter === "full") {
          if (ev.trackState !== "owned" || ev.carState !== "all") return false;
        } else if (contentFilter === "have-track") {
          if (ev.trackState !== "owned") return false;
        } else if (contentFilter === "missing") {
          if (ev.trackState === "owned" && (ev.carState === "all" || ev.carState === "unknown")) {
            return false;
          }
        }
      }
      if (!q) return true;
      const trackName =
        (ev?.matchedTrack?.name ?? findTrackMeta(s.trackId)?.name ?? "").toLowerCase();
      return (
        s.name.toLowerCase().includes(q) ||
        s.trackId.toLowerCase().includes(q) ||
        trackName.includes(q) ||
        s.host.toLowerCase().includes(q) ||
        (s.country ?? "").toLowerCase().includes(q) ||
        (s.region ?? "").toLowerCase().includes(q) ||
        s.cars.some((c) => c.toLowerCase().includes(q)) ||
        (s.playerNames ?? []).some((p) => p.toLowerCase().includes(q))
      );
    });

    const dirMul = sort.dir === "asc" ? 1 : -1;
    return [...list].sort((a, b) => {
      switch (sort.key) {
        case "name":
          return a.name.localeCompare(b.name) * dirMul;
        case "track": {
          const ta = findTrackMeta(a.trackId)?.name ?? a.trackId;
          const tb = findTrackMeta(b.trackId)?.name ?? b.trackId;
          return ta.localeCompare(tb) * dirMul;
        }
        case "content": {
          const ea = ownershipById.get(a.id);
          const eb = ownershipById.get(b.id);
          const sa = ea?.contentScore ?? 0;
          const sb = eb?.contentScore ?? 0;
          if (sa !== sb) return (sa - sb) * dirMul;
          const oa = ea?.ownedCars ?? 0;
          const ob = eb?.ownedCars ?? 0;
          if (oa !== ob) return (oa - ob) * dirMul;
          return b.clients - a.clients;
        }
        case "ping": {
          const pa = a.pingMs ?? Number.MAX_SAFE_INTEGER;
          const pb = b.pingMs ?? Number.MAX_SAFE_INTEGER;
          return (pa - pb) * dirMul || b.clients - a.clients;
        }
        case "clients":
        default:
          return (a.clients - b.clients) * dirMul || a.name.localeCompare(b.name);
      }
    });
  }, [
    rawTabServers,
    ownershipById,
    isInviteUri,
    searchInput,
    hideEmpty,
    minPlayers,
    hideFull,
    hidePassworded,
    regionFilter,
    sessionFilter,
    cspFilter,
    localCspBuildNum,
    contentFilter,
    findTrackMeta,
    sort,
  ]);

  const handleSortClick = (key: ServerSortKey) => {
    if (sort.key === key) {
      setSort({ key, dir: sort.dir === "asc" ? "desc" : "asc" });
    } else {
      setSort({ key, dir: key === "clients" ? "desc" : "asc" });
    }
  };

  const handleOpenInviteUri = async () => {
    const raw = searchInput.trim();
    if (!raw) return;
    try {
      const bareMatch = raw.match(BARE_HOST_PORT_RE);
      let host: string;
      let httpPort: number;
      let invitePassword: string | undefined;
      let inviteCar: string | undefined;
      let inviteSkin: string | undefined;

      if (bareMatch) {
        host = bareMatch[1];
        httpPort = Number(bareMatch[2]);
        if (!Number.isInteger(httpPort) || httpPort < 1 || httpPort > 65535) {
          throw new Error("Port must be between 1 and 65535");
        }
      } else {
        const parsed = await api.parseJoinUri(raw);
        if (!parsed?.host || !parsed.httpPort) {
          throw new Error("Invalid invite URI");
        }
        host = parsed.host;
        httpPort = parsed.httpPort;
        invitePassword = parsed.password;
        inviteCar = parsed.car;
        inviteSkin = parsed.skin;
      }

      const id = `${host}:${httpPort}`;
      pendingInviteRef.current = {
        serverId: id,
        password: invitePassword,
        car: inviteCar,
        skin: inviteSkin,
      };
      if (selectedServer?.id === id) {
        selectedCarIdRef.current = null;
        setSelectedCarId(null);
        setSelectedSkinId("");
        setPassword(invitePassword ?? "");
      }
      const synthetic: OnlineServerSummary = {
        id,
        name: `Direct Connect · ${host}:${httpPort}`,
        host,
        port: httpPort,
        httpPort,
        trackId: "unknown",
        clients: 0,
        maxClients: 24,
        cars: [],
        passworded: Boolean(invitePassword),
      };
      setSelectedServer(synthetic);
      if (invitePassword) setPassword(invitePassword);
      notify("Invite parsed", "info", `${host}:${httpPort}`);
    } catch (err) {
      notify("Invalid invite link", "error", String(err));
    }
  };

  const isFavorite = (s: OnlineServerSummary) => {
    const targetPort = s.httpPort ?? s.port;
    return watchlist.some(
      (w) => w.host === s.host && (w.port === targetPort || w.port === s.port)
    );
  };

  // All grouped car slots for the selected server (unfiltered, so selection/skins stay intact)
  const allGroupedCars = useMemo<GroupedCarSlot[]>(() => {
    if (!activeDetail) return [];
    const map = new Map<string, GroupedCarSlot>();
    for (const slot of activeDetail.slots) {
      const existing = map.get(slot.carId);
      if (existing) {
        existing.totalSlots += 1;
        if (!slot.connected) existing.freeSlots += 1;
        if (slot.skinId && !existing.skins.includes(slot.skinId)) {
          existing.skins.push(slot.skinId);
        }
        existing.slots.push(slot);
      } else {
        const carMeta = cars.find(
          (c) => c.id.toLowerCase() === slot.carId.toLowerCase()
        );
        map.set(slot.carId, {
          carId: slot.carId,
          carName: carMeta?.name ?? slot.carId,
          previewPath: carMeta?.previewPath,
          totalSlots: 1,
          freeSlots: slot.connected ? 0 : 1,
          owned: slot.owned || isCarOwnedSummary(slot.carId),
          skins: slot.skinId ? [slot.skinId] : [],
          slots: [slot],
        });
      }
    }
    return Array.from(map.values()).sort(
      (a, b) => Number(b.owned) - Number(a.owned) || b.freeSlots - a.freeSlots
    );
  }, [activeDetail, cars, isCarOwnedSummary]);

  const groupedCars = useMemo<GroupedCarSlot[]>(() => {
    const q = carSearch.trim().toLowerCase();
    return allGroupedCars.filter((g) => {
      if (carFilter === "with-drivers" && !g.slots.some((s) => s.connected)) {
        return false;
      }
      if (carFilter === "free" && g.freeSlots <= 0) return false;
      if (carFilter === "installed" && !g.owned) return false;
      if (!q) return true;
      return (
        g.carName.toLowerCase().includes(q) ||
        g.carId.toLowerCase().includes(q) ||
        g.slots.some((s) => (s.driverName ?? "").toLowerCase().includes(q))
      );
    });
  }, [allGroupedCars, carSearch, carFilter]);

  const pickPreferredSkin = useCallback((group?: GroupedCarSlot | null): string => {
    if (!group) return "";
    return (
      group.slots.find((s) => !s.connected && s.skinId)?.skinId ??
      group.skins[0] ??
      ""
    );
  }, []);

  const selectCarGroup = useCallback(
    (group: GroupedCarSlot) => {
      setSelectedCarId(group.carId);
      setSelectedSkinId(pickPreferredSkin(group));
    },
    [pickPreferredSkin]
  );

  const connectedDrivers = useMemo(
    () => (activeDetail?.slots ?? []).filter((s) => s.connected),
    [activeDetail]
  );

  const selectedCarGroup = useMemo(
    () => allGroupedCars.find((g) => g.carId === selectedCarId) ?? null,
    [allGroupedCars, selectedCarId]
  );

  // Compute clear join blockers
  const joinBlockers = useMemo<string[]>(() => {
    if (!liveSummary) return ["Select a server"];
    const reasons: string[] = [];
    if (loadingDetail) {
      reasons.push("Loading server details...");
      return reasons;
    }
    if (!activeDetail) {
      reasons.push("Server details unavailable");
      return reasons;
    }
    if (!activeDetail.trackOwned) reasons.push("Missing Track");
    if (
      activeDetail.minCspBuild &&
      localCspBuildNum !== null &&
      localCspBuildNum < activeDetail.minCspBuild
    ) {
      reasons.push(`Requires CSP ≥ ${activeDetail.minCspBuild}`);
    }
    if (!selectedCarGroup) {
      reasons.push("Select a Car");
    } else {
      if (!selectedCarGroup.owned) reasons.push("Missing Car");
      if (selectedCarGroup.freeSlots === 0 && !activeDetail.bookingMode) {
        reasons.push("Car Class Full");
      }
    }
    if (
      liveSummary.maxClients > 0 &&
      liveSummary.clients >= liveSummary.maxClients &&
      !activeDetail.bookingMode
    ) {
      reasons.push("Server Full");
    }
    if (gameRunning) reasons.push("AC Already Running");
    return reasons;
  }, [
    activeDetail,
    loadingDetail,
    liveSummary,
    selectedCarGroup,
    localCspBuildNum,
    gameRunning,
  ]);

  const canJoin = joinBlockers.length === 0;

  const handleJoin = async (overrideCarId?: string, overrideSkinId?: string) => {
    if (!selectedServer || !activeDetail || loadingDetail) return;
    const carId = overrideCarId ?? selectedCarId;
    if (!carId) return;
    setJoining(true);
    try {
      const req = {
        host: selectedServer.host,
        port: activeDetail.summary.port || selectedServer.port,
        httpPort: selectedServer.httpPort ?? selectedServer.port,
        serverName: selectedServer.name,
        track: activeDetail.resolvedTrackId || selectedServer.trackId,
        trackLayout:
          selectedServer.trackLayout ?? activeDetail.summary.trackLayout ?? null,
        carId,
        skinId: overrideSkinId ?? (selectedSkinId || "00_default"),
        password: password || undefined,
        driverName: driverName.trim() || undefined,
        features: activeDetail.features ?? [],
      };
      if (activeDetail.bookingMode) {
        await api.bookOnlineServer(req);
        notify("Slot booked (/sub)", "success", selectedServer.name);
      } else {
        await api.joinOnlineServer(req);
        notify("Joining server", "success", selectedServer.name);
      }
    } catch (err) {
      notify("Could not join server", "error", String(err));
    } finally {
      setJoining(false);
    }
  };

  const handleCopyInvite = async () => {
    if (!selectedServer) return;
    try {
      const httpPort = selectedServer.httpPort ?? selectedServer.port;
      const uri =
        (await api.formatInviteUri(
          selectedServer.host,
          httpPort,
          password || undefined
        )) ??
        `acmanager://race/online/join?ip=${selectedServer.host}&httpPort=${httpPort}`;
      await navigator.clipboard.writeText(uri);
      notify("Invite link copied", "success", uri);
    } catch (err) {
      notify("Failed to copy invite link", "error", String(err));
    }
  };

  const handleDownloadMissing = () => {
    if (!activeDetail || activeDetail.missingContent.length === 0) return;
    const urls = activeDetail.missingContent
      .map((m) => m.downloadUrl)
      .filter((u): u is string => Boolean(u));
    if (urls.length > 0) {
      urls.forEach((u) => window.open(u, "_blank", "noopener,noreferrer"));
      notify(
        `Opened ${urls.length} content download${urls.length > 1 ? "s" : ""}`,
        "info"
      );
    } else {
      notify(
        "Missing content listed",
        "warning",
        activeDetail.missingContent.map((m) => `${m.kind}: ${m.id}`).join(", ")
      );
    }
  };

  const selectedTrackMeta = useMemo(() => {
    const tid = activeDetail?.resolvedTrackId ?? liveSummary?.trackId;
    return findTrackMeta(tid);
  }, [activeDetail, liveSummary, findTrackMeta]);

  const heroTrackImg = assetUrl(selectedTrackMeta?.previewPath);
  const activeTabLoading = onlineTabLoading[tab];
  const showSkeletons = activeTabLoading && rawTabServers.length === 0;
  const activeFilterCount =
    (contentFilter !== "all" ? 1 : 0) +
    (hideEmpty || minPlayers > 0 ? 1 : 0) +
    (hideFull ? 1 : 0) +
    (hidePassworded ? 1 : 0) +
    (regionFilter !== "all" ? 1 : 0) +
    (cspFilter !== "all" ? 1 : 0) +
    (sessionFilter !== "all" ? 1 : 0) +
    (searchInput.trim() ? 1 : 0);
  const hasActiveFilters = activeFilterCount > 0;

  const clearFilters = () => {
    resetOnlineFilters();
  };

  const statusPillText = useMemo(() => {
    if (tab === "online") {
      if (onlineStreamProgress?.loading) {
        return onlineStreamProgress.loaded > 0
          ? `Updating directory… ${onlineStreamProgress.loaded}/${onlineStreamProgress.total || "?"}`
          : "Updating directory…";
      }
      if (remoteSearchLoading) {
        return "Searching directory…";
      }
      if (onlineCacheSavedUnix) {
        return `Cached ${formatRelativeAge(onlineCacheSavedUnix)}`;
      }
    }
    return null;
  }, [tab, onlineStreamProgress, remoteSearchLoading, onlineCacheSavedUnix]);

  const SortHeader = ({
    label,
    col,
    alignRight = false,
  }: {
    label: string;
    col: ServerSortKey;
    alignRight?: boolean;
  }) => {
    const active = sort.key === col;
    return (
      <button
        type="button"
        onClick={() => handleSortClick(col)}
        aria-sort={
          active ? (sort.dir === "asc" ? "ascending" : "descending") : "none"
        }
        className={`flex items-center gap-1 cursor-pointer select-none transition-colors hover:text-[var(--text-primary)] rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] ${
          alignRight ? "justify-end w-full" : ""
        } ${active ? "text-[var(--text-primary)]" : "text-[var(--text-muted)]"}`}
      >
        <span>{label}</span>
        {active &&
          (sort.dir === "asc" ? (
            <ArrowUp className="w-3 h-3 text-[var(--accent)]" />
          ) : (
            <ArrowDown className="w-3 h-3 text-[var(--accent)]" />
          ))}
      </button>
    );
  };

  const renderEmptyState = () => {
    if (hasActiveFilters) {
      return (
        <EmptyState
          icon={<Globe className="w-5 h-5" />}
          title="No Matching Servers"
          description="No servers match your current search or filter pills."
          action={
            <Button variant="secondary" size="sm" onClick={clearFilters}>
              Clear Filters
            </Button>
          }
        />
      );
    }
    if (tab === "watchlist") {
      return (
        <EmptyState
          icon={<Star className="w-5 h-5" />}
          title="Your Watchlist is Empty"
          description="Star any server in the Online directory to keep an eye on its live player count and track."
          action={
            <Button variant="secondary" size="sm" onClick={() => setTab("online")}>
              Browse Online
            </Button>
          }
        />
      );
    }
    if (tab === "recent") {
      return (
        <EmptyState
          icon={<Clock className="w-5 h-5" />}
          title="No Recent Servers"
          description="Servers you join will appear here for quick one-click reconnect."
          action={
            <Button variant="secondary" size="sm" onClick={() => setTab("online")}>
              Browse Online
            </Button>
          }
        />
      );
    }
    if (tab === "lan") {
      return (
        <EmptyState
          icon={<Wifi className="w-5 h-5" />}
          title="No LAN Servers Found"
          description="No dedicated Assetto Corsa servers were detected on your local network."
          action={
            <Button variant="secondary" size="sm" onClick={() => void scanLanServers()}>
              Scan LAN
            </Button>
          }
        />
      );
    }
    return (
      <EmptyState
        icon={<Globe className="w-5 h-5" />}
        title="Directory Unavailable"
        description="Could not load public servers from the directory. Check your connection and retry."
        action={
          <Button variant="secondary" size="sm" onClick={() => void refreshOnlineDirectory()}>
            Retry Refresh
          </Button>
        }
      />
    );
  };

  const hasMissingContent = Boolean(
    activeDetail && activeDetail.missingContent.length > 0
  );
  const isSlotWatched = Boolean(selectedServer && watchedSlots[selectedServer.id]);

  return (
    <div className="h-full flex flex-col overflow-hidden px-4 xl:px-6 py-3.5 gap-3 animate-fade-in">
      {/* Top Sleek Command & Overhauled Filter Bar */}
      <div className="shrink-0 glass-panel rounded-xl px-3.5 py-2.5 space-y-2">
        {/* Row 1: Navigation Tabs, Search/Invite, Status & Refresh */}
        <div className="flex flex-wrap items-center justify-between gap-2.5">
          <div className="flex flex-wrap items-center gap-2.5">
            <SegmentedControl
              size="sm"
              value={tab}
              onChange={handleTabChange}
              options={[
                {
                  value: "online",
                  label: tabCounts.online > 0 ? `Online (${tabCounts.online})` : "Online",
                  icon: <Globe className="w-3.5 h-3.5" />,
                },
                {
                  value: "watchlist",
                  label: tabCounts.watchlist > 0 ? `Watchlist (${tabCounts.watchlist})` : "Watchlist",
                  icon: <Star className="w-3.5 h-3.5" />,
                },
                {
                  value: "recent",
                  label: tabCounts.recent > 0 ? `Recent (${tabCounts.recent})` : "Recent",
                  icon: <Clock className="w-3.5 h-3.5" />,
                },
                {
                  value: "lan",
                  label: tabCounts.lan > 0 ? `LAN (${tabCounts.lan})` : "LAN",
                  icon: <Wifi className="w-3.5 h-3.5" />,
                },
              ]}
            />

            {/* Instant Search + Clear + acmanager:// Invite Paste */}
            <div className="relative flex items-center">
              <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-3 pointer-events-none" />
              <input
                type="text"
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && isInviteUri) void handleOpenInviteUri();
                }}
                aria-label="Search servers, tracks, cars, players, or paste invite URI"
                placeholder="Search server, track, car, driver, or acmanager://..."
                className="h-8 w-64 xl:w-96 pl-8 pr-7 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)] focus-visible:ring-2 focus-visible:ring-[var(--accent)]/60"
              />
              {searchInput && (
                <button
                  type="button"
                  onClick={() => setSearchInput("")}
                  aria-label="Clear server search"
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-white cursor-pointer"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              )}
              {isInviteUri && (
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Link2 className="w-3 h-3" />}
                  onClick={handleOpenInviteUri}
                  className="ml-1.5 h-8 shrink-0"
                >
                  Open Invite
                </Button>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2">
            {statusPillText && (
              <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-white/[0.04] border border-white/[0.08] text-[11px] font-mono text-[var(--text-secondary)] whitespace-nowrap">
                {(onlineStreamProgress?.loading || remoteSearchLoading) && (
                  <span className="w-1.5 h-1.5 rounded-full bg-[var(--accent)] animate-pulse" />
                )}
                {statusPillText}
              </span>
            )}

            <Badge variant="neutral" className="font-mono tabular-nums whitespace-nowrap">
              {filteredServers.length === rawTabServers.length
                ? `${filteredServers.length} servers`
                : `${filteredServers.length} / ${rawTabServers.length}`}
            </Badge>

            <Button
              variant="ghost"
              size="sm"
              loading={
                tab === "online"
                  ? Boolean(onlineStreamProgress?.loading || activeTabLoading)
                  : activeTabLoading
              }
              icon={<RefreshCw className="w-3.5 h-3.5" />}
              onClick={handleRefreshActiveTab}
              className="h-7 px-2 shrink-0"
              aria-label="Refresh server list"
              title={`Refresh ${tab} servers`}
            />
          </div>
        </div>

        {/* Row 2: Overhauled Content Ownership + Lobby + Region/CSP/Session Filter Bar */}
        <div className="flex flex-wrap items-center justify-between gap-2 pt-2 border-t border-white/[0.06]">
          {/* Content Ownership Filter Group */}
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="inline-flex items-center gap-1 text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)] mr-0.5">
              <Filter className="w-3 h-3 text-[var(--accent)]" />
              Content
            </span>
            {(
              [
                { id: "all", label: "All" },
                { id: "ready", label: "Ready to Join" },
                { id: "full", label: "All Cars Owned" },
                { id: "have-track", label: "Have Track" },
                { id: "missing", label: "Missing Content" },
              ] as Array<{ id: ContentOwnershipFilter; label: string }>
            ).map((opt) => {
              const active = contentFilter === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  aria-pressed={active}
                  onClick={() => setContentFilter(opt.id)}
                  className={`h-7 px-2.5 rounded-lg text-[11px] font-medium whitespace-nowrap border transition-all cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] ${
                    active
                      ? opt.id === "missing"
                        ? "bg-amber-500/15 text-amber-300 border-amber-500/35"
                        : "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)] font-semibold"
                      : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)] hover:bg-white/[0.06]"
                  }`}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>

          {/* Access Toggles + Region / CSP / Session / Min Players Selects */}
          <div className="flex flex-wrap items-center gap-1.5">
            {[
              {
                label: "Has Players",
                active: hideEmpty || minPlayers >= 1,
                toggle: () => {
                  if (hideEmpty || minPlayers >= 1) {
                    setHideEmpty(false);
                    setMinPlayers(0);
                  } else {
                    setHideEmpty(true);
                    setMinPlayers(1);
                  }
                },
              },
              {
                label: "Open Slots",
                active: hideFull,
                toggle: () => setHideFull(!hideFull),
              },
              {
                label: "No Password",
                active: hidePassworded,
                toggle: () => setHidePassworded(!hidePassworded),
              },
            ].map((pill) => (
              <button
                key={pill.label}
                type="button"
                aria-pressed={pill.active}
                onClick={pill.toggle}
                className={`h-7 px-2.5 rounded-lg text-[11px] font-medium whitespace-nowrap border transition-all cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] ${
                  pill.active
                    ? "bg-emerald-500/15 text-emerald-300 border-emerald-500/35"
                    : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
                }`}
              >
                {pill.label}
              </button>
            ))}

            <select
              value={minPlayers}
              onChange={(e) => {
                const v = Number(e.target.value);
                setMinPlayers(v);
                setHideEmpty(v > 0);
              }}
              aria-label="Minimum active players"
              className="h-7 px-2 rounded-lg bg-black/50 border border-white/[0.09] text-[11px] font-mono text-[var(--text-secondary)] outline-none focus:border-[var(--accent-border)] cursor-pointer min-w-0 max-w-full"
            >
              <option value={0}>Players: Any</option>
              <option value={1}>1+ Players</option>
              <option value={5}>5+ Active</option>
              <option value={15}>15+ Busy</option>
            </select>

            <select
              value={regionFilter}
              onChange={(e) => setRegionFilter(e.target.value)}
              aria-label="Filter by region"
              className="h-7 px-2 rounded-lg bg-black/50 border border-white/[0.09] text-[11px] font-mono text-[var(--text-secondary)] outline-none focus:border-[var(--accent-border)] cursor-pointer"
            >
              <option value="all">Region: All</option>
              {availableRegions.map((reg) => (
                <option key={reg} value={reg}>
                  {reg}
                </option>
              ))}
            </select>

            <select
              value={cspFilter}
              onChange={(e) => setCspFilter(e.target.value as CspRequirementFilter)}
              aria-label="Filter by CSP requirement"
              className="h-7 px-2 rounded-lg bg-black/50 border border-white/[0.09] text-[11px] font-mono text-[var(--text-secondary)] outline-none focus:border-[var(--accent-border)] cursor-pointer"
            >
              <option value="all">CSP: Any</option>
              <option value="compatible">CSP Compatible</option>
              <option value="required">CSP Required</option>
              <option value="vanilla">No CSP Req</option>
            </select>

            <select
              value={sessionFilter}
              onChange={(e) => setSessionFilter(e.target.value)}
              aria-label="Filter by session type"
              className="h-7 px-2 rounded-lg bg-black/50 border border-white/[0.09] text-[11px] font-mono text-[var(--text-secondary)] outline-none focus:border-[var(--accent-border)] cursor-pointer"
            >
              <option value="all">Session: All</option>
              <option value="Practice">Practice</option>
              <option value="Qualifying">Qualifying</option>
              <option value="Race">Race</option>
              <option value="Booking">Booking</option>
            </select>

            {hasActiveFilters && (
              <button
                type="button"
                onClick={clearFilters}
                className="h-7 px-2.5 rounded-lg bg-red-500/15 border border-red-500/30 text-[11px] font-mono text-red-300 hover:bg-red-500/25 inline-flex items-center gap-1 whitespace-nowrap cursor-pointer"
              >
                <RotateCcw className="w-3 h-3" />
                Reset ({activeFilterCount})
              </button>
            )}
          </div>
        </div>
      </div>

      {/* =================================================================== */}
      {/* Responsive 2-Pane Layout                                            */}
      {/* =================================================================== */}
      <div className="flex flex-col lg:flex-row gap-3 xl:gap-4 flex-1 min-h-0 overflow-hidden">
        {/* LEFT PANE: Server Table */}
        <div className="flex-1 min-w-0 min-h-0 glass-panel rounded-2xl flex flex-col overflow-hidden">
          {/* Clickable Sortable Table Header */}
          <div className="shrink-0 grid grid-cols-[24px_minmax(150px,1.55fr)_minmax(135px,1.1fr)_88px_84px_48px] gap-2 items-center px-3 py-2.5 border-b border-white/[0.07] border-l-2 border-l-transparent bg-black/25 text-[10px] font-mono uppercase tracking-[0.12em]">
            <span />
            <SortHeader label="Server" col="name" />
            <SortHeader label="Circuit" col="track" />
            <SortHeader label="Cars" col="content" />
            <SortHeader label="Capacity" col="clients" />
            <SortHeader label="Ping" col="ping" alignRight />
          </div>

          {/* Scrollable Server Rows */}
          <div className="flex-1 min-h-0 overflow-y-auto divide-y divide-white/[0.04]">
            {showSkeletons ? (
              <div className="p-4 space-y-2">
                {Array.from({ length: 8 }).map((_, i) => (
                  <Skeleton key={i} className="h-12 rounded-xl" />
                ))}
              </div>
            ) : filteredServers.length === 0 ? (
              <div className="p-8">{renderEmptyState()}</div>
            ) : (
              filteredServers.map((s) => {
                const selected = selectedServer?.id === s.id;
                const fav = isFavorite(s);
                const ev = ownershipById.get(s.id);
                const trackName =
                  ev?.matchedTrack?.name ?? findTrackMeta(s.trackId)?.name ?? s.trackId;
                const fillPct =
                  s.maxClients > 0
                    ? Math.min(100, Math.round((s.clients / s.maxClients) * 100))
                    : 0;
                const readyDot =
                  ev?.readiness === "full" || ev?.readiness === "ready"
                    ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.7)]"
                    : ev?.readiness === "missing-cars" || ev?.readiness === "missing-track"
                      ? "bg-amber-400"
                      : ev?.readiness === "missing-both"
                        ? "bg-red-400"
                        : "bg-slate-400/70";
                const missingSomething =
                  ev?.trackState === "missing" || ev?.carState === "none";

                return (
                  <div
                    key={s.id}
                    onContextMenu={(e) => {
                      const httpPort = s.httpPort ?? s.port ?? 8081;
                      const inviteUri = `acmanager://race/online/join?ip=${s.host}&httpPort=${httpPort}`;
                      openContextMenu(e, [
                        {
                          id: "server-inspect",
                          label: "Inspect Server",
                          icon: <Globe className="w-3.5 h-3.5" />,
                          onSelect: () => setSelectedServer(s),
                        },
                        {
                          id: "server-join",
                          label: "Join Server",
                          icon: <Play className="w-3.5 h-3.5" />,
                          onSelect: () => {
                            setSelectedServer(s);
                            void handleJoin();
                          },
                        },
                        {
                          id: "server-watchlist",
                          label: fav ? "Remove from Watchlist" : "Add to Watchlist",
                          icon: <Star className="w-3.5 h-3.5" />,
                          divider: true,
                          onSelect: () => void toggleWatchlistServer(s),
                        },
                        {
                          id: "server-copy-invite",
                          label: "Copy Server IP / Invite",
                          icon: <Copy className="w-3.5 h-3.5" />,
                          onSelect: () => {
                            if (navigator.clipboard?.writeText) {
                              void navigator.clipboard.writeText(inviteUri);
                              notify("Copied invite link", "success", `${s.host}:${httpPort}`);
                            }
                          },
                        },
                      ]);
                    }}
                    className={`grid grid-cols-[24px_minmax(150px,1.55fr)_minmax(135px,1.1fr)_88px_84px_48px] gap-2 items-center px-3 py-2.5 transition-all border-l-2 ${
                      selected
                        ? "bg-[var(--accent-soft)]/70 border-l-[var(--accent)]"
                        : "border-l-transparent hover:bg-white/[0.03]"
                    }`}
                  >
                    {/* Favorite Star */}
                    <button
                      type="button"
                      onClick={() => void toggleWatchlistServer(s)}
                      aria-label={
                        fav
                          ? `Remove ${s.name} from watchlist`
                          : `Add ${s.name} to watchlist`
                      }
                      aria-pressed={fav}
                      className="flex items-center justify-center w-6 h-6 rounded-md text-[var(--text-muted)] hover:text-amber-300 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
                      title={fav ? "Remove from Watchlist" : "Add to Watchlist"}
                    >
                      <Star
                        className={`w-3.5 h-3.5 ${
                          fav ? "fill-amber-400 text-amber-400" : ""
                        }`}
                      />
                    </button>

                    {/* Selectable Server Row Content */}
                    <button
                      type="button"
                      onClick={() => setSelectedServer(s)}
                      aria-current={selected ? "true" : undefined}
                      className="col-span-5 grid grid-cols-[minmax(150px,1.55fr)_minmax(135px,1.1fr)_88px_84px_48px] gap-2 items-center text-left min-w-0 cursor-pointer rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
                    >
                      {/* Server Name & Status */}
                      <div className="min-w-0">
                        <div className="flex items-center gap-2 min-w-0">
                          <span
                            className={`w-2 h-2 rounded-full shrink-0 ${readyDot}`}
                            title={
                              ev?.readiness === "full"
                                ? "Ready — Track & All Cars Owned"
                                : ev?.readiness === "ready"
                                  ? "Ready — Track & Some Cars Owned"
                                  : ev?.readiness === "missing-cars"
                                    ? "Missing Cars"
                                    : ev?.readiness === "missing-track"
                                      ? "Missing Track"
                                      : ev?.readiness === "missing-both"
                                        ? "Missing Track & Cars"
                                        : "Content Unknown"
                            }
                          />
                          <span
                            className={`text-[13px] font-semibold truncate ${
                              missingSomething
                                ? "text-[var(--text-secondary)]"
                                : "text-[var(--text-primary)]"
                            }`}
                          >
                            {s.name}
                          </span>
                          {s.passworded && (
                            <Lock className="w-3 h-3 text-amber-400 shrink-0" />
                          )}
                        </div>
                        <div className="flex items-center gap-2 mt-0.5 pl-4 text-[10px] font-mono text-[var(--text-muted)] overflow-hidden">
                          {s.sessionType && (
                            <span className="whitespace-nowrap shrink-0">
                              {s.sessionType}
                            </span>
                          )}
                          {s.cspRequired && (
                            <span className="text-cyan-400/80 whitespace-nowrap shrink-0">
                              CSP {s.cspRequired}+
                            </span>
                          )}
                          {s.country && (
                            <span className="whitespace-nowrap shrink-0">
                              {s.country}
                            </span>
                          )}
                          {s.region && s.region !== s.country && (
                            <span className="whitespace-nowrap shrink-0 hidden xl:inline opacity-75">
                              {s.region}
                            </span>
                          )}
                        </div>
                      </div>

                      {/* Circuit & Immediate Track Ownership */}
                      <div className="min-w-0 space-y-0.5">
                        <div className="text-xs font-medium text-[var(--text-secondary)] truncate">
                          {trackName}
                        </div>
                        <div className="flex items-center gap-1.5 min-w-0">
                          {ev?.trackState === "owned" ? (
                            <span className="inline-flex items-center gap-0.5 px-1.5 py-0 rounded bg-emerald-500/12 border border-emerald-500/30 text-[9.5px] font-mono text-emerald-300 whitespace-nowrap shrink-0">
                              <Check className="w-2.5 h-2.5" />
                              Track
                            </span>
                          ) : ev?.trackState === "missing" ? (
                            <span className="inline-flex items-center gap-0.5 px-1.5 py-0 rounded bg-red-500/12 border border-red-500/30 text-[9.5px] font-mono text-red-300 whitespace-nowrap shrink-0">
                              <X className="w-2.5 h-2.5" />
                              No Track
                            </span>
                          ) : (
                            <span className="text-[9.5px] font-mono text-[var(--text-muted)] whitespace-nowrap">
                              Track ?
                            </span>
                          )}
                          {s.trackLayout && (
                            <span className="text-[9.5px] font-mono text-[var(--text-muted)] truncate">
                              {s.trackLayout}
                            </span>
                          )}
                        </div>
                      </div>

                      {/* Immediate Car Ownership Column */}
                      <div className="min-w-0">
                        {ev?.carState === "all" ? (
                          <span
                            title={`All ${ev.totalCars} car(s) installed`}
                            className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-emerald-500/15 border border-emerald-500/30 text-[10.5px] font-mono text-emerald-300 tabular-nums whitespace-nowrap"
                          >
                            <Check className="w-3 h-3 shrink-0" />
                            {ev.ownedCars}/{ev.totalCars}
                          </span>
                        ) : ev?.carState === "partial" ? (
                          <span
                            title={`${ev.ownedCars} of ${ev.totalCars} cars installed`}
                            className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-amber-500/15 border border-amber-500/30 text-[10.5px] font-mono text-amber-300 tabular-nums whitespace-nowrap"
                          >
                            <Car className="w-3 h-3 shrink-0" />
                            {ev.ownedCars}/{ev.totalCars}
                          </span>
                        ) : ev?.carState === "none" ? (
                          <span
                            title={`Missing all ${ev.totalCars} cars`}
                            className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-red-500/15 border border-red-500/30 text-[10.5px] font-mono text-red-300 tabular-nums whitespace-nowrap"
                          >
                            <X className="w-3 h-3 shrink-0" />
                            0/{ev.totalCars}
                          </span>
                        ) : (
                          <span
                            title="Car list not published by directory"
                            className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-white/[0.04] border border-white/[0.07] text-[10.5px] font-mono text-[var(--text-muted)] whitespace-nowrap"
                          >
                            —
                          </span>
                        )}
                      </div>

                      {/* Player Capacity Fill Bar */}
                      <div className="space-y-1">
                        <div className="flex items-center justify-between text-[11px] font-mono tabular-nums whitespace-nowrap">
                          <span className="text-[var(--text-primary)] font-medium">
                            {s.clients}
                            <span className="text-[var(--text-muted)]">
                              /{s.maxClients}
                            </span>
                          </span>
                          <span className="text-[10px] text-[var(--text-muted)]">
                            {fillPct}%
                          </span>
                        </div>
                        <div className="h-1.5 rounded-full bg-white/[0.08] overflow-hidden">
                          <div
                            className={`h-full rounded-full transition-all duration-300 ${capacityBarColor(
                              s.clients,
                              s.maxClients
                            )}`}
                            style={{ width: `${fillPct}%` }}
                          />
                        </div>
                      </div>

                      {/* Color-Coded Mono Ping Pill */}
                      <div className="flex justify-end">
                        <span
                          className={`inline-flex items-center px-1.5 py-0.5 rounded-md border font-mono text-[10.5px] tabular-nums whitespace-nowrap shrink-0 ${pingStyle(
                            s.pingMs
                          )}`}
                        >
                          {s.pingMs != null ? `${s.pingMs}ms` : "—"}
                        </span>
                      </div>
                    </button>
                  </div>
                );
              })
            )}
          </div>

          {/* Sleek Table Footer Bar */}
          <div className="shrink-0 px-4 py-2 border-t border-white/[0.07] bg-black/25 flex flex-wrap items-center justify-between gap-2 text-[11px] font-mono text-[var(--text-muted)]">
            <span className="tabular-nums">
              Showing {filteredServers.length}
              {filteredServers.length !== rawTabServers.length
                ? ` of ${rawTabServers.length}`
                : ""}{" "}
              server{filteredServers.length === 1 ? "" : "s"}
            </span>
            <div className="flex items-center gap-3">
              {statusPillText && <span>{statusPillText}</span>}
              <span className="opacity-75">Directory: acservers.online</span>
            </div>
          </div>
        </div>

        {/* ================================================================= */}
        {/* RIGHT PANE: Unified Glass Server Inspector                        */}
        {/* ================================================================= */}
        <aside className="w-full lg:w-[364px] xl:w-[448px] shrink-0 max-h-[58vh] lg:max-h-none glass-panel rounded-2xl flex flex-col overflow-y-auto lg:overflow-hidden [@media(max-height:740px)]:overflow-y-auto">
          {!liveSummary ? (
            <div className="flex-1 flex items-center justify-center p-6">
              <EmptyState
                icon={<Globe className="w-5 h-5" />}
                title="Select a Server"
                description="Choose any lobby from the table to inspect cars, drivers, liveries, and join."
                className="border-0 bg-transparent shadow-none"
              />
            </div>
          ) : (
            <>
              {/* Pinned Compact Hero & Join Bar */}
              <div className="shrink-0">
                {/* Atmospheric Track Hero Header */}
                <div
                  className={`relative overflow-hidden border-b border-white/[0.08] ${
                    heroTrackImg ? "min-h-[68px] xl:min-h-[92px]" : "min-h-[64px] xl:min-h-[78px]"
                  }`}
                >
                  {heroTrackImg ? (
                    <img
                      src={heroTrackImg}
                      alt={selectedTrackMeta?.name ?? liveSummary.trackId}
                      className="absolute inset-0 w-full h-full object-cover scale-105 blur-[1.5px] opacity-65"
                    />
                  ) : (
                    <div
                      className="absolute inset-0 w-full h-full"
                      style={{
                        background:
                          "radial-gradient(360px 180px at 80% 15%, rgba(var(--accent-rgb), 0.28), transparent 65%), radial-gradient(280px 160px at 15% 90%, rgba(0, 209, 255, 0.16), transparent 70%), linear-gradient(135deg, #121A29 0%, #090D14 100%)",
                      }}
                    />
                  )}
                  <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-[#0D1118]/75 to-[#0D1118]/30" />

                  <div className="relative p-2 xl:p-2.5 flex flex-col justify-between gap-1 xl:gap-1.5 min-h-[inherit]">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <Badge variant="success" dot className="whitespace-nowrap">
                          <span className="font-mono tabular-nums">
                            {liveSummary.clients}/{liveSummary.maxClients}
                          </span>
                        </Badge>
                        {liveSummary.sessionType && (
                          <Badge variant="neutral" className="whitespace-nowrap">
                            {liveSummary.sessionType}
                          </Badge>
                        )}
                        {activeDetail?.minCspBuild && (
                          <Badge variant="info" className="whitespace-nowrap">
                            CSP ≥ {activeDetail.minCspBuild}
                          </Badge>
                        )}
                      </div>
                      <span
                        className={`px-2 py-0.5 rounded-md border font-mono text-[11px] tabular-nums whitespace-nowrap shrink-0 ${pingStyle(
                          liveSummary.pingMs
                        )}`}
                      >
                        {liveSummary.pingMs != null
                          ? `${liveSummary.pingMs} ms`
                          : "—"}
                      </span>
                    </div>

                    <div>
                      <h2 className="font-display text-base xl:text-lg font-bold uppercase tracking-wide text-white leading-tight line-clamp-1">
                        {liveSummary.name}
                      </h2>
                      <div className="flex items-center justify-between gap-2 mt-0.5 text-xs">
                        <span className="text-[var(--text-secondary)] truncate">
                          {selectedTrackMeta?.name ?? liveSummary.trackId}
                          {liveSummary.trackLayout
                            ? ` · ${liveSummary.trackLayout}`
                            : ""}
                        </span>
                        <span className="font-mono text-[10px] text-[var(--text-muted)] shrink-0 whitespace-nowrap">
                          {liveSummary.host}:{liveSummary.port}
                        </span>
                      </div>
                    </div>
                  </div>
                </div>

                {/* Action Bar (Race As + Password + Join / Book + Utilities) */}
                <div className="p-2 xl:p-2.5 border-b border-white/[0.07] space-y-1 xl:space-y-1.5 bg-black/25">
                  <div
                    className={`grid gap-1.5 ${
                      liveSummary.passworded ? "grid-cols-2" : "grid-cols-1"
                    }`}
                  >
                    <div className="relative flex items-center">
                      <User className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-2.5 pointer-events-none" />
                      <input
                        type="text"
                        value={driverName}
                        onChange={(e) => {
                          const val = e.target.value;
                          setDriverName(val);
                          try {
                            localStorage.setItem(DRIVER_NAME_KEY, val);
                          } catch {
                            // ignore storage errors
                          }
                        }}
                        aria-label="Race as driver name"
                        placeholder="Race as (driver name)..."
                        className="w-full h-7 xl:h-8 pl-8 pr-2.5 rounded-lg bg-black/55 border border-white/[0.1] text-xs text-white placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)] focus-visible:ring-2 focus-visible:ring-[var(--accent)]/60"
                      />
                    </div>
                    {liveSummary.passworded && (
                      <div className="relative flex items-center">
                        <Lock className="w-3.5 h-3.5 text-amber-400 absolute left-2.5 pointer-events-none" />
                        <input
                          type="password"
                          value={password}
                          onChange={(e) => setPassword(e.target.value)}
                          aria-label="Server password"
                          placeholder="Password required..."
                          className="w-full h-7 xl:h-8 pl-8 pr-2.5 rounded-lg bg-black/55 border border-amber-500/30 text-xs text-white placeholder:text-[var(--text-muted)] outline-none focus:border-amber-400 focus-visible:ring-2 focus-visible:ring-[var(--accent)]/60"
                        />
                      </div>
                    )}
                  </div>

                  {canJoin ? (
                    <Button
                      variant="primary"
                      size="lg"
                      loading={joining}
                      icon={<Play className="w-4 h-4 fill-current" />}
                      onClick={() => void handleJoin()}
                      className="w-full font-display text-sm xl:text-base uppercase tracking-wider h-8 xl:h-9"
                    >
                      {activeDetail?.bookingMode ? "Book Slot (/sub)" : "Join Server"}
                    </Button>
                  ) : (
                    <div className="rounded-lg bg-amber-500/[0.07] border border-amber-500/25 px-2 py-1 flex items-center justify-between gap-2">
                      <span className="flex items-center gap-1.5 text-[11px] font-medium text-[var(--text-secondary)] shrink-0">
                        <ShieldAlert className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                        Cannot Join Yet
                      </span>
                      <div className="flex flex-wrap items-center justify-end gap-1 min-w-0">
                        {joinBlockers.map((reason) => (
                          <Badge key={reason} variant="warning">
                            {reason}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}

                  <div
                    className={`grid gap-1.5 ${
                      hasMissingContent ? "grid-cols-3" : "grid-cols-2"
                    }`}
                  >
                    {hasMissingContent && activeDetail && (
                      <Button
                        variant="secondary"
                        size="sm"
                        icon={<Download className="w-3.5 h-3.5" />}
                        onClick={handleDownloadMissing}
                        className="text-[11px] px-2 whitespace-nowrap h-7"
                      >
                        Missing ({activeDetail.missingContent.length})
                      </Button>
                    )}
                    <Button
                      variant="secondary"
                      size="sm"
                      icon={<Copy className="w-3.5 h-3.5" />}
                      onClick={() => void handleCopyInvite()}
                      className="text-[11px] px-2 whitespace-nowrap h-7"
                    >
                      Copy Invite
                    </Button>
                    <Button
                      variant={isSlotWatched ? "primary" : "secondary"}
                      size="sm"
                      icon={
                        isSlotWatched ? (
                          <BellRing className="w-3.5 h-3.5" />
                        ) : (
                          <Bell className="w-3.5 h-3.5" />
                        )
                      }
                      onClick={() => toggleWatchSlot(liveSummary)}
                      className="text-[11px] px-2 whitespace-nowrap h-7"
                    >
                      {isSlotWatched ? "Watching" : "Watch Slot"}
                    </Button>
                  </div>
                </div>
              </div>

              {/* Unified Scrollable Cockpit: Cars & Drivers -> Liveries -> Active Drivers -> Info */}
              <div className="flex-1 min-h-[240px] overflow-y-auto p-3 space-y-4">
                {loadingDetail ? (
                  <div className="space-y-2">
                    <Skeleton className="h-8 rounded-lg" />
                    <div className="grid grid-cols-2 gap-2">
                      <Skeleton className="h-28 rounded-xl" />
                      <Skeleton className="h-28 rounded-xl" />
                      <Skeleton className="h-28 rounded-xl" />
                      <Skeleton className="h-28 rounded-xl" />
                    </div>
                  </div>
                ) : (
                  <>
                    {/* SECTION 1: Cars & Live Drivers */}
                    <section className="space-y-2.5">
                      <div className="flex items-center justify-between gap-2">
                        <span className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-[0.12em] text-[var(--text-secondary)] font-semibold">
                          <Car className="w-3.5 h-3.5 text-[var(--accent)]" />
                          Cars &amp; Live Drivers ({allGroupedCars.length})
                        </span>
                        <span className="text-[10px] font-mono text-[var(--text-muted)]">
                          Double-click car to join
                        </span>
                      </div>

                      {/* Search + Non-blocking Filter Pills */}
                      <div className="space-y-1.5">
                        <div className="relative">
                          <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
                          <input
                            type="text"
                            value={carSearch}
                            onChange={(e) => setCarSearch(e.target.value)}
                            aria-label="Filter available cars or drivers"
                            placeholder="Filter cars or connected drivers..."
                            className="w-full h-8 pl-8 pr-3 rounded-lg bg-black/45 border border-white/[0.09] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)] focus-visible:ring-2 focus-visible:ring-[var(--accent)]/60"
                          />
                        </div>
                        <div className="flex flex-wrap items-center gap-1">
                          {(
                            [
                              { id: "all", label: "All" },
                              { id: "with-drivers", label: "With Drivers" },
                              { id: "free", label: "Free Slots" },
                              { id: "installed", label: "Installed" },
                            ] as Array<{ id: CarFilter; label: string }>
                          ).map((pill) => (
                            <button
                              key={pill.id}
                              type="button"
                              aria-pressed={carFilter === pill.id}
                              onClick={() => setCarFilter(pill.id)}
                              className={`h-6 px-2 rounded-md text-[10px] font-medium border transition-colors cursor-pointer ${
                                carFilter === pill.id
                                  ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                                  : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-white"
                              }`}
                            >
                              {pill.label}
                            </button>
                          ))}
                        </div>
                      </div>

                      {/* 2-Column Car Cards with Inline Driver Pills */}
                      {groupedCars.length === 0 ? (
                        <div className="rounded-xl bg-white/[0.03] border border-white/[0.07] p-4 text-center space-y-2">
                          <div className="text-xs font-medium text-[var(--text-secondary)]">
                            {carSearch.trim() || carFilter !== "all"
                              ? "No cars match your filter."
                              : "No car slots reported by this server."}
                          </div>
                          {(carSearch.trim() || carFilter !== "all") && (
                            <Button
                              variant="secondary"
                              size="sm"
                              onClick={() => {
                                setCarSearch("");
                                setCarFilter("all");
                              }}
                            >
                              Reset Car Filter
                            </Button>
                          )}
                        </div>
                      ) : (
                        <div className="grid grid-cols-2 gap-2">
                          {groupedCars.map((group) => {
                            const active = selectedCarId === group.carId;
                            const img = assetUrl(group.previewPath);
                            const driversOnCar = group.slots.filter((s) => s.connected);
                            const visibleDrivers = driversOnCar.slice(0, 3);
                            const extraDrivers = driversOnCar.length - visibleDrivers.length;

                            return (
                              <button
                                key={group.carId}
                                type="button"
                                onClick={() => selectCarGroup(group)}
                                onDoubleClick={() => {
                                  const nextSkin = pickPreferredSkin(group);
                                  selectCarGroup(group);
                                  const cspOk =
                                    !activeDetail?.minCspBuild ||
                                    localCspBuildNum === null ||
                                    localCspBuildNum >= activeDetail.minCspBuild;
                                  const serverFull =
                                    liveSummary.maxClients > 0 &&
                                    liveSummary.clients >= liveSummary.maxClients &&
                                    !activeDetail?.bookingMode;
                                  const groupReady =
                                    Boolean(activeDetail) &&
                                    !loadingDetail &&
                                    group.owned &&
                                    (group.freeSlots > 0 ||
                                      Boolean(activeDetail?.bookingMode)) &&
                                    Boolean(activeDetail?.trackOwned) &&
                                    cspOk &&
                                    !serverFull &&
                                    !gameRunning;
                                  if (groupReady) {
                                    void handleJoin(group.carId, nextSkin);
                                  }
                                }}
                                className={`text-left rounded-xl overflow-hidden border transition-all cursor-pointer group flex flex-col focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] ${
                                  active
                                    ? "border-[var(--accent)] bg-[var(--accent-soft)] shadow-[0_0_16px_-6px_var(--accent-glow)]"
                                    : "border-white/[0.08] bg-white/[0.03] hover:border-white/[0.18]"
                                }`}
                              >
                                <div className="relative h-14 bg-[#080B11] overflow-hidden shrink-0">
                                  {img ? (
                                    <img
                                      src={img}
                                      alt={group.carName}
                                      className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                                    />
                                  ) : (
                                    <div className="w-full h-full bg-gradient-to-br from-white/[0.07] to-transparent flex items-center justify-center">
                                      <Car className="w-4 h-4 text-white/25" />
                                    </div>
                                  )}
                                  <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-transparent to-transparent" />
                                  <span
                                    className={`absolute top-1 right-1 px-1.5 py-0.5 rounded text-[10px] font-mono font-semibold tabular-nums whitespace-nowrap ${
                                      group.freeSlots > 0
                                        ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/35"
                                        : "bg-red-500/20 text-red-300 border border-red-500/35"
                                    }`}
                                  >
                                    {group.freeSlots > 0
                                      ? `${group.freeSlots} free`
                                      : "Full"}
                                  </span>
                                  <span className="absolute top-1 left-1">
                                    {group.owned ? (
                                      <span className="inline-flex items-center justify-center w-4 h-4 rounded-full bg-emerald-500/20 border border-emerald-500/40 text-emerald-300">
                                        <Check className="w-2.5 h-2.5" />
                                      </span>
                                    ) : (
                                      <span className="inline-flex items-center justify-center w-4 h-4 rounded-full bg-amber-500/20 border border-amber-500/40 text-amber-300">
                                        <AlertTriangle className="w-2.5 h-2.5" />
                                      </span>
                                    )}
                                  </span>
                                </div>
                                <div className="p-1.5 space-y-1 flex-1 flex flex-col justify-between">
                                  <div>
                                    <div className="text-[11px] font-semibold text-[var(--text-primary)] truncate">
                                      {group.carName}
                                    </div>
                                    <div className="text-[10px] font-mono text-[var(--text-muted)] truncate">
                                      {group.owned ? "Installed" : "Missing"} ·{" "}
                                      {group.totalSlots} slot
                                      {group.totalSlots === 1 ? "" : "s"}
                                    </div>
                                  </div>
                                  {driversOnCar.length > 0 && (
                                    <div className="pt-1 border-t border-white/[0.06] space-y-0.5">
                                      {visibleDrivers.map((drv, dIdx) => (
                                        <div
                                          key={`${drv.driverName ?? "d"}-${dIdx}`}
                                          className="flex items-center justify-between gap-1 text-[10px] rounded px-1 py-0.5 bg-emerald-500/10 border border-emerald-500/20"
                                        >
                                          <span className="flex items-center gap-1 min-w-0 text-emerald-200 font-medium truncate">
                                            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />
                                            <span className="truncate">
                                              {drv.driverName || `Driver #${dIdx + 1}`}
                                            </span>
                                          </span>
                                          {drv.skinId && (
                                            <span className="font-mono text-[9px] text-[var(--text-muted)] truncate max-w-[54px] shrink-0">
                                              {drv.skinId}
                                            </span>
                                          )}
                                        </div>
                                      ))}
                                      {extraDrivers > 0 && (
                                        <div className="text-[9px] font-mono text-emerald-300/80 pl-1">
                                          +{extraDrivers} more driving
                                        </div>
                                      )}
                                    </div>
                                  )}
                                </div>
                              </button>
                            );
                          })}
                        </div>
                      )}

                      {/* Livery / Skin Selector */}
                      {selectedCarGroup && (() => {
                        const occupiedBySkin = new Map<string, string>();
                        for (const slot of selectedCarGroup.slots) {
                          if (slot.connected && slot.skinId) {
                            occupiedBySkin.set(
                              slot.skinId,
                              slot.driverName || "In use"
                            );
                          }
                        }
                        const availableSkins =
                          selectedCarGroup.skins.length > 0
                            ? selectedCarGroup.skins.map((id) => ({
                                id,
                                name:
                                  localSkins.find((s) => s.id === id)?.name ?? id,
                                takenBy: occupiedBySkin.get(id) ?? null,
                              }))
                            : localSkins.map((s) => ({
                                id: s.id,
                                name: s.name,
                                takenBy: occupiedBySkin.get(s.id) ?? null,
                              }));
                        return (
                          <div className="pt-2 border-t border-white/[0.06] space-y-1.5">
                            <div className="flex items-center justify-between text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                              <span>Livery / Skin</span>
                              <span className="truncate max-w-[180px] text-[var(--text-secondary)]">
                                {selectedCarGroup.carName}
                              </span>
                            </div>
                            {availableSkins.length === 0 ? (
                              <div className="text-[11px] font-mono text-[var(--text-muted)] py-1">
                                No liveries reported
                              </div>
                            ) : (
                              <div className="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto p-0.5">
                                {availableSkins.map((skin) => {
                                  const active = selectedSkinId === skin.id;
                                  return (
                                    <button
                                      key={skin.id}
                                      type="button"
                                      onClick={() => setSelectedSkinId(skin.id)}
                                      className={`px-2 py-1 rounded-lg text-[11px] whitespace-nowrap border transition-all cursor-pointer flex items-center gap-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] ${
                                        active
                                          ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)] font-semibold"
                                          : skin.takenBy
                                          ? "bg-amber-500/[0.07] text-amber-200/85 border-amber-500/25 hover:border-amber-400/50"
                                          : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-white"
                                      }`}
                                    >
                                      <span>{skin.name}</span>
                                      {skin.takenBy && (
                                        <span className="text-[9px] font-mono px-1 py-0.2 rounded bg-black/40 text-amber-300">
                                          Taken · {skin.takenBy}
                                        </span>
                                      )}
                                    </button>
                                  );
                                })}
                              </div>
                            )}
                          </div>
                        );
                      })()}
                    </section>

                    {/* SECTION 2: Active Drivers Roster */}
                    <section className="pt-3 border-t border-white/[0.07] space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-[0.12em] text-[var(--text-secondary)] font-semibold">
                          <Users className="w-3.5 h-3.5 text-emerald-400" />
                          Active Drivers ({connectedDrivers.length})
                        </span>
                        {connectedDrivers.length > 0 && (
                          <span className="text-[10px] font-mono text-[var(--text-muted)]">
                            Click to highlight car
                          </span>
                        )}
                      </div>

                      {connectedDrivers.length === 0 ? (
                        <div className="rounded-xl bg-white/[0.025] border border-white/[0.06] px-3 py-2.5 text-[11px] text-[var(--text-muted)]">
                          No active drivers in lobby — open slots ready.
                        </div>
                      ) : (
                        <div className="space-y-1">
                          {connectedDrivers.map((slot, idx) => {
                            const carGroup = allGroupedCars.find(
                              (g) => g.carId === slot.carId
                            );
                            const carName = carGroup?.carName ?? slot.carId;
                            const isSelectedCar = selectedCarId === slot.carId;
                            return (
                              <button
                                key={`${slot.driverName ?? "drv"}-${idx}`}
                                type="button"
                                onClick={() => {
                                  if (carGroup) selectCarGroup(carGroup);
                                  else setSelectedCarId(slot.carId);
                                }}
                                className={`w-full text-left flex items-center justify-between gap-2.5 px-2.5 py-2 rounded-xl border transition-all cursor-pointer ${
                                  isSelectedCar
                                    ? "bg-[var(--accent-soft)]/50 border-[var(--accent-border)]"
                                    : "bg-white/[0.03] border-white/[0.06] hover:bg-white/[0.06]"
                                }`}
                              >
                                <div className="flex items-center gap-2 min-w-0">
                                  <span className="w-2 h-2 rounded-full bg-emerald-400 shrink-0" />
                                  <div className="min-w-0">
                                    <div className="text-xs font-semibold text-[var(--text-primary)] truncate">
                                      {slot.driverName || `Driver #${idx + 1}`}
                                    </div>
                                    <div className="text-[10px] text-[var(--text-muted)] truncate">
                                      {carName}
                                    </div>
                                  </div>
                                </div>
                                <Badge variant="neutral" className="font-mono text-[10px] whitespace-nowrap shrink-0">
                                  {slot.skinId || "default"}
                                </Badge>
                              </button>
                            );
                          })}
                        </div>
                      )}
                    </section>

                    {/* SECTION 3: Server Info & Content Check */}
                    <section className="pt-3 border-t border-white/[0.07] space-y-2.5 text-xs">
                      <div className="flex items-center gap-1.5 text-[11px] font-mono uppercase tracking-[0.12em] text-[var(--text-secondary)] font-semibold">
                        <Info className="w-3.5 h-3.5 text-cyan-400" />
                        Server Info &amp; Content Check
                      </div>

                      {activeDetail?.description && (
                        <div className="rounded-xl bg-white/[0.03] border border-white/[0.07] p-2.5 text-[var(--text-secondary)] leading-relaxed">
                          {activeDetail.description}
                        </div>
                      )}

                      <div className="rounded-xl bg-white/[0.03] border border-white/[0.07] divide-y divide-white/[0.05]">
                        {[
                          {
                            k: "Address",
                            v: `${liveSummary.host}:${liveSummary.port}`,
                          },
                          {
                            k: "HTTP Port",
                            v: String(liveSummary.httpPort ?? "—"),
                          },
                          {
                            k: "Track Status",
                            v: activeDetail?.trackOwned ? "Installed" : "Missing",
                          },
                          {
                            k: "CSP Minimum",
                            v: activeDetail?.minCspBuild
                              ? `Build ${activeDetail.minCspBuild}`
                              : "None",
                          },
                          {
                            k: "Booking Mode",
                            v: activeDetail?.bookingMode ? "Booking (/sub)" : "Pickup",
                          },
                        ].map((row) => (
                          <div
                            key={row.k}
                            className="flex items-center justify-between px-3 py-1.5"
                          >
                            <span className="text-[var(--text-muted)]">{row.k}</span>
                            <span className="font-mono text-[var(--text-primary)]">
                              {row.v}
                            </span>
                          </div>
                        ))}
                      </div>

                      {activeDetail && activeDetail.missingContent.length > 0 && (
                        <div className="rounded-xl bg-amber-500/10 border border-amber-500/25 p-2.5 space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-semibold text-amber-300">
                              Missing Content ({activeDetail.missingContent.length})
                            </span>
                            <Button
                              variant="ghost"
                              size="sm"
                              icon={<ExternalLink className="w-3 h-3" />}
                              onClick={handleDownloadMissing}
                            >
                              Download
                            </Button>
                          </div>
                          <div className="space-y-1">
                            {activeDetail.missingContent.map((m) => (
                              <div
                                key={`${m.kind}-${m.id}`}
                                className="flex items-center justify-between gap-2 text-[11px] font-mono text-amber-200/90"
                              >
                                <span className="min-w-0 truncate">{m.id}</span>
                                <Badge variant="warning" className="shrink-0">
                                  {m.kind}
                                </Badge>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {activeDetail && activeDetail.missingContent.length === 0 && (
                        <div className="rounded-xl bg-emerald-500/10 border border-emerald-500/25 px-3 py-2 flex items-center gap-2 text-emerald-300">
                          <CheckCircle2 className="w-4 h-4 shrink-0" />
                          <span>All required cars and track are installed locally.</span>
                        </div>
                      )}
                    </section>
                  </>
                )}
              </div>
            </>
          )}
        </aside>
      </div>
    </div>
  );
}

export default ServersPage;

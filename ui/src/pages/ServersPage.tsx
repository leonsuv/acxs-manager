import React, { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Bell,
  BellRing,
  Car,
  Check,
  CheckCircle2,
  Clock,
  Copy,
  Download,
  ExternalLink,
  Globe,
  Info,
  Link2,
  Lock,
  Play,
  RefreshCw,
  Search,
  ShieldAlert,
  Star,
  Users,
  Wifi,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import type {
  OnlineServerDetail,
  OnlineServerSummary,
  ServerCarSlot,
  SkinEntry,
  WatchlistEntry,
} from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  EmptyState,
  SegmentedControl,
  Skeleton,
} from "../components/Primitives";

type ServerTab = "online" | "watchlist" | "recent" | "lan";
type InspectorTab = "cars" | "drivers" | "info";

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

function parseCspNumber(cspBuild: string | null): number | null {
  if (!cspBuild) return null;
  const match = cspBuild.match(/(\d{4,})/);
  if (match) return Number(match[1]);
  // Version string like "0.2.5-preview1" -> assume modern CSP >= 2500
  if (cspBuild.startsWith("0.2")) return 2800;
  if (cspBuild.startsWith("0.1.79")) return 2144;
  return 2000;
}

export function ServersPage() {
  const { cars, tracks, cspBuild, gameRunning, notify } = useAppStore();

  const [tab, setTab] = useState<ServerTab>("online");
  const [servers, setServers] = useState<OnlineServerSummary[]>([]);
  const [watchlist, setWatchlist] = useState<WatchlistEntry[]>([]);
  const [watchedSlots, setWatchedSlots] = useState<Record<string, boolean>>({});
  const [loadingList, setLoadingList] = useState(true);

  // Filters
  const [searchInput, setSearchInput] = useState("");
  const [onlyJoinable, setOnlyJoinable] = useState(false);
  const [hideEmpty, setHideEmpty] = useState(false);
  const [hideFull, setHideFull] = useState(false);
  const [hidePassworded, setHidePassworded] = useState(false);

  // Selected Server & Detail Inspector
  const [selectedServer, setSelectedServer] =
    useState<OnlineServerSummary | null>(null);
  const [detail, setDetail] = useState<OnlineServerDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("cars");
  const [carSearch, setCarSearch] = useState("");
  const [selectedCarId, setSelectedCarId] = useState<string | null>(null);
  const [selectedSkinId, setSelectedSkinId] = useState<string>("");
  const [localSkins, setLocalSkins] = useState<SkinEntry[]>([]);
  const [password, setPassword] = useState("");
  const [joining, setJoining] = useState(false);

  const ownedCarIds = useMemo(() => new Set(cars.map((c) => c.id)), [cars]);
  const ownedTrackIds = useMemo(() => new Set(tracks.map((t) => t.id)), [tracks]);
  const localCspBuildNum = useMemo(() => parseCspNumber(cspBuild), [cspBuild]);

  const loadServers = useCallback(
    async (activeTab: ServerTab) => {
      setLoadingList(true);
      try {
        const wl = await api.listWatchlist().catch(() => []);
        setWatchlist(wl ?? []);

        let list: OnlineServerSummary[] = [];
        if (activeTab === "online") {
          list = (await api.fetchDirectoryServers("", false)) ?? [];
        } else if (activeTab === "recent") {
          list = (await api.listRecentServers()) ?? [];
        } else if (activeTab === "lan") {
          list = (await api.scanLanServers()) ?? [];
        } else {
          list = (wl ?? []).map((w) => ({
            id: `${w.host}:${w.port}`,
            name: w.name,
            host: w.host,
            port: w.port,
            trackId: "unknown",
            clients: 0,
            maxClients: 24,
            cars: [],
            passworded: false,
          }));
        }
        setServers(list);
        if (list.length > 0) {
          setSelectedServer((prev) =>
            prev && list.some((s) => s.id === prev.id) ? prev : list[0]
          );
        } else {
          setSelectedServer(null);
          setDetail(null);
        }
      } catch (err) {
        notify("Failed to load servers", "error", String(err));
      } finally {
        setLoadingList(false);
      }
    },
    [notify]
  );

  useEffect(() => {
    void loadServers(tab);
  }, [tab, loadServers]);

  // Fetch server detail whenever selection changes
  useEffect(() => {
    if (!selectedServer) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setLoadingDetail(true);
    setPassword("");
    setCarSearch("");

    void api
      .fetchServerDetail(selectedServer.host, selectedServer.port)
      .then((res) => {
        if (cancelled) return;
        if (res) {
          setDetail(res);
          const firstOwned =
            res.slots.find((s) => s.owned && !s.connected) ??
            res.slots.find((s) => s.owned) ??
            res.slots[0];
          setSelectedCarId(firstOwned?.carId ?? null);
          setSelectedSkinId(firstOwned?.skinId ?? "");
        } else {
          // Synthesize rich detail in browser preview mode when backend returns null
          const trackOwned = ownedTrackIds.has(selectedServer.trackId);
          const synthSlots: ServerCarSlot[] = (
            selectedServer.cars.length > 0
              ? selectedServer.cars
              : ["ks_porsche_911_gt3_r_2016", "ks_ferrari_488_gt3"]
          ).flatMap((cid, idx) => [
            {
              carId: cid,
              skinId: "00_factory_white",
              driverName: idx === 0 ? "L. Rossi" : "K. Sato",
              connected: true,
              owned: ownedCarIds.has(cid),
            },
            {
              carId: cid,
              skinId: "01_obsidian_carbon",
              connected: false,
              owned: ownedCarIds.has(cid),
            },
            {
              carId: cid,
              skinId: "02_crimson_gt",
              connected: false,
              owned: ownedCarIds.has(cid),
            },
          ]);
          const missing: OnlineServerDetail["missingContent"] = [];
          if (!trackOwned) {
            missing.push({ kind: "track", id: selectedServer.trackId });
          }
          selectedServer.cars.forEach((cid) => {
            if (!ownedCarIds.has(cid)) missing.push({ kind: "car", id: cid });
          });

          const synth: OnlineServerDetail = {
            summary: selectedServer,
            resolvedTrackId: selectedServer.trackId,
            trackOwned,
            minCspBuild: selectedServer.cspRequired ?? null,
            slots: synthSlots,
            missingContent: missing,
            description:
              "Clean racing lobby with live timing, dynamic rubbering, and Sol/Pure WeatherFX sync.",
            bookingMode: false,
          };
          setDetail(synth);
          const first =
            synthSlots.find((s) => s.owned && !s.connected) ?? synthSlots[0];
          setSelectedCarId(first?.carId ?? null);
          setSelectedSkinId(first?.skinId ?? "");
        }
      })
      .catch(() => {
        if (!cancelled) setDetail(null);
      })
      .finally(() => {
        if (!cancelled) setLoadingDetail(false);
      });

    return () => {
      cancelled = true;
    };
  }, [selectedServer, ownedCarIds, ownedTrackIds]);

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
    void api
      .getCarSkins(car.path)
      .then((list) => setLocalSkins(list ?? []))
      .catch(() => setLocalSkins([]));
  }, [selectedCarId, cars]);

  // Handle acmanager:// or host:port paste
  const isInviteUri = useMemo(() => {
    const t = searchInput.trim();
    return (
      t.startsWith("acmanager://") ||
      /^\d{1,3}(?:\.\d{1,3}){3}:\d{2,5}$/.test(t)
    );
  }, [searchInput]);

  const handleOpenInviteUri = async () => {
    const raw = searchInput.trim();
    if (!raw) return;
    try {
      const parsed = await api.parseJoinUri(raw);
      const host = parsed?.host ?? raw.split(":")[0];
      const port = parsed?.port ?? (Number(raw.split(":")[1]) || 9600);
      const synthetic: OnlineServerSummary = {
        id: `${host}:${port}`,
        name: `Direct Connect · ${host}:${port}`,
        host,
        port,
        trackId: "unknown",
        clients: 0,
        maxClients: 24,
        cars: [],
        passworded: Boolean(parsed?.password),
      };
      if (parsed?.password) setPassword(parsed.password);
      setSelectedServer(synthetic);
      notify("Invite parsed", "info", `${host}:${port}`);
    } catch (err) {
      notify("Invalid invite link", "error", String(err));
    }
  };

  // Watchlist toggle
  const isFavorite = (s: OnlineServerSummary) =>
    watchlist.some((w) => w.host === s.host && w.port === s.port);

  const toggleFavorite = async (
    s: OnlineServerSummary,
    e?: React.MouseEvent
  ) => {
    e?.stopPropagation();
    const exists = isFavorite(s);
    const next = exists
      ? watchlist.filter((w) => !(w.host === s.host && w.port === s.port))
      : [
          { name: s.name, host: s.host, port: s.port, favorite: true },
          ...watchlist,
        ];
    setWatchlist(next);
    try {
      await api.saveWatchlist(next);
      notify(
        exists ? "Removed from watchlist" : "Added to watchlist",
        "info",
        s.name
      );
    } catch (err) {
      notify("Failed to update watchlist", "error", String(err));
    }
  };

  // Filtered server rows
  const filteredServers = useMemo(() => {
    const q = isInviteUri ? "" : searchInput.trim().toLowerCase();
    return servers.filter((s) => {
      if (hideEmpty && s.clients === 0) return false;
      if (hideFull && s.maxClients > 0 && s.clients >= s.maxClients) return false;
      if (hidePassworded && s.passworded) return false;
      if (onlyJoinable) {
        const hasTrack = ownedTrackIds.has(s.trackId);
        const hasAnyCar =
          s.cars.length === 0 || s.cars.some((cid) => ownedCarIds.has(cid));
        const hasSpace = s.maxClients === 0 || s.clients < s.maxClients;
        if (!hasTrack || !hasAnyCar || !hasSpace) return false;
      }
      if (!q) return true;
      const trackName =
        tracks.find((t) => t.id === s.trackId)?.name.toLowerCase() ?? "";
      return (
        s.name.toLowerCase().includes(q) ||
        s.trackId.toLowerCase().includes(q) ||
        trackName.includes(q) ||
        s.host.toLowerCase().includes(q) ||
        s.cars.some((c) => c.toLowerCase().includes(q))
      );
    });
  }, [
    servers,
    searchInput,
    isInviteUri,
    hideEmpty,
    hideFull,
    hidePassworded,
    onlyJoinable,
    ownedTrackIds,
    ownedCarIds,
    tracks,
  ]);

  // Grouped car slots for the Inspector Cars tab
  const groupedCars = useMemo<GroupedCarSlot[]>(() => {
    if (!detail) return [];
    const map = new Map<string, GroupedCarSlot>();
    for (const slot of detail.slots) {
      const existing = map.get(slot.carId);
      if (existing) {
        existing.totalSlots += 1;
        if (!slot.connected) existing.freeSlots += 1;
        if (slot.skinId && !existing.skins.includes(slot.skinId)) {
          existing.skins.push(slot.skinId);
        }
        existing.slots.push(slot);
      } else {
        const carMeta = cars.find((c) => c.id === slot.carId);
        map.set(slot.carId, {
          carId: slot.carId,
          carName: carMeta?.name ?? slot.carId,
          previewPath: carMeta?.previewPath,
          totalSlots: 1,
          freeSlots: slot.connected ? 0 : 1,
          owned: slot.owned || ownedCarIds.has(slot.carId),
          skins: slot.skinId ? [slot.skinId] : [],
          slots: [slot],
        });
      }
    }
    const q = carSearch.trim().toLowerCase();
    return Array.from(map.values())
      .filter(
        (g) =>
          !q ||
          g.carName.toLowerCase().includes(q) ||
          g.carId.toLowerCase().includes(q)
      )
      .sort((a, b) => Number(b.owned) - Number(a.owned) || b.freeSlots - a.freeSlots);
  }, [detail, cars, ownedCarIds, carSearch]);

  const connectedDrivers = useMemo(
    () => (detail?.slots ?? []).filter((s) => s.connected),
    [detail]
  );

  const selectedCarGroup = useMemo(
    () => groupedCars.find((g) => g.carId === selectedCarId) ?? null,
    [groupedCars, selectedCarId]
  );

  // Compute clear join blockers
  const joinBlockers = useMemo<string[]>(() => {
    if (!detail || !selectedServer) return ["Select a server"];
    const reasons: string[] = [];
    if (!detail.trackOwned) reasons.push("Missing Track");
    if (
      detail.minCspBuild &&
      localCspBuildNum !== null &&
      localCspBuildNum < detail.minCspBuild
    ) {
      reasons.push(`Requires CSP ≥ ${detail.minCspBuild}`);
    }
    if (!selectedCarGroup) {
      reasons.push("Select a Car");
    } else {
      if (!selectedCarGroup.owned) reasons.push("Missing Car");
      if (selectedCarGroup.freeSlots === 0 && !detail.bookingMode) {
        reasons.push("Car Class Full");
      }
    }
    if (
      selectedServer.maxClients > 0 &&
      selectedServer.clients >= selectedServer.maxClients &&
      !detail.bookingMode
    ) {
      reasons.push("Server Full");
    }
    if (gameRunning) reasons.push("AC Already Running");
    return reasons;
  }, [detail, selectedServer, selectedCarGroup, localCspBuildNum, gameRunning]);

  const canJoin = joinBlockers.length === 0;

  const handleJoin = async (overrideCarId?: string) => {
    if (!selectedServer || !detail) return;
    const carId = overrideCarId ?? selectedCarId;
    if (!carId) return;
    setJoining(true);
    try {
      const req = {
        host: selectedServer.host,
        port: selectedServer.port,
        httpPort: selectedServer.httpPort,
        carId,
        skinId: selectedSkinId || "00_default",
        password: password || undefined,
      };
      if (detail.bookingMode) {
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
      const uri =
        (await api.formatInviteUri(
          selectedServer.host,
          selectedServer.port,
          password || undefined
        )) ??
        `acmanager://race/online/join?ip=${selectedServer.host}&httpPort=${
          selectedServer.httpPort ?? selectedServer.port
        }`;
      await navigator.clipboard.writeText(uri);
      notify("Invite link copied", "success", uri);
    } catch (err) {
      notify("Failed to copy invite link", "error", String(err));
    }
  };

  const handleDownloadMissing = () => {
    if (!detail || detail.missingContent.length === 0) return;
    const urls = detail.missingContent
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
        detail.missingContent.map((m) => `${m.kind}: ${m.id}`).join(", ")
      );
    }
  };

  const toggleWatchSlot = () => {
    if (!selectedServer) return;
    const key = selectedServer.id;
    const next = !watchedSlots[key];
    setWatchedSlots((prev) => ({ ...prev, [key]: next }));
    notify(
      next ? "Watching for open slot" : "Slot watch disabled",
      "info",
      selectedServer.name
    );
  };

  const selectedTrackMeta = useMemo(() => {
    const tid = detail?.resolvedTrackId ?? selectedServer?.trackId;
    return tracks.find((t) => t.id === tid) ?? null;
  }, [detail, selectedServer, tracks]);

  const heroTrackImg = assetUrl(selectedTrackMeta?.previewPath);

  return (
    <div className="h-full flex flex-col overflow-hidden px-6 py-4 gap-3.5 animate-fade-in">
      {/* Top Sleek Filter Bar */}
      <div className="shrink-0 glass-panel rounded-xl px-3.5 py-2.5 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2.5">
          <SegmentedControl
            size="sm"
            value={tab}
            onChange={setTab}
            options={[
              { value: "online", label: "Online", icon: <Globe className="w-3.5 h-3.5" /> },
              { value: "watchlist", label: "Watchlist", icon: <Star className="w-3.5 h-3.5" /> },
              { value: "recent", label: "Recent", icon: <Clock className="w-3.5 h-3.5" /> },
              { value: "lan", label: "LAN", icon: <Wifi className="w-3.5 h-3.5" /> },
            ]}
          />

          {/* Instant Search + acmanager:// Invite Paste */}
          <div className="relative flex items-center">
            <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-3 pointer-events-none" />
            <input
              type="text"
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && isInviteUri) void handleOpenInviteUri();
              }}
              placeholder="Search servers, tracks, cars, or paste acmanager://..."
              className="h-8 w-72 xl:w-80 pl-8 pr-3 rounded-lg bg-black/50 border border-white/[0.1] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
            />
            {isInviteUri && (
              <Button
                variant="primary"
                size="sm"
                icon={<Link2 className="w-3 h-3" />}
                onClick={handleOpenInviteUri}
                className="ml-1.5 h-8"
              >
                Open Invite
              </Button>
            )}
          </div>
        </div>

        {/* Filter Pills + Live Count */}
        <div className="flex flex-wrap items-center gap-1.5">
          {[
            { label: "100% Joinable", active: onlyJoinable, toggle: () => setOnlyJoinable((v) => !v) },
            { label: "Hide Empty", active: hideEmpty, toggle: () => setHideEmpty((v) => !v) },
            { label: "Hide Full", active: hideFull, toggle: () => setHideFull((v) => !v) },
            { label: "No Password", active: hidePassworded, toggle: () => setHidePassworded((v) => !v) },
          ].map((pill) => (
            <button
              key={pill.label}
              type="button"
              onClick={pill.toggle}
              className={`h-7 px-2.5 rounded-lg text-[11px] font-medium border transition-all cursor-pointer ${
                pill.active
                  ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                  : "bg-white/[0.04] text-[var(--text-secondary)] border-white/[0.07] hover:text-[var(--text-primary)]"
              }`}
            >
              {pill.label}
            </button>
          ))}

          <div className="h-4 w-px bg-white/[0.08] mx-1" />

          <Badge variant="neutral" className="font-mono tabular-nums">
            {filteredServers.length} servers
          </Badge>

          <Button
            variant="ghost"
            size="sm"
            loading={loadingList}
            icon={<RefreshCw className="w-3.5 h-3.5" />}
            onClick={() => loadServers(tab)}
            className="h-7 px-2"
            title="Refresh server list"
          />
        </div>
      </div>

      {/* =================================================================== */}
      {/* Clean 2-Pane Flex Layout (ZERO overlap possible)                    */}
      {/* =================================================================== */}
      <div className="flex gap-4 flex-1 min-h-0 overflow-hidden">
        {/* LEFT PANE: Server Table (flex-1 min-w-0) */}
        <div className="flex-1 min-w-0 glass-panel rounded-2xl flex flex-col overflow-hidden">
          {/* Table Header */}
          <div className="shrink-0 grid grid-cols-[28px_minmax(0,1fr)_170px_112px_72px] gap-3 px-4 py-2.5 border-b border-white/[0.07] bg-black/25 text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
            <span />
            <span>Server</span>
            <span>Circuit</span>
            <span>Capacity</span>
            <span className="text-right">Ping</span>
          </div>

          {/* Scrollable Server Rows */}
          <div className="flex-1 overflow-y-auto divide-y divide-white/[0.04]">
            {loadingList ? (
              <div className="p-4 space-y-2.5">
                {Array.from({ length: 8 }).map((_, i) => (
                  <Skeleton key={i} className="h-14 rounded-xl" />
                ))}
              </div>
            ) : filteredServers.length === 0 ? (
              <div className="p-8">
                <EmptyState
                  icon={<Globe className="w-5 h-5" />}
                  title="No Servers Found"
                  description="Try relaxing your filter pills or refreshing the lobby directory."
                  action={
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => {
                        setOnlyJoinable(false);
                        setHideEmpty(false);
                        setHideFull(false);
                        setHidePassworded(false);
                        setSearchInput("");
                      }}
                    >
                      Clear Filters
                    </Button>
                  }
                />
              </div>
            ) : (
              filteredServers.map((s) => {
                const selected = selectedServer?.id === s.id;
                const fav = isFavorite(s);
                const trackName =
                  tracks.find((t) => t.id === s.trackId)?.name ?? s.trackId;
                const fillPct =
                  s.maxClients > 0
                    ? Math.min(100, Math.round((s.clients / s.maxClients) * 100))
                    : 0;
                const liveDot =
                  s.clients > 0 ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.7)]" : "bg-amber-400/75";

                return (
                  <div
                    key={s.id}
                    onClick={() => setSelectedServer(s)}
                    className={`grid grid-cols-[28px_minmax(0,1fr)_170px_112px_72px] gap-3 items-center px-4 py-3 cursor-pointer transition-all border-l-2 ${
                      selected
                        ? "bg-[var(--accent-soft)]/70 border-l-[var(--accent)]"
                        : "border-l-transparent hover:bg-white/[0.03]"
                    }`}
                  >
                    {/* Favorite Star */}
                    <button
                      type="button"
                      onClick={(e) => toggleFavorite(s, e)}
                      className="flex items-center justify-center text-[var(--text-muted)] hover:text-amber-300 cursor-pointer"
                      title={fav ? "Remove from Watchlist" : "Add to Watchlist"}
                    >
                      <Star
                        className={`w-3.5 h-3.5 ${
                          fav ? "fill-amber-400 text-amber-400" : ""
                        }`}
                      />
                    </button>

                    {/* Server Name & Status */}
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span
                          className={`w-2 h-2 rounded-full shrink-0 ${liveDot}`}
                        />
                        <span className="text-[13px] font-semibold text-[var(--text-primary)] truncate">
                          {s.name}
                        </span>
                        {s.passworded && (
                          <Lock className="w-3 h-3 text-amber-400 shrink-0" />
                        )}
                      </div>
                      <div className="flex items-center gap-2 mt-0.5 pl-4 text-[11px] font-mono text-[var(--text-muted)]">
                        {s.sessionType && <span>{s.sessionType}</span>}
                        {s.cspRequired && (
                          <span className="text-cyan-400/80">
                            CSP {s.cspRequired}+
                          </span>
                        )}
                        {s.country && <span>{s.country}</span>}
                        <span className="truncate opacity-75">
                          {s.cars.length} car{s.cars.length === 1 ? "" : "s"}
                        </span>
                      </div>
                    </div>

                    {/* Track Subtitle */}
                    <div className="min-w-0">
                      <div className="text-xs font-medium text-[var(--text-secondary)] truncate">
                        {trackName}
                      </div>
                      {s.trackLayout && (
                        <div className="text-[10px] font-mono text-[var(--text-muted)] truncate">
                          {s.trackLayout}
                        </div>
                      )}
                    </div>

                    {/* Player Capacity Fill Bar */}
                    <div className="space-y-1">
                      <div className="flex items-center justify-between text-[11px] font-mono tabular-nums">
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
                        className={`inline-flex items-center px-2 py-0.5 rounded-md border font-mono text-[11px] tabular-nums ${pingStyle(
                          s.pingMs
                        )}`}
                      >
                        {s.pingMs != null ? `${s.pingMs} ms` : "—"}
                      </span>
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>

        {/* ================================================================= */}
        {/* RIGHT PANE: Fixed w-[440px] shrink-0 Glass Detail Inspector       */}
        {/* ================================================================= */}
        <aside className="w-[440px] shrink-0 glass-panel rounded-2xl flex flex-col overflow-hidden">
          {!selectedServer ? (
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
              {/* Pinned Atmospheric Track Hero Header */}
              <div className="shrink-0 relative h-40 overflow-hidden border-b border-white/[0.08]">
                {heroTrackImg ? (
                  <img
                    src={heroTrackImg}
                    alt={selectedTrackMeta?.name ?? selectedServer.trackId}
                    className="w-full h-full object-cover scale-105 blur-[1.5px] opacity-65"
                  />
                ) : (
                  <div
                    className="w-full h-full"
                    style={{
                      background:
                        "radial-gradient(360px 180px at 80% 15%, rgba(var(--accent-rgb), 0.28), transparent 65%), radial-gradient(280px 160px at 15% 90%, rgba(0, 209, 255, 0.16), transparent 70%), linear-gradient(135deg, #121A29 0%, #090D14 100%)",
                    }}
                  />
                )}
                <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-[#0D1118]/70 to-[#0D1118]/30" />

                <div className="absolute inset-0 p-4 flex flex-col justify-between">
                  {/* Top chips */}
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <Badge variant="success" dot>
                        <span className="font-mono tabular-nums">
                          {selectedServer.clients}/{selectedServer.maxClients}
                        </span>
                      </Badge>
                      {selectedServer.sessionType && (
                        <Badge variant="neutral">
                          {selectedServer.sessionType}
                        </Badge>
                      )}
                      {detail?.minCspBuild && (
                        <Badge variant="info">
                          CSP ≥ {detail.minCspBuild}
                        </Badge>
                      )}
                    </div>
                    <span
                      className={`px-2 py-0.5 rounded-md border font-mono text-[11px] tabular-nums ${pingStyle(
                        selectedServer.pingMs
                      )}`}
                    >
                      {selectedServer.pingMs != null
                        ? `${selectedServer.pingMs} ms`
                        : "—"}
                    </span>
                  </div>

                  {/* Server Title & Circuit */}
                  <div>
                    <h2 className="font-display text-2xl font-bold uppercase tracking-wide text-white leading-tight line-clamp-1">
                      {selectedServer.name}
                    </h2>
                    <div className="flex items-center justify-between gap-2 mt-0.5 text-xs">
                      <span className="text-[var(--text-secondary)] truncate">
                        {selectedTrackMeta?.name ?? selectedServer.trackId}
                        {selectedServer.trackLayout
                          ? ` · ${selectedServer.trackLayout}`
                          : ""}
                      </span>
                      <span className="font-mono text-[10px] text-[var(--text-muted)] shrink-0">
                        {selectedServer.host}:{selectedServer.port}
                      </span>
                    </div>
                  </div>
                </div>
              </div>

              {/* Pinned Action Bar (Join / Book + Secondary Utilities) */}
              <div className="shrink-0 p-3.5 border-b border-white/[0.07] space-y-2.5 bg-black/20">
                {selectedServer.passworded && (
                  <div className="relative">
                    <Lock className="w-3.5 h-3.5 text-amber-400 absolute left-3 top-1/2 -translate-y-1/2" />
                    <input
                      type="password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      placeholder="Server password required..."
                      className="w-full h-8 pl-8 pr-3 rounded-lg bg-black/55 border border-amber-500/30 text-xs text-white placeholder:text-[var(--text-muted)] outline-none focus:border-amber-400"
                    />
                  </div>
                )}

                {/* Primary Join CTA or Glass Disabled Blocker State */}
                {canJoin ? (
                  <Button
                    variant="primary"
                    size="lg"
                    loading={joining}
                    icon={<Play className="w-4 h-4 fill-current" />}
                    onClick={() => handleJoin()}
                    className="w-full font-display text-base uppercase tracking-wider h-10"
                  >
                    {detail?.bookingMode ? "Book Slot (/sub)" : "Join Server"}
                  </Button>
                ) : (
                  <div className="rounded-xl bg-white/[0.04] border border-white/[0.09] p-2.5 space-y-2">
                    <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                      <span className="flex items-center gap-1.5 font-medium">
                        <ShieldAlert className="w-3.5 h-3.5 text-amber-400" />
                        Cannot Join Yet
                      </span>
                      <span className="font-mono text-[10px]">
                        {joinBlockers.length} blocker
                        {joinBlockers.length === 1 ? "" : "s"}
                      </span>
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      {joinBlockers.map((reason) => (
                        <Badge key={reason} variant="warning">
                          {reason}
                        </Badge>
                      ))}
                    </div>
                  </div>
                )}

                {/* 1-Click Utility Buttons */}
                <div className="grid grid-cols-3 gap-1.5">
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!detail || detail.missingContent.length === 0}
                    icon={<Download className="w-3.5 h-3.5" />}
                    onClick={handleDownloadMissing}
                    className="text-[11px] px-2"
                  >
                    Missing
                    {detail && detail.missingContent.length > 0
                      ? ` (${detail.missingContent.length})`
                      : ""}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<Copy className="w-3.5 h-3.5" />}
                    onClick={handleCopyInvite}
                    className="text-[11px] px-2"
                  >
                    Copy Invite
                  </Button>
                  <Button
                    variant={
                      watchedSlots[selectedServer.id] ? "primary" : "secondary"
                    }
                    size="sm"
                    icon={
                      watchedSlots[selectedServer.id] ? (
                        <BellRing className="w-3.5 h-3.5" />
                      ) : (
                        <Bell className="w-3.5 h-3.5" />
                      )
                    }
                    onClick={toggleWatchSlot}
                    className="text-[11px] px-2"
                  >
                    {watchedSlots[selectedServer.id] ? "Watching" : "Watch Slot"}
                  </Button>
                </div>

                {/* Inspector Sub-Tabs: Cars | Drivers | Info */}
                <SegmentedControl
                  size="sm"
                  value={inspectorTab}
                  onChange={setInspectorTab}
                  className="w-full grid grid-cols-3"
                  options={[
                    {
                      value: "cars",
                      label: `Cars (${groupedCars.length})`,
                      icon: <Car className="w-3.5 h-3.5" />,
                    },
                    {
                      value: "drivers",
                      label: `Drivers (${connectedDrivers.length})`,
                      icon: <Users className="w-3.5 h-3.5" />,
                    },
                    {
                      value: "info",
                      label: "Info",
                      icon: <Info className="w-3.5 h-3.5" />,
                    },
                  ]}
                />
              </div>

              {/* Scrollable Inspector Body */}
              <div className="flex-1 min-h-0 overflow-y-auto p-3.5 space-y-3">
                {loadingDetail ? (
                  <div className="space-y-2.5">
                    <Skeleton className="h-9 rounded-lg" />
                    <div className="grid grid-cols-2 gap-2.5">
                      <Skeleton className="h-28 rounded-xl" />
                      <Skeleton className="h-28 rounded-xl" />
                      <Skeleton className="h-28 rounded-xl" />
                      <Skeleton className="h-28 rounded-xl" />
                    </div>
                  </div>
                ) : inspectorTab === "cars" ? (
                  <>
                    {/* Car Slot Search */}
                    <div className="relative">
                      <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-2.5 top-1/2 -translate-y-1/2" />
                      <input
                        type="text"
                        value={carSearch}
                        onChange={(e) => setCarSearch(e.target.value)}
                        placeholder="Filter available cars (double-click to join)..."
                        className="w-full h-8 pl-8 pr-3 rounded-lg bg-black/45 border border-white/[0.09] text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-border)]"
                      />
                    </div>

                    {/* 2-Column Visual Car Slot Grid */}
                    <div className="grid grid-cols-2 gap-2.5">
                      {groupedCars.map((group) => {
                        const active = selectedCarId === group.carId;
                        const img = assetUrl(group.previewPath);
                        return (
                          <button
                            key={group.carId}
                            type="button"
                            onClick={() => {
                              setSelectedCarId(group.carId);
                              if (group.skins[0]) setSelectedSkinId(group.skins[0]);
                            }}
                            onDoubleClick={() => {
                              setSelectedCarId(group.carId);
                              if (group.owned && group.freeSlots > 0 && canJoin) {
                                void handleJoin(group.carId);
                              }
                            }}
                            className={`text-left rounded-xl overflow-hidden border transition-all cursor-pointer group ${
                              active
                                ? "border-[var(--accent)] bg-[var(--accent-soft)] shadow-[0_0_16px_-6px_var(--accent-glow)]"
                                : "border-white/[0.08] bg-white/[0.03] hover:border-white/[0.18]"
                            }`}
                          >
                            <div className="relative h-20 bg-[#080B11] overflow-hidden">
                              {img ? (
                                <img
                                  src={img}
                                  alt={group.carName}
                                  className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                                />
                              ) : (
                                <div className="w-full h-full bg-gradient-to-br from-white/[0.07] to-transparent flex items-center justify-center">
                                  <Car className="w-5 h-5 text-white/20" />
                                </div>
                              )}
                              <div className="absolute inset-0 bg-gradient-to-t from-[#0D1118] via-transparent to-transparent" />
                              {/* Free count badge */}
                              <span
                                className={`absolute top-1.5 right-1.5 px-1.5 py-0.5 rounded text-[10px] font-mono font-semibold tabular-nums ${
                                  group.freeSlots > 0
                                    ? "bg-emerald-500/20 text-emerald-300 border border-emerald-500/35"
                                    : "bg-red-500/20 text-red-300 border border-red-500/35"
                                }`}
                              >
                                {group.freeSlots > 0
                                  ? `${group.freeSlots} free`
                                  : "Full"}
                              </span>
                              {/* Installed / Missing indicator */}
                              <span className="absolute top-1.5 left-1.5">
                                {group.owned ? (
                                  <span className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-emerald-500/20 border border-emerald-500/40 text-emerald-300">
                                    <Check className="w-3 h-3" />
                                  </span>
                                ) : (
                                  <span className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-amber-500/20 border border-amber-500/40 text-amber-300">
                                    <AlertTriangle className="w-3 h-3" />
                                  </span>
                                )}
                              </span>
                            </div>
                            <div className="p-2">
                              <div className="text-[11px] font-semibold text-[var(--text-primary)] truncate">
                                {group.carName}
                              </div>
                              <div className="text-[10px] font-mono text-[var(--text-muted)]">
                                {group.owned ? "Installed" : "Missing"} ·{" "}
                                {group.totalSlots} slot
                                {group.totalSlots === 1 ? "" : "s"}
                              </div>
                            </div>
                          </button>
                        );
                      })}
                    </div>

                    {/* Skin Selector Strip */}
                    {selectedCarGroup && (
                      <div className="pt-2 border-t border-white/[0.06] space-y-1.5">
                        <div className="flex items-center justify-between text-[10px] font-mono uppercase tracking-[0.12em] text-[var(--text-muted)]">
                          <span>Livery / Skin</span>
                          <span className="truncate max-w-[200px] text-[var(--text-secondary)]">
                            {selectedCarGroup.carName}
                          </span>
                        </div>
                        <div className="flex gap-1.5 overflow-x-auto pb-1">
                          {(selectedCarGroup.skins.length > 0
                            ? selectedCarGroup.skins.map((id) => ({
                                id,
                                name:
                                  localSkins.find((s) => s.id === id)?.name ??
                                  id,
                              }))
                            : localSkins.map((s) => ({
                                id: s.id,
                                name: s.name,
                              }))
                          ).map((skin) => {
                            const active = selectedSkinId === skin.id;
                            return (
                              <button
                                key={skin.id}
                                type="button"
                                onClick={() => setSelectedSkinId(skin.id)}
                                className={`px-2.5 py-1 rounded-lg text-[11px] whitespace-nowrap border transition-all cursor-pointer ${
                                  active
                                    ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)] font-semibold"
                                    : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.07] hover:text-white"
                                }`}
                              >
                                {skin.name}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    )}
                  </>
                ) : inspectorTab === "drivers" ? (
                  connectedDrivers.length === 0 ? (
                    <EmptyState
                      icon={<Users className="w-5 h-5" />}
                      title="No Active Drivers"
                      description="This server currently has open slots waiting for the first driver."
                      className="p-6"
                    />
                  ) : (
                    <div className="space-y-1.5">
                      {connectedDrivers.map((slot, idx) => {
                        const carName =
                          cars.find((c) => c.id === slot.carId)?.name ??
                          slot.carId;
                        return (
                          <div
                            key={`${slot.driverName ?? "drv"}-${idx}`}
                            className="flex items-center justify-between gap-2.5 px-3 py-2 rounded-xl bg-white/[0.03] border border-white/[0.06]"
                          >
                            <div className="flex items-center gap-2.5 min-w-0">
                              <span className="w-2 h-2 rounded-full bg-emerald-400 shrink-0" />
                              <div className="min-w-0">
                                <div className="text-xs font-semibold text-[var(--text-primary)] truncate">
                                  {slot.driverName || `Driver #${idx + 1}`}
                                </div>
                                <div className="text-[11px] text-[var(--text-muted)] truncate">
                                  {carName}
                                </div>
                              </div>
                            </div>
                            <Badge variant="neutral" className="font-mono text-[10px]">
                              {slot.skinId}
                            </Badge>
                          </div>
                        );
                      })}
                    </div>
                  )
                ) : (
                  <div className="space-y-3 text-xs">
                    {detail?.description && (
                      <div className="rounded-xl bg-white/[0.03] border border-white/[0.07] p-3 text-[var(--text-secondary)] leading-relaxed">
                        {detail.description}
                      </div>
                    )}

                    <div className="rounded-xl bg-white/[0.03] border border-white/[0.07] divide-y divide-white/[0.05]">
                      {[
                        {
                          k: "Address",
                          v: `${selectedServer.host}:${selectedServer.port}`,
                        },
                        {
                          k: "HTTP Port",
                          v: String(selectedServer.httpPort ?? "—"),
                        },
                        {
                          k: "Track Status",
                          v: detail?.trackOwned ? "Installed" : "Missing",
                        },
                        {
                          k: "CSP Minimum",
                          v: detail?.minCspBuild
                            ? `Build ${detail.minCspBuild}`
                            : "None",
                        },
                        {
                          k: "Booking Mode",
                          v: detail?.bookingMode ? "Booking (/sub)" : "Pickup",
                        },
                      ].map((row) => (
                        <div
                          key={row.k}
                          className="flex items-center justify-between px-3 py-2"
                        >
                          <span className="text-[var(--text-muted)]">{row.k}</span>
                          <span className="font-mono text-[var(--text-primary)]">
                            {row.v}
                          </span>
                        </div>
                      ))}
                    </div>

                    {detail && detail.missingContent.length > 0 && (
                      <div className="rounded-xl bg-amber-500/10 border border-amber-500/25 p-3 space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-semibold text-amber-300">
                            Missing Content ({detail.missingContent.length})
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
                          {detail.missingContent.map((m) => (
                            <div
                              key={`${m.kind}-${m.id}`}
                              className="flex items-center justify-between text-[11px] font-mono text-amber-200/90"
                            >
                              <span>{m.id}</span>
                              <Badge variant="warning">{m.kind}</Badge>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}

                    {detail && detail.missingContent.length === 0 && (
                      <div className="rounded-xl bg-emerald-500/10 border border-emerald-500/25 px-3 py-2.5 flex items-center gap-2 text-emerald-300">
                        <CheckCircle2 className="w-4 h-4 shrink-0" />
                        <span>All required cars and track are installed locally.</span>
                      </div>
                    )}
                  </div>
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

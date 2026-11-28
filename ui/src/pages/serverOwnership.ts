import type { CarEntry, OnlineServerDetail, OnlineServerSummary, TrackEntry } from "../lib/types";

export type TrackOwnershipState = "owned" | "missing" | "unknown";
export type CarOwnershipState = "all" | "partial" | "none" | "unknown";
export type ServerReadiness =
  | "full"
  | "ready"
  | "missing-cars"
  | "missing-track"
  | "missing-both"
  | "unknown";

export interface ServerOwnershipEvaluation {
  trackState: TrackOwnershipState;
  carState: CarOwnershipState;
  ownedCars: number;
  totalCars: number;
  readiness: ServerReadiness;
  contentScore: number;
  matchedTrack: TrackEntry | null;
}

export interface TrackOwnershipIndex {
  exactMap: Map<string, TrackEntry>;
  prefixes: Array<{ prefix: string; track: TrackEntry }>;
}

export interface CarOwnershipIndex {
  exactKeys: Set<string>;
  yearStrippedKeys: Set<string>;
}

/** Normalize asset IDs/names across Unicode accents, punctuation, and whitespace. */
export function normalizeAssetKey(raw: string): string {
  return raw
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[_:/.\-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function stripLeadingPrefix(norm: string, prefix: string): string {
  return norm.startsWith(prefix) ? norm.slice(prefix.length).trim() : norm;
}

function stripTrailingYear(norm: string): string {
  return norm.replace(/\s+\b(?:19|20)\d{2}\b$/, "").trim();
}

/** Build a fast lookup index covering IDs, names, layouts, and common prefixes for installed tracks. */
export function buildTrackOwnershipIndex(tracks: TrackEntry[]): TrackOwnershipIndex {
  const exactMap = new Map<string, TrackEntry>();
  const prefixMap = new Map<string, TrackEntry>();

  const registerExact = (raw: string | undefined | null, track: TrackEntry) => {
    if (!raw) return;
    const norm = normalizeAssetKey(raw);
    if (!norm) return;
    if (!exactMap.has(norm)) exactMap.set(norm, track);
    if (norm.length >= 5 && !prefixMap.has(norm)) prefixMap.set(norm, track);
  };

  for (const t of tracks) {
    registerExact(t.id, t);
    const noKs = stripLeadingPrefix(normalizeAssetKey(t.id), "ks ");
    if (noKs.length >= 3) {
      if (!exactMap.has(noKs)) exactMap.set(noKs, t);
      if (noKs.length >= 3 && !prefixMap.has(noKs)) prefixMap.set(noKs, t);
    }

    const nameNorm = normalizeAssetKey(t.name);
    registerExact(t.name, t);
    for (const lead of [
      "circuit de ",
      "circuit of ",
      "autodromo nazionale ",
      "autodromo internazionale ",
      "autodromo ",
      "autodrome ",
    ]) {
      const stripped = stripLeadingPrefix(nameNorm, lead);
      if (stripped !== nameNorm && stripped.length >= 3) {
        if (!exactMap.has(stripped)) exactMap.set(stripped, t);
        if (!prefixMap.has(stripped)) prefixMap.set(stripped, t);
      }
    }

    for (const lay of t.layouts ?? []) {
      registerExact(`${t.name} ${lay.name}`, t);
      registerExact(`${t.name} ${lay.id}`, t);
      registerExact(`${t.id} ${lay.id}`, t);
      registerExact(`${t.id} ${lay.name}`, t);
      if (noKs.length >= 3) {
        registerExact(`${noKs} ${lay.name}`, t);
        registerExact(`${noKs} ${lay.id}`, t);
      }
    }
  }

  const prefixes = Array.from(prefixMap.entries())
    .map(([prefix, track]) => ({ prefix, track }))
    .sort((a, b) => b.prefix.length - a.prefix.length);

  return { exactMap, prefixes };
}

/** Resolve an installed track entry from a folder ID or directory display track string. */
export function resolveTrackFromIndex(
  rawTrack: string | undefined | null,
  index: TrackOwnershipIndex
): TrackEntry | null {
  if (!rawTrack) return null;
  const norm = normalizeAssetKey(rawTrack);
  if (!norm || norm === "unknown") return null;

  const direct = index.exactMap.get(norm);
  if (direct) return direct;

  const noKsQuery = stripLeadingPrefix(norm, "ks ");
  if (noKsQuery !== norm) {
    const ksHit = index.exactMap.get(noKsQuery);
    if (ksHit) return ksHit;
  }

  // Handle slash/colon delimited CSP or folder-layout strings
  for (const part of rawTrack.split(/[/:]/)) {
    const pNorm = normalizeAssetKey(part);
    if (!pNorm) continue;
    const hit = index.exactMap.get(pNorm) ?? index.exactMap.get(stripLeadingPrefix(pNorm, "ks "));
    if (hit) return hit;
  }

  // Word-boundary prefix match (e.g., "la canyons freeroam" matches "la canyons", "ks spa 6h" matches "spa")
  for (const { prefix, track } of index.prefixes) {
    if (
      norm === prefix ||
      norm.startsWith(`${prefix} `) ||
      noKsQuery === prefix ||
      noKsQuery.startsWith(`${prefix} `)
    ) {
      return track;
    }
  }

  return null;
}

/** Build a lookup set covering car folder IDs, display names, brand+name, and year-stripped aliases. */
export function buildCarOwnershipIndex(cars: CarEntry[]): CarOwnershipIndex {
  const exactKeys = new Set<string>();
  const yearStrippedKeys = new Set<string>();

  const add = (raw: string | undefined | null) => {
    if (!raw) return;
    const norm = normalizeAssetKey(raw);
    if (!norm) return;
    exactKeys.add(norm);
    const noYear = stripTrailingYear(norm);
    if (noYear.length >= 6 && noYear !== norm) {
      yearStrippedKeys.add(noYear);
    }
  };

  for (const c of cars) {
    add(c.id);
    const noKs = stripLeadingPrefix(normalizeAssetKey(c.id), "ks ");
    if (noKs.length >= 3) exactKeys.add(noKs);
    add(c.name);
    if (c.brand && c.name && !c.name.toLowerCase().includes(c.brand.toLowerCase())) {
      add(`${c.brand} ${c.name}`);
    }
  }

  return { exactKeys, yearStrippedKeys };
}

/** Check whether a car folder ID or display car name matches an installed car. */
export function isCarOwnedInIndex(
  rawCar: string | undefined | null,
  index: CarOwnershipIndex
): boolean {
  if (!rawCar) return false;
  const norm = normalizeAssetKey(rawCar);
  if (!norm) return false;
  if (index.exactKeys.has(norm)) return true;

  const noKs = stripLeadingPrefix(norm, "ks ");
  if (noKs !== norm && index.exactKeys.has(noKs)) return true;

  const noYear = stripTrailingYear(norm);
  if (noYear.length >= 6 && (index.exactKeys.has(noYear) || index.yearStrippedKeys.has(noYear))) {
    return true;
  }

  return false;
}

/** Evaluate immediate track & car ownership readiness for a server row. */
export function evaluateServerOwnership(
  server: OnlineServerSummary,
  trackIndex: TrackOwnershipIndex,
  carIndex: CarOwnershipIndex,
  probedDetail?: OnlineServerDetail | null
): ServerOwnershipEvaluation {
  const matchedTrack = resolveTrackFromIndex(
    probedDetail?.resolvedTrackId ?? server.trackId,
    trackIndex
  );

  let trackState: TrackOwnershipState;
  if (probedDetail && probedDetail.resolvedTrackId && probedDetail.resolvedTrackId !== "unknown") {
    trackState = probedDetail.trackOwned || Boolean(matchedTrack) ? "owned" : "missing";
  } else if (!server.trackId || server.trackId.trim().toLowerCase() === "unknown") {
    trackState = "unknown";
  } else {
    trackState = matchedTrack ? "owned" : "missing";
  }

  let ownedCars = 0;
  let totalCars = 0;
  if (probedDetail && probedDetail.slots.length > 0) {
    const models = new Map<string, boolean>();
    for (const slot of probedDetail.slots) {
      if (!slot.carId) continue;
      const owned = Boolean(slot.owned || isCarOwnedInIndex(slot.carId, carIndex));
      models.set(slot.carId, Boolean(models.get(slot.carId) || owned));
    }
    totalCars = models.size;
    for (const owned of models.values()) {
      if (owned) ownedCars += 1;
    }
  } else if (server.cars.length > 0) {
    const uniqueCars = Array.from(new Set(server.cars.filter(Boolean)));
    totalCars = uniqueCars.length;
    for (const car of uniqueCars) {
      if (isCarOwnedInIndex(car, carIndex)) ownedCars += 1;
    }
  }

  let carState: CarOwnershipState;
  if (totalCars === 0) {
    carState = "unknown";
  } else if (ownedCars === totalCars) {
    carState = "all";
  } else if (ownedCars > 0) {
    carState = "partial";
  } else {
    carState = "none";
  }

  let readiness: ServerReadiness;
  let contentScore: number;
  if (trackState === "owned") {
    if (carState === "all") {
      readiness = "full";
      contentScore = 5;
    } else if (carState === "partial" || carState === "unknown") {
      readiness = "ready";
      contentScore = 4;
    } else {
      readiness = "missing-cars";
      contentScore = 3;
    }
  } else if (trackState === "missing") {
    if (carState === "none") {
      readiness = "missing-both";
      contentScore = 1;
    } else {
      readiness = "missing-track";
      contentScore = 2;
    }
  } else {
    readiness = "unknown";
    contentScore = carState === "all" ? 3 : carState === "partial" ? 2 : 0;
  }

  return {
    trackState,
    carState,
    ownedCars,
    totalCars,
    readiness,
    contentScore,
    matchedTrack,
  };
}

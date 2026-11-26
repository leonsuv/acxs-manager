//! Aggregation over the installed content for the home dashboard.
//!
//! Everything here is pure: it takes already-scanned car and track entries
//! plus a map of car id to skin count, and returns counts and extremes.

use std::collections::BTreeMap;

use crate::content::{CarEntry, TrackEntry};

/// Label used for entries whose grouping value is empty or whitespace only.
const UNKNOWN_LABEL: &str = "Unknown";

/// One row of a grouped count, such as "GT3: 12 cars".
#[derive(Debug, Clone, PartialEq)]
pub struct CountEntry {
    pub label: String,
    pub count: usize,
}

/// Summary numbers for the home dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentStats {
    /// Number of car entries.
    pub car_total: usize,
    /// Number of track entries.
    pub track_total: usize,
    /// Sum of layout entries across all tracks.
    pub layout_total: usize,
    /// Sum of skin counts for the listed cars. Cars missing from the skin map count as 0.
    pub skin_total: usize,
    /// Cars grouped by class, sorted by count descending then label.
    pub cars_by_class: Vec<CountEntry>,
    /// Cars grouped by brand, sorted by count descending then label.
    pub cars_by_brand: Vec<CountEntry>,
    /// Tracks grouped by country, sorted by count descending then label.
    pub tracks_by_country: Vec<CountEntry>,
    /// Longest finite track length in metres, if any track has one.
    pub longest_track_m: Option<f64>,
    /// Shortest finite track length in metres, if any track has one.
    pub shortest_track_m: Option<f64>,
    /// Sum of pitboxes across all tracks. Tracks without a value count as 0.
    pub total_pitboxes: u64,
    /// Cars with no tags, or only blank tags.
    pub untagged_cars: usize,
}

/// Compute the dashboard summary for the given installed content.
pub fn compute(
    cars: &[CarEntry],
    tracks: &[TrackEntry],
    skin_counts: &BTreeMap<String, usize>,
) -> ContentStats {
    let skin_total = cars
        .iter()
        .map(|car| skin_counts.get(&car.id).copied().unwrap_or(0))
        .sum::<usize>();

    let layout_total = tracks.iter().map(|track| track.layouts.len()).sum::<usize>();

    let finite_lengths = tracks
        .iter()
        .filter_map(|track| track.length_m)
        .filter(|length| length.is_finite());
    let longest_track_m = finite_lengths
        .clone()
        .fold(None, |best: Option<f64>, length| {
            Some(best.map_or(length, |b| b.max(length)))
        });
    let shortest_track_m = finite_lengths.fold(None, |best: Option<f64>, length| {
        Some(best.map_or(length, |b| b.min(length)))
    });

    let total_pitboxes = tracks
        .iter()
        .map(|track| u64::from(track.pitboxes.unwrap_or(0)))
        .sum::<u64>();

    let untagged_cars = cars
        .iter()
        .filter(|car| car.tags.iter().all(|tag| tag.trim().is_empty()))
        .count();

    ContentStats {
        car_total: cars.len(),
        track_total: tracks.len(),
        layout_total,
        skin_total,
        cars_by_class: group_counts(cars.iter().map(|car| car.car_class.as_str())),
        cars_by_brand: group_counts(cars.iter().map(|car| car.brand.as_str())),
        tracks_by_country: group_counts(tracks.iter().map(|track| track.country.as_str())),
        longest_track_m,
        shortest_track_m,
        total_pitboxes,
        untagged_cars,
    }
}

/// Return the first `n` entries, or all of them when `n` exceeds the length.
pub fn top_n(entries: &[CountEntry], n: usize) -> &[CountEntry] {
    &entries[..n.min(entries.len())]
}

/// Count occurrences of each label. Blank labels are grouped under "Unknown".
/// Result is sorted by count descending, then label case-insensitively.
fn group_counts<'a>(labels: impl Iterator<Item = &'a str>) -> Vec<CountEntry> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for label in labels {
        let key = if label.trim().is_empty() {
            UNKNOWN_LABEL.to_string()
        } else {
            label.to_string()
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    let mut entries: Vec<CountEntry> = counts
        .into_iter()
        .map(|(label, count)| CountEntry { label, count })
        .collect();
    entries.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.label.cmp(&b.label))
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn car(id: &str, brand: &str, car_class: &str, tags: &[&str]) -> CarEntry {
        CarEntry {
            id: id.to_string(),
            name: format!("Car {id}"),
            brand: brand.to_string(),
            car_class: car_class.to_string(),
            year: "2000".to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            path: PathBuf::new(),
            ..CarEntry::default()
        }
    }

    fn track(
        id: &str,
        country: &str,
        length_m: Option<f64>,
        pitboxes: Option<u32>,
        layouts: &[&str],
    ) -> TrackEntry {
        TrackEntry {
            id: id.to_string(),
            name: format!("Track {id}"),
            country: country.to_string(),
            city: String::new(),
            length_m,
            pitboxes,
            layouts: layouts.iter().map(|l| l.to_string()).collect(),
            path: PathBuf::new(),
            ..TrackEntry::default()
        }
    }

    fn entry(label: &str, count: usize) -> CountEntry {
        CountEntry {
            label: label.to_string(),
            count,
        }
    }

    fn fixture_cars() -> Vec<CarEntry> {
        vec![
            car("ks_audi", "Audi", "GT3", &["gt", "modern"]),
            car("ks_bmw", "BMW", "GT3", &[]),
            car("ks_ferrari", "Ferrari", "", &["  "]),
            car("ks_mazda", "Mazda", "Street", &["old"]),
        ]
    }

    fn fixture_tracks() -> Vec<TrackEntry> {
        vec![
            track("monza", "Italy", Some(5793.0), Some(20), &["GP", "Junior"]),
            track("spa", "Belgium", Some(7004.0), Some(50), &["GP"]),
            track("unknown", "", None, None, &[]),
            track("imola", "Italy", Some(4909.0), Some(25), &[]),
        ]
    }

    fn fixture_skins() -> BTreeMap<String, usize> {
        let mut skins = BTreeMap::new();
        skins.insert("ks_audi".to_string(), 3);
        skins.insert("ks_bmw".to_string(), 2);
        skins.insert("ks_mazda".to_string(), 0);
        // Not in the car list, so it must not be counted.
        skins.insert("ks_ghost".to_string(), 9);
        skins
    }

    #[test]
    fn empty_input_gives_zeroed_stats() {
        let stats = compute(&[], &[], &BTreeMap::new());
        assert_eq!(
            stats,
            ContentStats {
                car_total: 0,
                track_total: 0,
                layout_total: 0,
                skin_total: 0,
                cars_by_class: vec![],
                cars_by_brand: vec![],
                tracks_by_country: vec![],
                longest_track_m: None,
                shortest_track_m: None,
                total_pitboxes: 0,
                untagged_cars: 0,
            }
        );
    }

    #[test]
    fn every_field_is_computed() {
        let stats = compute(&fixture_cars(), &fixture_tracks(), &fixture_skins());

        assert_eq!(stats.car_total, 4);
        assert_eq!(stats.track_total, 4);
        assert_eq!(stats.layout_total, 3);
        // 3 (audi) + 2 (bmw) + 0 (mazda) + 0 (ferrari, missing); ghost excluded.
        assert_eq!(stats.skin_total, 5);
        assert_eq!(stats.total_pitboxes, 95);
        assert_eq!(stats.longest_track_m, Some(7004.0));
        assert_eq!(stats.shortest_track_m, Some(4909.0));
        // bmw has no tags, ferrari has only a blank tag.
        assert_eq!(stats.untagged_cars, 2);

        assert_eq!(
            stats.cars_by_class,
            vec![entry("GT3", 2), entry("Street", 1), entry("Unknown", 1)]
        );
        assert_eq!(
            stats.cars_by_brand,
            vec![
                entry("Audi", 1),
                entry("BMW", 1),
                entry("Ferrari", 1),
                entry("Mazda", 1),
            ]
        );
        assert_eq!(
            stats.tracks_by_country,
            vec![entry("Italy", 2), entry("Belgium", 1), entry("Unknown", 1)]
        );
    }

    #[test]
    fn blank_labels_group_as_unknown() {
        let cars = vec![
            car("a", "", "", &["x"]),
            car("b", "   ", "  ", &["x"]),
            car("c", "Lotus", "Open", &["x"]),
        ];
        let tracks = vec![
            track("t1", "", None, None, &[]),
            track("t2", " ", None, None, &[]),
        ];
        let stats = compute(&cars, &tracks, &BTreeMap::new());

        assert_eq!(stats.cars_by_class, vec![entry("Unknown", 2), entry("Open", 1)]);
        assert_eq!(stats.cars_by_brand, vec![entry("Unknown", 2), entry("Lotus", 1)]);
        assert_eq!(stats.tracks_by_country, vec![entry("Unknown", 2)]);
    }

    #[test]
    fn sort_is_count_desc_then_label_case_insensitive() {
        let cars = vec![
            car("1", "Zeta", "", &["x"]),
            car("2", "zeta", "", &["x"]),
            car("3", "alfa", "", &["x"]),
            car("4", "Audi", "", &["x"]),
            car("5", "bmw", "", &["x"]),
            car("6", "Mazda", "", &["x"]),
            car("7", "Mazda", "", &["x"]),
        ];
        let stats = compute(&cars, &[], &BTreeMap::new());

        // Mazda has the highest count, then the count-1 labels in case-insensitive order.
        assert_eq!(
            stats.cars_by_brand,
            vec![
                entry("Mazda", 2),
                entry("alfa", 1),
                entry("Audi", 1),
                entry("bmw", 1),
                entry("Zeta", 1),
                entry("zeta", 1),
            ]
        );
    }

    #[test]
    fn untagged_cars_counts_empty_and_blank_tags() {
        let cars = vec![
            car("a", "A", "", &[]),
            car("b", "B", "", &[""]),
            car("c", "C", "", &["  ", "\t"]),
            car("d", "D", "", &["", "gt"]),
        ];
        let stats = compute(&cars, &[], &BTreeMap::new());
        assert_eq!(stats.untagged_cars, 3);
    }

    #[test]
    fn skin_total_only_counts_listed_cars() {
        let cars = vec![car("ks_audi", "Audi", "", &["x"])];
        let stats = compute(&cars, &[], &fixture_skins());
        assert_eq!(stats.skin_total, 3);

        let stats = compute(&[], &[], &fixture_skins());
        assert_eq!(stats.skin_total, 0);
    }

    #[test]
    fn track_extremes_ignore_missing_and_non_finite_lengths() {
        let tracks = vec![
            track("none", "X", None, None, &[]),
            track("nan", "X", Some(f64::NAN), None, &[]),
            track("inf", "X", Some(f64::INFINITY), None, &[]),
        ];
        let stats = compute(&[], &tracks, &BTreeMap::new());
        assert_eq!(stats.longest_track_m, None);
        assert_eq!(stats.shortest_track_m, None);

        let tracks = vec![
            track("only", "X", Some(1200.5), None, &[]),
            track("nan", "X", Some(f64::NAN), None, &[]),
        ];
        let stats = compute(&[], &tracks, &BTreeMap::new());
        assert_eq!(stats.longest_track_m, Some(1200.5));
        assert_eq!(stats.shortest_track_m, Some(1200.5));
    }

    #[test]
    fn pitboxes_and_layouts_treat_missing_as_zero() {
        let tracks = vec![
            track("a", "X", Some(1000.0), None, &[]),
            track("b", "X", Some(1000.0), Some(12), &["one", "two", "three"]),
        ];
        let stats = compute(&[], &tracks, &BTreeMap::new());
        assert_eq!(stats.total_pitboxes, 12);
        assert_eq!(stats.layout_total, 3);
    }

    #[test]
    fn top_n_respects_bounds() {
        let entries = vec![entry("a", 3), entry("b", 2), entry("c", 1)];

        assert!(top_n(&entries, 0).is_empty());
        assert_eq!(top_n(&entries, 1), &entries[..1]);
        assert_eq!(top_n(&entries, 2), &entries[..2]);
        assert_eq!(top_n(&entries, 3), &entries[..]);
        assert_eq!(top_n(&entries, 10), &entries[..]);
        assert!(top_n(&[], 5).is_empty());
        assert!(top_n(&[], 0).is_empty());
    }
}

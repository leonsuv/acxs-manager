//! Runs the read-only library code over a real install and reports what looks wrong:
//! `cargo run --example smoke_real`. Writes nothing.

use acxs_manager::{ac_log, game_options, cardata, content, content_repair, csp, csp_config, game_cfg, graphics_profiles, lights_profile, lua_apps, mods, settings, setups, skins, track_surfaces, weather};
use std::time::Instant;

fn main() {
    let cfg = settings::Settings::load(&settings::settings_path());
    let root = cfg.ac_root_path().or_else(settings::detect_ac_root).expect("no AC folder");
    println!("root {}", root.display());
    let t = Instant::now();
    let user_cfg_dir = settings::ac_user_cfg_dir().expect("cfg");
    let cars = content::scan_cars(&root);
    let tracks = content::scan_tracks(&root);
    println!("cars {} tracks {} in {:?}", cars.len(), tracks.len(), t.elapsed());
    let unnamed: Vec<&str> = cars.iter().filter(|c| c.name.trim().is_empty() || c.name == c.id).map(|c| c.id.as_str()).collect();
    println!("cars without a name: {} {:?}", unnamed.len(), &unnamed[..unnamed.len().min(8)]);
    let no_brand = cars.iter().filter(|c| c.brand.trim().is_empty()).count();
    println!("cars without brand: {no_brand}");
    let unnamed_t: Vec<&str> = tracks.iter().filter(|c| c.name.trim().is_empty() || c.name == c.id).map(|c| c.id.as_str()).collect();
    println!("tracks without a name: {:?}", unnamed_t);
    let mut skinless = Vec::new();
    let (mut missing_power, mut missing_mass, mut packed_fail) = (Vec::new(), 0, 0);
    let t = Instant::now();
    for car in &cars {
        if skins::scan_skins(&root, &car.id).is_empty() {
            skinless.push(car.id.clone());
        }
        let specs = cardata::read_specs(&car.path);
        if specs.max_power_w.is_none() {
            missing_power.push(car.id.clone());
        }
        if specs.mass_kg.is_none() {
            missing_mass += 1;
        }
        if cardata::load_data_ini(&car.path, "suspensions.ini").is_err() {
            packed_fail += 1;
        }
    }
    println!("car specs in {:?}; skinless {} {:?}", t.elapsed(), skinless.len(), &skinless[..skinless.len().min(6)]);
    println!("no power {} {:?}; no mass {missing_mass}; suspensions unreadable {packed_fail}", missing_power.len(), &missing_power[..missing_power.len().min(6)]);
    for track in &tracks {
        for layout in track.layouts.iter().map(Some).chain([None]) {
            let dir = layout.map_or(track.path.clone(), |l| track.path.join(l));
            let data = dir.join("data");
            if data.is_dir() {
                if let Err(err) = track_surfaces::read_surfaces(&data) {
                    println!("surfaces {}: {err}", dir.display());
                }
            }
        }
    }
    println!("csp version: {:?}", csp::installed_version(&root));
    println!("graphics active: {:?}", graphics_profiles::active(&root).map(|_| "ok"));
    let ext = root.join("extension").join("config");
    let files = csp_config::list_config_files(&ext);
    let mut bad = 0;
    let mut settings_total = 0;
    for f in &files {
        match csp_config::read_settings_with_docs(&f.path, None) {
            Ok(s) => settings_total += s.len(),
            Err(err) => {
                bad += 1;
                println!("csp config {}: {err}", f.path.display());
            }
        }
    }
    println!("csp config files {} settings {settings_total} unreadable {bad}; lights version {:?}", files.len(), lights_profile::detect_version(&ext));
    println!("lua apps: {}", lua_apps::list_all(&root).len());
    let (mut read, mut failed, mut formats) = (0, 0, std::collections::BTreeMap::new());
    for car in &cars {
        for skin in skins::scan_skins(&root, &car.id).iter().take(1) {
            for tex in acxs_manager::livery_paint::textures(&car.path.join("skins").join(&skin.skin_id)) {
                match acxs_manager::livery_paint::load(&tex) {
                    Ok(picture) => {
                        read += 1;
                        let again = acxs_manager::dds::decode(&acxs_manager::dds::encode(&picture)).unwrap();
                        assert_eq!(again, picture);
                    }
                    Err(err) => {
                        failed += 1;
                        *formats.entry(err).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    println!("skin textures read {read}, failed {failed} {formats:?}");
    let special = acxs_manager::events::special_events(&root);
    let career = acxs_manager::events::career_events(&root);
    let playable = special.iter().chain(&career).filter(|e| e.is_playable(&root)).count();
    println!("events: {} special, {} career, {playable} playable", special.len(), career.len());
    let mut kinds = std::collections::BTreeMap::new();
    for e in special.iter().chain(&career) {
        *kinds.entry(e.kind.clone()).or_insert(0) += 1;
    }
    println!("  kinds {kinds:?}; without goals {}", special.iter().chain(&career).filter(|e| e.goals.is_empty()).count());
    println!("python apps {:?}", game_options::python_apps(&root, &user_cfg_dir).iter().map(|a| (a.id.clone(), a.active)).collect::<Vec<_>>());
    println!("filters {:?} active {:?}", game_options::pp_filters(&root, &user_cfg_dir).len(), game_options::active_filter(&user_cfg_dir));
    println!("options read {}/{}", game_options::load(&user_cfg_dir).iter().flatten().count(), game_options::OPTIONS.len());
    println!("weather presets {} controllers {}", weather::list_presets(&root).len(), weather::list_controllers(&root).len());
    println!("weather fx: {:?}", weather::read_weather_fx(&root).map(|v| v.len()));
    println!("disabled mods: {}", mods::list_disabled(&root).len());
    let user_cfg = settings::ac_user_cfg_dir().expect("cfg");
    println!("video: {:?}", game_cfg::read_video(&user_cfg).map(|_| "ok"));
    println!("assetto_corsa.ini: {:?}", game_cfg::read_assetto_corsa_ini(&user_cfg).map(|_| "ok").map_err(|e| e.to_string()));
    let docs = settings::ac_documents_dir().expect("docs");
    println!("setups: {}", setups::list_setups(&docs.join("setups"), None).len());
    for path in ac_log::known_log_paths(&root) {
        println!("log {} {:?}", path.display(), ac_log::read_log(&path).map(|t| ac_log::parse_lines(&t).len()));
    }
    let t = Instant::now();
    let issues = content_repair::check_all(&root);
    println!("content issues {} in {:?}", issues.len(), t.elapsed());
    println!("  summary {:?}", content_repair::summary(&issues));
    let mut groups = std::collections::BTreeMap::new();
    for issue in &issues {
        let text = format!("{:?} {}", issue.severity, issue.message);
        let key: String = text.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
        *groups.entry(key).or_insert(0) += 1;
    }
    for (key, count) in &groups {
        println!("  {count:4}  {key}");
    }
    for issue in issues.iter().take(0) {
        println!("  {issue:?}");
    }
}

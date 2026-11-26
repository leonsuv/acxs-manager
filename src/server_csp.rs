//! CSP dedicated server extras (`csp_extra_options.ini`) and AssettoServer
//! `extra_cfg.yml` configuration read/write with automatic backups.
//!
//! Both files are optional extensions sitting in `<server>/cfg/`. Unknown keys
//! and comments in `csp_extra_options.ini` are preserved via [`crate::ini::Ini`].

use crate::ini::Ini;
use std::fs;
use std::io;
use std::path::Path;

pub const CSP_EXTRA_OPTIONS: &str = "csp_extra_options.ini";
pub const EXTRA_CFG_YML: &str = "extra_cfg.yml";

/// Typed subset of CSP's `csp_extra_options.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspExtraOptions {
    /// `[EXTRA_RULES]` `ALLOW_WRONG_WAY`.
    pub allow_wrong_way: bool,
    /// `[EXTRA_RULES]` `ENFORCE_BACK_TO_PITS_PENALTY`.
    pub enforce_back_to_pits_penalty: bool,
    /// `[EXTRA_RULES]` `NO_COLLISIONS`.
    pub no_collisions: bool,
    /// `[PITS_SPEED_LIMITER]` `KEEP_COLLISIONS`.
    pub pits_keep_collisions: bool,
    /// `[PITS_SPEED_LIMITER]` `NON_LETHAL`.
    pub pits_non_lethal: bool,
    /// `[PITS_SPEED_LIMITER]` `SPEED_KMH`.
    pub pits_speed_kmh: u32,
    /// `[WEATHER_FX]` `USE_CUSTOM_WEATHER`.
    pub use_custom_weather: bool,
    /// `[WEATHER_FX]` `FORCE_RAIN`.
    pub force_rain: bool,
    /// `[CONTENT_URL]` `URL` (CSP/CM missing-content download URL).
    pub content_url: String,
    /// `[CUSTOM_PHYSICS]` `MINIMUM_CSP_VERSION` (build number, 0 = unset).
    pub minimum_csp_build: u32,
}

impl Default for CspExtraOptions {
    fn default() -> Self {
        Self {
            allow_wrong_way: false,
            enforce_back_to_pits_penalty: true,
            no_collisions: false,
            pits_keep_collisions: false,
            pits_non_lethal: true,
            pits_speed_kmh: 80,
            use_custom_weather: false,
            force_rain: false,
            content_url: String::new(),
            minimum_csp_build: 0,
        }
    }
}

/// Reads `<cfg_dir>/csp_extra_options.ini`.
pub fn read_csp_extra_options(cfg_dir: &Path) -> io::Result<CspExtraOptions> {
    let ini = Ini::load(&cfg_dir.join(CSP_EXTRA_OPTIONS))?;
    let d = CspExtraOptions::default();
    Ok(CspExtraOptions {
        allow_wrong_way: ini_flag(&ini, "EXTRA_RULES", "ALLOW_WRONG_WAY", d.allow_wrong_way),
        enforce_back_to_pits_penalty: ini_flag(
            &ini,
            "EXTRA_RULES",
            "ENFORCE_BACK_TO_PITS_PENALTY",
            d.enforce_back_to_pits_penalty,
        ),
        no_collisions: ini_flag(&ini, "EXTRA_RULES", "NO_COLLISIONS", d.no_collisions),
        pits_keep_collisions: ini_flag(
            &ini,
            "PITS_SPEED_LIMITER",
            "KEEP_COLLISIONS",
            d.pits_keep_collisions,
        ),
        pits_non_lethal: ini_flag(&ini, "PITS_SPEED_LIMITER", "NON_LETHAL", d.pits_non_lethal),
        pits_speed_kmh: ini_u32(&ini, "PITS_SPEED_LIMITER", "SPEED_KMH", d.pits_speed_kmh),
        use_custom_weather: ini_flag(
            &ini,
            "WEATHER_FX",
            "USE_CUSTOM_WEATHER",
            d.use_custom_weather,
        ),
        force_rain: ini_flag(&ini, "WEATHER_FX", "FORCE_RAIN", d.force_rain),
        content_url: ini
            .get("CONTENT_URL", "URL")
            .map(str::to_string)
            .unwrap_or(d.content_url),
        minimum_csp_build: ini_u32(
            &ini,
            "CUSTOM_PHYSICS",
            "MINIMUM_CSP_VERSION",
            d.minimum_csp_build,
        ),
    })
}

/// Writes `<cfg_dir>/csp_extra_options.ini`, preserving unknown sections/comments and backing up.
pub fn write_csp_extra_options(cfg_dir: &Path, opts: &CspExtraOptions) -> io::Result<()> {
    if opts.content_url.contains(['\r', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "content_url contains a line break",
        ));
    }
    let path = cfg_dir.join(CSP_EXTRA_OPTIONS);
    let mut ini = if path.exists() {
        Ini::load(&path)?
    } else {
        Ini::default()
    };
    ini.set("EXTRA_RULES", "ALLOW_WRONG_WAY", u8::from(opts.allow_wrong_way));
    ini.set(
        "EXTRA_RULES",
        "ENFORCE_BACK_TO_PITS_PENALTY",
        u8::from(opts.enforce_back_to_pits_penalty),
    );
    ini.set("EXTRA_RULES", "NO_COLLISIONS", u8::from(opts.no_collisions));
    ini.set(
        "PITS_SPEED_LIMITER",
        "KEEP_COLLISIONS",
        u8::from(opts.pits_keep_collisions),
    );
    ini.set(
        "PITS_SPEED_LIMITER",
        "NON_LETHAL",
        u8::from(opts.pits_non_lethal),
    );
    ini.set("PITS_SPEED_LIMITER", "SPEED_KMH", opts.pits_speed_kmh);
    ini.set(
        "WEATHER_FX",
        "USE_CUSTOM_WEATHER",
        u8::from(opts.use_custom_weather),
    );
    ini.set("WEATHER_FX", "FORCE_RAIN", u8::from(opts.force_rain));
    if !opts.content_url.is_empty() {
        ini.set("CONTENT_URL", "URL", &opts.content_url);
    }
    if opts.minimum_csp_build > 0 {
        ini.set(
            "CUSTOM_PHYSICS",
            "MINIMUM_CSP_VERSION",
            opts.minimum_csp_build,
        );
    }
    ini.save_with_backup(&path)
}

/// Typed subset of AssettoServer's `extra_cfg.yml`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraCfgYml {
    pub enable_ai: bool,
    pub enable_client_messages: bool,
    pub enable_real_penalty: bool,
    pub content_url: String,
    pub minimum_csp_version: u32,
    pub ai_max_density: u32,
    pub ai_max_speed_kph: u32,
}

impl Default for ExtraCfgYml {
    fn default() -> Self {
        Self {
            enable_ai: false,
            enable_client_messages: true,
            enable_real_penalty: false,
            content_url: String::new(),
            minimum_csp_version: 0,
            ai_max_density: 30,
            ai_max_speed_kph: 120,
        }
    }
}

/// Reads `<cfg_dir>/extra_cfg.yml` using a minimal line-based YAML parser for known keys.
pub fn read_extra_cfg_yml(cfg_dir: &Path) -> io::Result<ExtraCfgYml> {
    let text = fs::read_to_string(cfg_dir.join(EXTRA_CFG_YML))?;
    Ok(parse_extra_cfg_yml(&text))
}

/// Parses known top-level and `AiParams:` keys from `extra_cfg.yml` text.
pub fn parse_extra_cfg_yml(text: &str) -> ExtraCfgYml {
    let mut cfg = ExtraCfgYml::default();
    let mut in_ai_params = false;
    for raw_line in text.lines() {
        let stripped = raw_line.split('#').next().unwrap_or("");
        if stripped.trim().is_empty() {
            continue;
        }
        let indented = stripped.starts_with(' ') || stripped.starts_with('\t');
        let line = stripped.trim();
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');
        if !indented {
            in_ai_params = key == "AiParams";
            match key {
                "EnableAi" => cfg.enable_ai = parse_bool(val, cfg.enable_ai),
                "EnableClientMessages" => {
                    cfg.enable_client_messages = parse_bool(val, cfg.enable_client_messages)
                }
                "EnableRealPenalty" => {
                    cfg.enable_real_penalty = parse_bool(val, cfg.enable_real_penalty)
                }
                "ContentUrl" => cfg.content_url = val.to_string(),
                "MinimumCspVersion" => {
                    cfg.minimum_csp_version = val.parse().unwrap_or(cfg.minimum_csp_version)
                }
                _ => {}
            }
        } else if in_ai_params {
            match key {
                "MaxAiSafetyDistanceMeters" | "MaxDensity" => {
                    cfg.ai_max_density = val.parse().unwrap_or(cfg.ai_max_density)
                }
                "MaxSpeedKph" => {
                    cfg.ai_max_speed_kph = val.parse().unwrap_or(cfg.ai_max_speed_kph)
                }
                _ => {}
            }
        }
    }
    cfg
}

/// Serializes [`ExtraCfgYml`] into deterministic YAML.
pub fn format_extra_cfg_yml(cfg: &ExtraCfgYml) -> String {
    let escaped_url = cfg.content_url.replace('"', "\\\"");
    format!(
        "EnableAi: {}\n\
         EnableClientMessages: {}\n\
         EnableRealPenalty: {}\n\
         ContentUrl: \"{}\"\n\
         MinimumCspVersion: {}\n\
         AiParams:\n\
         \x20\x20MaxDensity: {}\n\
         \x20\x20MaxSpeedKph: {}\n",
        cfg.enable_ai,
        cfg.enable_client_messages,
        cfg.enable_real_penalty,
        escaped_url,
        cfg.minimum_csp_version,
        cfg.ai_max_density,
        cfg.ai_max_speed_kph,
    )
}

/// Writes `<cfg_dir>/extra_cfg.yml`, backing up any existing file first.
pub fn write_extra_cfg_yml(cfg_dir: &Path, cfg: &ExtraCfgYml) -> io::Result<()> {
    if cfg.content_url.contains(['\r', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "content_url contains a line break",
        ));
    }
    let path = cfg_dir.join(EXTRA_CFG_YML);
    if path.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut backup = path.with_file_name(format!("{EXTRA_CFG_YML}.acxs-backup-{stamp}"));
        let mut n = 1u32;
        while backup.exists() {
            backup = path.with_file_name(format!("{EXTRA_CFG_YML}.acxs-backup-{stamp}-{n}"));
            n += 1;
        }
        fs::copy(&path, &backup)?;
    }
    fs::write(&path, format_extra_cfg_yml(cfg))
}

fn ini_flag(ini: &Ini, section: &str, key: &str, default: bool) -> bool {
    ini.get_i64(section, key).map(|v| v != 0).unwrap_or(default)
}

fn ini_u32(ini: &Ini, section: &str, key: &str, default: u32) -> u32 {
    ini.get_i64(section, key)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(default)
}

fn parse_bool(val: &str, default: bool) -> bool {
    match val.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => true,
        "false" | "0" | "no" => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_extra_options_ini_and_extra_cfg_yml_round_trip() {
        let dir = std::env::temp_dir().join(format!("acxs-server-csp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let csp = CspExtraOptions {
            allow_wrong_way: true,
            enforce_back_to_pits_penalty: false,
            no_collisions: true,
            pits_keep_collisions: true,
            pits_non_lethal: false,
            pits_speed_kmh: 60,
            use_custom_weather: true,
            force_rain: true,
            content_url: "https://example.com/content.json".into(),
            minimum_csp_build: 2651,
        };
        write_csp_extra_options(&dir, &csp).unwrap();
        assert_eq!(read_csp_extra_options(&dir).unwrap(), csp);

        let yml = ExtraCfgYml {
            enable_ai: true,
            enable_client_messages: false,
            enable_real_penalty: true,
            content_url: "https://example.com/pack.zip".into(),
            minimum_csp_version: 3334,
            ai_max_density: 45,
            ai_max_speed_kph: 140,
        };
        write_extra_cfg_yml(&dir, &yml).unwrap();
        assert_eq!(read_extra_cfg_yml(&dir).unwrap(), yml);

        // Second write creates a backup.
        write_extra_cfg_yml(&dir, &yml).unwrap();
        let has_backup = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with("extra_cfg.yml.acxs-backup-"));
        assert!(has_backup);

        let _ = fs::remove_dir_all(&dir);
    }
}

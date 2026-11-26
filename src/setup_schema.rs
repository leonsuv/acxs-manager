//! Parser for Assetto Corsa `data/setup.ini` (from `data.acd` or `data/`).
//!
//! Gives `setups.rs` and the Setup Editor real car-specific tabs, human-readable
//! labels, min/max/step ranges, click display mode, and optional LUT value labels.

use std::path::Path;

use crate::cardata;
use crate::ini::Ini;

/// One configurable parameter declared in a car's `setup.ini`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupParamSpec {
    /// Section name in both `setup.ini` and user setup `.ini` files (e.g. `PRESSURE_LF`).
    pub section: String,
    /// Tab heading in AC's setup screen (e.g. `TYRES`, `AERO`, `SUSPENSION`).
    pub tab: String,
    /// Human-readable display name (e.g. `Pressure LF`).
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// When true, AC displays integer click offsets instead of raw values.
    pub show_clicks: bool,
    pub pos_x: Option<f64>,
    pub pos_y: Option<f64>,
    pub help: Option<String>,
    /// Optional LUT mapping raw numeric index -> display label (e.g. gear ratios or compounds).
    pub lut: Option<Vec<(f64, String)>>,
}

/// Complete parsed setup schema for one car.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetupSchema {
    pub params: Vec<SetupParamSpec>,
}

impl SetupSchema {
    /// Distinct tab names in declaration order.
    pub fn tabs(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for p in &self.params {
            if !out.contains(&p.tab.as_str()) {
                out.push(p.tab.as_str());
            }
        }
        out
    }

    /// Lookup a parameter spec by section name (case-insensitive).
    pub fn find(&self, section: &str) -> Option<&SetupParamSpec> {
        self.params
            .iter()
            .find(|p| p.section.eq_ignore_ascii_case(section))
    }

    /// All parameters belonging to `tab` (case-insensitive).
    pub fn params_in_tab<'a>(&'a self, tab: &str) -> Vec<&'a SetupParamSpec> {
        self.params
            .iter()
            .filter(|p| p.tab.eq_ignore_ascii_case(tab))
            .collect()
    }
}

fn clean(raw: Option<&str>) -> Option<&str> {
    raw.map(|s| {
        let s = s.split(';').next().unwrap_or("");
        s.split("//").next().unwrap_or("").trim()
    })
    .filter(|s| !s.is_empty())
}

fn num(ini: &Ini, section: &str, key: &str) -> Option<f64> {
    clean(ini.get(section, key))?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn parse_label_lut(text: &str) -> Vec<(f64, String)> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with(';')
                || line.starts_with("//")
                || line.starts_with('#')
            {
                return None;
            }
            let (k, v) = line.split_once('|')?;
            let k = k.trim().parse::<f64>().ok()?;
            let v = v.split(';').next().unwrap_or("").trim().to_string();
            (k.is_finite() && !v.is_empty()).then_some((k, v))
        })
        .collect()
}

/// Parses `setup.ini` text directly (without external LUT file resolution).
pub fn parse_setup_schema(ini_text: &str) -> SetupSchema {
    parse_setup_schema_with_luts(ini_text, |_| None)
}

fn parse_setup_schema_with_luts<F>(ini_text: &str, mut read_lut: F) -> SetupSchema
where
    F: FnMut(&str) -> Option<String>,
{
    let ini = Ini::parse(ini_text);
    let mut params = Vec::new();
    for section in ini.section_names() {
        if section.eq_ignore_ascii_case("DISPLAY_METHOD")
            || section.eq_ignore_ascii_case("HEADER")
            || section.eq_ignore_ascii_case("GEARS")
        {
            continue;
        }
        let Some(min) = num(&ini, section, "MIN") else {
            continue;
        };
        let Some(max) = num(&ini, section, "MAX") else {
            continue;
        };
        let step = num(&ini, section, "STEP")
            .filter(|&s| s > 0.0)
            .unwrap_or(1.0);
        let tab = clean(ini.get(section, "TAB"))
            .unwrap_or("GENERIC")
            .to_string();
        let name = clean(ini.get(section, "NAME"))
            .unwrap_or(section)
            .to_string();
        let show_clicks = clean(ini.get(section, "SHOW_CLICKS")) == Some("1");
        let pos_x = num(&ini, section, "POS_X");
        let pos_y = num(&ini, section, "POS_Y");
        let help = clean(ini.get(section, "HELP")).map(str::to_string);
        let lut = clean(ini.get(section, "LUT"))
            .and_then(|lut_name| read_lut(lut_name))
            .map(|text| parse_label_lut(&text))
            .filter(|entries| !entries.is_empty());

        params.push(SetupParamSpec {
            section: section.to_string(),
            tab,
            name,
            min,
            max,
            step,
            show_clicks,
            pos_x,
            pos_y,
            help,
            lut,
        });
    }
    SetupSchema { params }
}

/// Reads and parses `setup.ini` (plus any referenced LUTs) from a car's `data.acd` or `data/`.
pub fn read_setup_schema(car_dir: &Path) -> Option<SetupSchema> {
    let text = cardata::read_data_text(car_dir, "setup.ini").ok()?;
    Some(parse_setup_schema_with_luts(&text, |lut_name| {
        cardata::read_data_text(car_dir, lut_name).ok()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_tabs_bounds_clicks_and_luts() {
        let dir = std::env::temp_dir().join(format!("acxs_setup_schema_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::write(
            dir.join("data").join("setup.ini"),
            "[DISPLAY_METHOD]\r\nSHOW_CLICKS=0\r\n\r\n\
             [PRESSURE_LF]\r\nSHOW_CLICKS=0\r\nTAB=TYRES\r\nNAME=Pressure LF\r\nMIN=16\r\nMAX=32\r\nSTEP=1\r\nPOS_X=0\r\nPOS_Y=2\r\nHELP=HELP_LF_PRESSURE\r\n\r\n\
             [WING_1]\r\nSHOW_CLICKS=1\r\nTAB=AERO\r\nNAME=Rear Wing\r\nMIN=0\r\nMAX=12\r\nSTEP=1\r\n\r\n\
             [FINAL_RATIO]\r\nSHOW_CLICKS=0\r\nTAB=GEARS\r\nNAME=Final Gear Ratio\r\nMIN=0\r\nMAX=2\r\nSTEP=1\r\nLUT=final.rto\r\n",
        )
        .unwrap();
        fs::write(
            dir.join("data").join("final.rto"),
            "0|3.44\r\n1|3.89\r\n2|4.10\r\n",
        )
        .unwrap();

        let schema = read_setup_schema(&dir).unwrap();
        assert_eq!(schema.tabs(), vec!["TYRES", "AERO", "GEARS"]);
        let p = schema.find("pressure_lf").unwrap();
        assert_eq!(p.name, "Pressure LF");
        assert_eq!(p.min, 16.0);
        assert_eq!(p.max, 32.0);
        assert!(!p.show_clicks);

        let w = schema.find("WING_1").unwrap();
        assert!(w.show_clicks);
        assert_eq!(w.tab, "AERO");

        let f = schema.find("FINAL_RATIO").unwrap();
        let lut = f.lut.as_ref().unwrap();
        assert_eq!(lut.len(), 3);
        assert_eq!(lut[1], (1.0, "3.89".to_string()));

        let _ = fs::remove_dir_all(&dir);
    }
}

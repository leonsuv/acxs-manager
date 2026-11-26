//! One-click atomic Rig & Profile Switcher (VR ↔ Triple ↔ Flat / Wheel ↔ Gamepad).
//!
//! Bundles `VideoSettings`, `ControlsConfig`, `FfbPostProcess`, and `PadSetup` into a named
//! profile stored in `%APPDATA%\ACXS\rig-profiles.json`. Applying a profile refuses to run
//! while `acs.exe` is active, snapshots all touched files before writing, and rolls back
//! every file to its pre-switch bytes if any single write fails.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::controls::{self, ControlsConfig};
use crate::ffb::{self, FfbPostProcess};
use crate::game_cfg::{self, TripleScreenSettings, VideoSettings, VrSettings};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigProfile {
    pub name: String,
    pub video: Option<VideoSettings>,
    pub triple_screen: Option<TripleScreenSettings>,
    pub vr_oculus: Option<VrSettings>,
    pub vr_openvr: Option<VrSettings>,
    pub controls: Option<ControlsConfig>,
    pub ffb_post_process: Option<FfbPostProcess>,
}

impl Default for RigProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            video: None,
            triple_screen: None,
            vr_oculus: None,
            vr_openvr: None,
            controls: None,
            ffb_post_process: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigProfileStore {
    pub profiles: Vec<RigProfile>,
}

impl RigProfileStore {
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad rig profiles: {e}"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
    }

    pub fn upsert(&mut self, profile: RigProfile) -> io::Result<()> {
        let name = profile.name.trim();
        if name.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Rig profile name cannot be empty"));
        }
        if let Some(pos) = self.profiles.iter().position(|p| p.name.eq_ignore_ascii_case(name)) {
            self.profiles[pos] = profile;
        } else {
            self.profiles.push(profile);
        }
        self.profiles.sort_by_key(|p| p.name.to_lowercase());
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RigProfile> {
        self.profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }
}

/// Captures original file contents so any mid-switch failure restores all files cleanly.
struct RollbackGuard {
    originals: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl RollbackGuard {
    fn snapshot(paths: &[PathBuf]) -> io::Result<Self> {
        let mut originals = Vec::with_capacity(paths.len());
        for p in paths {
            let bytes = match fs::read(p) {
                Ok(b) => Some(b),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            originals.push((p.clone(), bytes));
        }
        Ok(Self { originals })
    }

    fn rollback(&self) {
        for (path, orig) in &self.originals {
            match orig {
                Some(bytes) => {
                    let _ = fs::write(path, bytes);
                }
                None => {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}

/// Atomically applies `profile` to `cfg_dir`. Refuses if `acs.exe` is running.
/// If any step fails, every previously modified file is rolled back to its exact prior bytes.
pub fn apply_rig_profile(cfg_dir: &Path, profile: &RigProfile) -> io::Result<()> {
    apply_rig_profile_with_guard(cfg_dir, profile, crate::game::is_game_running())
}

pub fn apply_rig_profile_with_guard(
    cfg_dir: &Path,
    profile: &RigProfile,
    game_running: bool,
) -> io::Result<()> {
    if game_running {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "Assetto Corsa is running; close it before switching rig profiles.",
        ));
    }

    let targets = [
        cfg_dir.join(game_cfg::VIDEO_FILE),
        cfg_dir.join(game_cfg::TRIPLE_SCREEN_FILE),
        cfg_dir.join(game_cfg::OCULUS_FILE),
        cfg_dir.join(game_cfg::OPENVR_FILE),
        cfg_dir.join(controls::CONTROLS_FILE),
        cfg_dir.join(ffb::FF_POST_PROCESS_FILE),
    ];
    let guard = RollbackGuard::snapshot(&targets)?;

    let res = (|| -> io::Result<()> {
        if let Some(video) = &profile.video {
            game_cfg::write_video(cfg_dir, video)?;
        }
        if let Some(triple) = &profile.triple_screen {
            game_cfg::write_triple_screen(cfg_dir, triple)?;
        }
        if let Some(vr) = &profile.vr_oculus {
            game_cfg::write_vr(cfg_dir, false, vr)?;
        }
        if let Some(vr) = &profile.vr_openvr {
            game_cfg::write_vr(cfg_dir, true, vr)?;
        }
        if let Some(ctrl) = &profile.controls {
            controls::write_controls(cfg_dir, ctrl)?;
        }
        if let Some(pp) = &profile.ffb_post_process {
            ffb::write_post_process(cfg_dir, pp)?;
        }
        Ok(())
    })();

    if res.is_err() {
        guard.rollback();
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_cfg::CameraMode;

    #[test]
    fn atomic_rig_switch_rolls_back_all_files_on_mid_write_error() {
        let dir = std::env::temp_dir().join(format!("acxs-rig-rollback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let original_video = "[CAMERA]\r\nMODE=DEFAULT\r\n";
        fs::write(dir.join(game_cfg::VIDEO_FILE), original_video).unwrap();

        // Video is valid (OpenVR), but VR settings have invalid PPD=9.9 -> fails mid-switch!
        let bad_profile = RigProfile {
            name: "Broken VR".into(),
            video: Some(VideoSettings {
                camera_mode: CameraMode::OpenVr,
                ..VideoSettings::default()
            }),
            vr_openvr: Some(VrSettings {
                pixel_per_display: 9.9,
                ..VrSettings::default()
            }),
            ..RigProfile::default()
        };

        let err = apply_rig_profile(&dir, &bad_profile).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        // Video file MUST be rolled back to exact original bytes!
        assert_eq!(fs::read_to_string(dir.join(game_cfg::VIDEO_FILE)).unwrap(), original_video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_switch_while_acs_running() {
        let dir = std::env::temp_dir().join(format!("acxs-rig-busy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let err = apply_rig_profile_with_guard(&dir, &RigProfile::default(), true).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ResourceBusy);
        let _ = fs::remove_dir_all(&dir);
    }
}

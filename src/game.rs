//! Check whether Assetto Corsa is running and launch it.

use crate::settings::AC_EXECUTABLE;
use std::io;
use std::path::Path;
use std::process::{Child, Command};

/// True when `acs.exe` is running. Only Windows can answer this, elsewhere it is false.
pub fn is_game_running() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let mut command = Command::new("tasklist");
    command.args(["/FI", &format!("IMAGENAME eq {AC_EXECUTABLE}"), "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.output() {
        Ok(output) => tasklist_lists_game(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => false,
    }
}

/// True when `tasklist` output names the game executable. Matching ignores case.
/// The "no tasks are running" message and empty output do not name it, so they give false.
fn tasklist_lists_game(output: &str) -> bool {
    output.to_lowercase().contains(&AC_EXECUTABLE.to_lowercase())
}

/// Steam app id of Assetto Corsa.
const STEAM_APP_ID: &str = "244210";

pub fn launch_game(ac_root: &Path) -> io::Result<Child> {
    let exe = ac_root.join(AC_EXECUTABLE);
    if !exe.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{AC_EXECUTABLE} not found in {}", ac_root.display()),
        ));
    }
    // `acs.exe` hands over to Steam to restart the game through its launcher unless it already
    // knows which Steam app it is. On a PC where the launcher is Content Manager that opens
    // Content Manager instead of the race, so name the app up front.
    Command::new(&exe)
        .current_dir(ac_root)
        .env("SteamAppId", STEAM_APP_ID)
        .env("SteamGameId", STEAM_APP_ID)
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A unique empty temp directory, removed on drop (also when a test panics).
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> TempRoot {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("acxs-game-{}-{n}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempRoot(dir)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tasklist_no_tasks_message_is_not_running() {
        let output = "INFO: No tasks are running which match the specified criteria.\r\n";
        assert!(!tasklist_lists_game(output));
    }

    #[test]
    fn tasklist_empty_output_is_not_running() {
        assert!(!tasklist_lists_game(""));
    }

    #[test]
    fn tasklist_matching_line_is_running() {
        let output = "acs.exe                       4312 Console                    1    512,344 K\r\n";
        assert!(tasklist_lists_game(output));
    }

    #[test]
    fn tasklist_match_ignores_case() {
        let output = "ACS.EXE                       4312 Console                    1    512,344 K\r\n";
        assert!(tasklist_lists_game(output));
    }

    #[test]
    fn tasklist_other_process_is_not_running() {
        let output = "notepad.exe                   1200 Console                    1     12,000 K\r\n";
        assert!(!tasklist_lists_game(output));
    }

    #[test]
    fn launch_game_missing_exe_is_not_found_and_not_spawned() {
        let dir = TempRoot::new("missing-exe");
        let err = launch_game(&dir.0).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        // The message comes from this function, so the spawn path was never reached.
        assert!(err.to_string().contains(&format!("{AC_EXECUTABLE} not found in")));
    }

    #[test]
    fn launch_game_directory_named_like_exe_is_not_found() {
        let dir = TempRoot::new("dir-named-exe");
        fs::create_dir_all(dir.0.join(AC_EXECUTABLE)).unwrap();
        let err = launch_game(&dir.0).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}

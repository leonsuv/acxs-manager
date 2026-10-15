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
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains(&AC_EXECUTABLE.to_lowercase()),
        Err(_) => false,
    }
}

pub fn launch_game(ac_root: &Path) -> io::Result<Child> {
    let exe = ac_root.join(AC_EXECUTABLE);
    if !exe.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{AC_EXECUTABLE} not found in {}", ac_root.display()),
        ));
    }
    Command::new(&exe).current_dir(ac_root).spawn()
}

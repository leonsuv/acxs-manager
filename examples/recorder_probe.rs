//! Runs the session recorder against whatever is in the game's shared memory for a while.
//! Used with `scripts/fake-ac-drive.ps1`, which plays a scripted drive into fake shared memory:
//! `recorder_probe <seconds>`; set `APPDATA` to a scratch folder first so nothing real is touched.

use std::sync::Arc;
use std::time::Duration;

use acxs_manager::session_recorder::{self, RecorderControl};

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let control = Arc::new(RecorderControl::default());
    session_recorder::spawn(Arc::clone(&control), || println!("saved a session"));
    std::thread::sleep(Duration::from_secs(seconds));
    println!("recording at exit: {}", control.recording.load(std::sync::atomic::Ordering::Relaxed));
    println!("sessions dir: {}", session_recorder::sessions_dir().display());
}

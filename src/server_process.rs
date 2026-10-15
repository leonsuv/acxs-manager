//! Runs the Assetto Corsa dedicated server as a managed child process.
//!
//! The server lives in `<AC root>/server`. It is started with its working directory set to that
//! folder, its standard output and error are piped, and a reader thread per pipe keeps the most
//! recent lines in a bounded buffer. [`parse_log_line`] classifies a line so the UI can flag
//! errors, joins, leaves and lap times.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use crate::server::{ENTRY_LIST, SERVER_CFG};

/// Executable name on Windows, looked up in `<AC root>/server`.
pub const SERVER_EXECUTABLE_WINDOWS: &str = "acServer.exe";

/// Executable name on every other platform, looked up in `<AC root>/server`.
pub const SERVER_EXECUTABLE_UNIX: &str = "acServer";

/// How many output lines the buffer keeps before the oldest are dropped.
pub const LOG_CAPACITY: usize = 500;

/// Name of the server executable for the platform this build runs on.
pub fn server_binary_name() -> &'static str {
    if cfg!(windows) { SERVER_EXECUTABLE_WINDOWS } else { SERVER_EXECUTABLE_UNIX }
}

/// Finds the server executable at `<ac_root>/server/<server_binary_name()>`.
///
/// Returns `NotFound` when the file is missing or is not a regular file.
pub fn find_server_executable(ac_root: &Path) -> io::Result<PathBuf> {
    let path = ac_root.join("server").join(server_binary_name());
    if path.is_file() {
        Ok(path)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, format!("server executable not found at {}", path.display())))
    }
}

/// How the server is told where its configuration is.
///
/// The server takes the settings file after `-c` and the entry list after `-e`, both as full
/// file paths. Those are the defaults. If a build expects other flags, or no entry-list flag,
/// change this value rather than the command builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchArgs {
    /// Flag placed before the path of `server_cfg.ini`.
    pub config_flag: String,
    /// Flag placed before the path of `entry_list.ini`. `None` leaves the entry list out.
    pub entry_list_flag: Option<String>,
}

impl Default for LaunchArgs {
    fn default() -> Self {
        LaunchArgs { config_flag: "-c".to_string(), entry_list_flag: Some("-e".to_string()) }
    }
}

/// The arguments for a server that reads its files from `cfg_dir`.
///
/// Produces `[config_flag, cfg_dir/server_cfg.ini, entry_list_flag, cfg_dir/entry_list.ini]`,
/// dropping the entry-list pair when `entry_list_flag` is `None`.
pub fn build_args(cfg_dir: &Path, launch: &LaunchArgs) -> Vec<OsString> {
    let mut args = vec![OsString::from(&launch.config_flag), cfg_dir.join(SERVER_CFG).into_os_string()];
    if let Some(flag) = &launch.entry_list_flag {
        args.push(OsString::from(flag));
        args.push(cfg_dir.join(ENTRY_LIST).into_os_string());
    }
    args
}

/// A ready-to-spawn command for `exe`, with arguments from [`build_args`].
///
/// The working directory is the folder that holds `exe`, which is the server folder, so the
/// server finds its content and config relative to it.
pub fn build_command(exe: &Path, cfg_dir: &Path, launch: &LaunchArgs) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(build_args(cfg_dir, launch));
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd
}

/// Where the server process is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not started, or stopped by [`ServerProcess::stop`].
    Stopped,
    /// The child is alive.
    Running,
    /// The child exited on its own. The code is `None` when it was ended by a signal.
    Exited(Option<i32>),
}

/// A line the server printed that matters to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    /// The line reports an error.
    Error,
    /// The line reports a warning.
    Warning,
    /// A driver has taken a grid slot and joined.
    PlayerJoined,
    /// A driver has disconnected or left.
    PlayerLeft,
    /// A driver completed a lap.
    LapCompleted,
}

/// Classifies one output line, or returns `None` when it is not one of the interesting kinds.
///
/// Matching ignores case. The first rule that fits wins, in this order: error, warning, joined,
/// left, lap. An error line that mentions a disconnect is therefore an error.
///
/// - Error: contains `error`, or starts with `fatal`.
/// - Warning: contains `warning`.
/// - Joined: starts with `Slot found at index`, or contains `has connected`.
/// - Left: contains `has disconnected` or `has left the server`.
/// - Lap: contains `lap completed`.
pub fn parse_log_line(line: &str) -> Option<LogKind> {
    let lower = line.trim().to_ascii_lowercase();
    if lower.contains("error") || lower.starts_with("fatal") {
        Some(LogKind::Error)
    } else if lower.contains("warning") {
        Some(LogKind::Warning)
    } else if lower.starts_with("slot found at index") || lower.contains("has connected") {
        Some(LogKind::PlayerJoined)
    } else if lower.contains("has disconnected") || lower.contains("has left the server") {
        Some(LogKind::PlayerLeft)
    } else if lower.contains("lap completed") {
        Some(LogKind::LapCompleted)
    } else {
        None
    }
}

/// A fixed-capacity buffer of lines. Pushing past capacity drops the oldest line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineBuffer {
    cap: usize,
    lines: VecDeque<String>,
}

impl LineBuffer {
    /// An empty buffer that keeps at most `cap` lines. A capacity of 0 is raised to 1.
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        LineBuffer { cap, lines: VecDeque::with_capacity(cap.min(LOG_CAPACITY)) }
    }

    /// Appends a line, dropping the oldest one when the buffer is full.
    pub fn push(&mut self, line: String) {
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    /// Maximum number of lines kept.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Number of lines currently kept.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// True when no line has been pushed.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The kept lines, oldest first.
    pub fn to_vec(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

/// The server child process and the output it has printed.
///
/// Lifecycle: `Stopped` -> [`start`](Self::start) -> `Running` -> the child exits -> `Exited(code)`.
/// [`stop`](Self::stop) kills a running child, waits for it, and leaves the status `Stopped`.
/// Dropping a `ServerProcess` stops the child. The output buffer survives restarts.
pub struct ServerProcess {
    child: Option<Child>,
    status: Status,
    lines: Arc<Mutex<LineBuffer>>,
}

impl Default for ServerProcess {
    fn default() -> Self {
        ServerProcess::new()
    }
}

impl ServerProcess {
    /// A stopped process with an empty output buffer of [`LOG_CAPACITY`] lines.
    pub fn new() -> Self {
        ServerProcess {
            child: None,
            status: Status::Stopped,
            lines: Arc::new(Mutex::new(LineBuffer::new(LOG_CAPACITY))),
        }
    }

    /// Starts the server at `exe`, reading its settings from `cfg_dir`.
    ///
    /// Fails with `AlreadyExists` when a child is still running, and with the spawn error
    /// when the executable cannot be started.
    pub fn start(&mut self, exe: &Path, cfg_dir: &Path, launch: &LaunchArgs) -> io::Result<()> {
        self.start_command(build_command(exe, cfg_dir, launch))
    }

    /// Spawns a prepared command with piped output. Shared by [`start`](Self::start) and tests.
    fn start_command(&mut self, mut cmd: Command) -> io::Result<()> {
        self.refresh();
        if self.status == Status::Running {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "server is already running"));
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        if let Some(out) = child.stdout.take() {
            spawn_reader(out, Arc::clone(&self.lines));
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, Arc::clone(&self.lines));
        }
        self.child = Some(child);
        self.status = Status::Running;
        Ok(())
    }

    /// Current status. Checks whether a running child has exited since the last call.
    pub fn status(&mut self) -> Status {
        self.refresh();
        self.status
    }

    /// The most recent output lines, oldest first, at most [`LOG_CAPACITY`].
    pub fn recent_lines(&self) -> Vec<String> {
        self.buffer().to_vec()
    }

    /// Kills the child if it is running, waits for it, and sets the status to `Stopped`.
    ///
    /// Returns the exit code of the child that was stopped or had already exited, or `None`
    /// when there was no such child or it ended by a signal.
    pub fn stop(&mut self) -> io::Result<Option<i32>> {
        let code = match self.child.take() {
            Some(mut child) => match child.try_wait()? {
                Some(status) => status.code(),
                None => {
                    // The child may exit between the check and the kill; that is not an error.
                    let _ = child.kill();
                    child.wait()?.code()
                }
            },
            None => match self.status {
                Status::Exited(code) => code,
                _ => None,
            },
        };
        self.status = Status::Stopped;
        Ok(code)
    }

    /// Moves a running child to `Exited` when it has finished.
    fn refresh(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                self.status = Status::Exited(status.code());
            }
        }
    }

    fn buffer(&self) -> MutexGuard<'_, LineBuffer> {
        self.lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Reads `pipe` line by line on a background thread until end of file, pushing each non-blank
/// line into `sink`. Bytes that are not valid UTF-8 are replaced, not treated as an error.
fn spawn_reader<R: Read + Send + 'static>(pipe: R, sink: Arc<Mutex<LineBuffer>>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut raw = Vec::new();
        loop {
            raw.clear();
            match reader.read_until(b'\n', &mut raw) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&raw);
                    let line = text.trim_end_matches(['\r', '\n']);
                    if !line.trim().is_empty() {
                        let mut buf = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        buf.push(line.to_string());
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    /// A unique directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("acxs-server-process-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn binary_name_matches_platform() {
        let name = server_binary_name();
        if cfg!(windows) {
            assert_eq!(name, "acServer.exe");
        } else {
            assert_eq!(name, "acServer");
        }
    }

    #[test]
    fn finds_executable_under_server_folder() {
        let tmp = TempDir::new("find-exe");
        assert_eq!(find_server_executable(tmp.path()).unwrap_err().kind(), io::ErrorKind::NotFound);

        let server_dir = tmp.path().join("server");
        fs::create_dir_all(&server_dir).unwrap();
        fs::write(server_dir.join(server_binary_name()), b"").unwrap();
        let found = find_server_executable(tmp.path()).unwrap();
        assert_eq!(found, server_dir.join(server_binary_name()));
    }

    #[test]
    fn build_args_point_at_cfg_files() {
        let args = build_args(Path::new("/ac/server/cfg"), &LaunchArgs::default());
        assert_eq!(
            args,
            vec![
                OsString::from("-c"),
                Path::new("/ac/server/cfg").join("server_cfg.ini").into_os_string(),
                OsString::from("-e"),
                Path::new("/ac/server/cfg").join("entry_list.ini").into_os_string(),
            ]
        );
    }

    #[test]
    fn build_args_can_be_configured() {
        let no_entry = LaunchArgs { config_flag: "--config".to_string(), entry_list_flag: None };
        let args = build_args(Path::new("cfg"), &no_entry);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], OsString::from("--config"));
        assert_eq!(args[1], Path::new("cfg").join("server_cfg.ini").into_os_string());
    }

    #[test]
    fn build_command_runs_from_server_folder() {
        let exe = Path::new("/ac/server/acServer");
        let cmd = build_command(exe, Path::new("/ac/server/cfg"), &LaunchArgs::default());
        assert_eq!(cmd.get_program(), exe.as_os_str());
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/ac/server")));
        let args: Vec<OsString> = cmd.get_args().map(|a| a.to_os_string()).collect();
        assert_eq!(args, build_args(Path::new("/ac/server/cfg"), &LaunchArgs::default()));
    }

    #[test]
    fn parses_error_and_warning_lines() {
        assert_eq!(parse_log_line("ERROR: cannot open track"), Some(LogKind::Error));
        assert_eq!(parse_log_line("Fatal problem in physics"), Some(LogKind::Error));
        assert_eq!(parse_log_line("Warning, server CPU overload"), Some(LogKind::Warning));
        assert_eq!(parse_log_line("  warning: slow frame  "), Some(LogKind::Warning));
        // Error wins over the other kinds when a line matches several.
        assert_eq!(parse_log_line("error: bob has disconnected"), Some(LogKind::Error));
    }

    #[test]
    fn parses_player_and_lap_lines() {
        assert_eq!(parse_log_line("Slot found at index 3"), Some(LogKind::PlayerJoined));
        assert_eq!(parse_log_line("Alice has connected"), Some(LogKind::PlayerJoined));
        assert_eq!(parse_log_line("Bob has disconnected"), Some(LogKind::PlayerLeft));
        assert_eq!(parse_log_line("Bob has left the server"), Some(LogKind::PlayerLeft));
        assert_eq!(parse_log_line("Lap completed by Carol 1:52.334"), Some(LogKind::LapCompleted));
    }

    #[test]
    fn ignores_ordinary_lines() {
        assert_eq!(parse_log_line("Starting HTTP server on port 8081"), None);
        assert_eq!(parse_log_line("Weather update. Ambient: 21 Road: 23"), None);
        assert_eq!(parse_log_line(""), None);
    }

    #[test]
    fn line_buffer_keeps_only_the_newest_lines() {
        let mut buf = LineBuffer::new(3);
        assert!(buf.is_empty());
        for i in 0..5 {
            buf.push(format!("line {i}"));
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.capacity(), 3);
        assert_eq!(buf.to_vec(), vec!["line 2", "line 3", "line 4"]);
    }

    #[test]
    fn line_buffer_zero_capacity_keeps_one_line() {
        let mut buf = LineBuffer::new(0);
        buf.push("a".to_string());
        buf.push("b".to_string());
        assert_eq!(buf.to_vec(), vec!["b"]);
    }

    #[test]
    fn default_process_is_stopped_and_empty() {
        let mut sp = ServerProcess::new();
        assert_eq!(sp.status(), Status::Stopped);
        assert!(sp.recent_lines().is_empty());
        assert_eq!(sp.stop().unwrap(), None);
        assert_eq!(sp.status(), Status::Stopped);
    }

    #[cfg(unix)]
    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    /// Polls `check` until it returns true or a few seconds pass.
    #[cfg(unix)]
    fn wait_until(mut check: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if check() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[cfg(unix)]
    #[test]
    fn child_exit_moves_status_to_exited_with_code() {
        let mut sp = ServerProcess::new();
        sp.start_command(sh("exit 3")).unwrap();
        assert!(wait_until(|| sp.status() != Status::Running));
        assert_eq!(sp.status(), Status::Exited(Some(3)));
        assert_eq!(sp.stop().unwrap(), Some(3));
        assert_eq!(sp.status(), Status::Stopped);
    }

    #[cfg(unix)]
    #[test]
    fn output_from_both_pipes_is_captured() {
        let mut sp = ServerProcess::new();
        sp.start_command(sh("echo first; echo second >&2; echo ''; exit 0")).unwrap();
        assert!(wait_until(|| sp.recent_lines().len() == 2));
        let lines = sp.recent_lines();
        assert!(lines.contains(&"first".to_string()), "{lines:?}");
        assert!(lines.contains(&"second".to_string()), "{lines:?}");
    }

    #[cfg(unix)]
    #[test]
    fn stop_kills_running_child_and_sets_stopped() {
        let mut sp = ServerProcess::new();
        sp.start_command(sh("echo ready; exec sleep 60")).unwrap();
        assert_eq!(sp.status(), Status::Running);
        assert!(wait_until(|| sp.recent_lines().iter().any(|l| l == "ready")));

        let code = sp.stop().unwrap();
        assert_eq!(code, None, "a killed child has no exit code");
        assert_eq!(sp.status(), Status::Stopped);
    }

    #[cfg(unix)]
    #[test]
    fn cannot_start_twice_while_running() {
        let mut sp = ServerProcess::new();
        sp.start_command(sh("exec sleep 60")).unwrap();
        let err = sp.start_command(sh("exit 0")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        sp.stop().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn can_restart_after_exit_and_keeps_output() {
        let mut sp = ServerProcess::new();
        sp.start_command(sh("echo run-one; exit 1")).unwrap();
        assert!(wait_until(|| sp.status() == Status::Exited(Some(1))));
        assert!(wait_until(|| sp.recent_lines().len() == 1));

        sp.start_command(sh("echo run-two; exit 0")).unwrap();
        assert!(wait_until(|| sp.recent_lines().len() == 2));
        assert_eq!(sp.recent_lines(), vec!["run-one", "run-two"]);
        assert!(wait_until(|| sp.status() == Status::Exited(Some(0))));
    }

    #[test]
    fn missing_executable_is_a_spawn_error() {
        let tmp = TempDir::new("missing-exe");
        let mut sp = ServerProcess::new();
        let err = sp.start(&tmp.path().join("no-such-acServer"), tmp.path(), &LaunchArgs::default()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(sp.status(), Status::Stopped);
    }
}

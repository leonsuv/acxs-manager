//! Single-instance IPC over a local Windows Named Pipe (`\\.\pipe\ACXS-Manager-IPC-v1`).
//!
//! When a second instance is launched (for example from an `acmanager://` link click),
//! [`try_forward`] sends the raw URI (or `FOCUS`) to the already-running instance so it
//! focuses its window and opens the server. On non-Windows hosts this module provides a
//! clean no-op stub.

use crate::uri::{JoinUri, parse_join_uri};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Windows Named Pipe path for single-instance forwarding.
pub const PIPE_NAME: &str = r"\\.\pipe\ACXS-Manager-IPC-v1";

/// Maximum IPC message payload in bytes.
pub const MAX_MESSAGE_BYTES: usize = 4096;

/// Control token sent when a second instance is launched without a URI argument.
pub const FOCUS_TOKEN: &str = "FOCUS";

/// Message delivered to the primary instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcMessage {
    Focus,
    OpenUri(JoinUri),
}

/// Parses one raw line received over the IPC pipe into an [`IpcMessage`].
pub fn decode_message(raw: &str) -> Option<IpcMessage> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == FOCUS_TOKEN {
        return Some(IpcMessage::Focus);
    }
    parse_join_uri(trimmed).ok().map(IpcMessage::OpenUri)
}

/// Encodes an optional raw URI argument into a single-line IPC payload.
pub fn encode_message(raw_uri: Option<&str>) -> Vec<u8> {
    let body = raw_uri
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(FOCUS_TOKEN);
    let mut out = body.as_bytes().iter().copied().take(MAX_MESSAGE_BYTES - 1).collect::<Vec<_>>();
    out.push(b'\n');
    out
}

/// Handle to a running IPC listener. Dropping it signals the listener to stop.
pub struct IpcListener {
    shutdown: Arc<AtomicBool>,
}

impl IpcListener {
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

impl Drop for IpcListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Attempts to forward `raw_uri` (or `FOCUS`) to an already-running ACXS instance.
/// Returns `true` when the message was delivered and this secondary process should exit.
#[cfg(windows)]
pub fn try_forward(raw_uri: Option<&str>) -> bool {
    win::try_forward_pipe(raw_uri)
}

/// Starts listening on [`PIPE_NAME`] on a background thread and invokes `on_msg` for each
/// incoming [`IpcMessage`].
#[cfg(windows)]
pub fn start_listener(on_msg: impl Fn(IpcMessage) + Send + 'static) -> IpcListener {
    win::start_pipe_listener(on_msg)
}

#[cfg(not(windows))]
pub fn try_forward(_raw_uri: Option<&str>) -> bool {
    false
}

#[cfg(not(windows))]
pub fn start_listener(_on_msg: impl Fn(IpcMessage) + Send + 'static) -> IpcListener {
    IpcListener {
        shutdown: Arc::new(AtomicBool::new(false)),
    }
}

#[cfg(windows)]
mod win {
    use super::{IpcListener, IpcMessage, MAX_MESSAGE_BYTES, PIPE_NAME, decode_message, encode_message};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FlushFileBuffers, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    };

    const GENERIC_WRITE: u32 = 0x4000_0000;
    const PIPE_ACCESS_INBOUND: u32 = 0x0000_0001;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    /// Reject remote SMB pipe clients so only local processes can send IPC messages.
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub(super) fn try_forward_pipe(raw_uri: Option<&str>) -> bool {
        let name = wide(PIPE_NAME);
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return false;
        }
        let payload = encode_message(raw_uri);
        let mut written: u32 = 0;
        let ok = unsafe {
            WriteFile(
                handle,
                payload.as_ptr(),
                payload.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            ) != 0
        };
        unsafe {
            FlushFileBuffers(handle);
            CloseHandle(handle);
        }
        ok && written as usize == payload.len()
    }

    pub(super) fn start_pipe_listener(on_msg: impl Fn(IpcMessage) + Send + 'static) -> IpcListener {
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&shutdown);
        let _ = thread::Builder::new().name("acxs-ipc-pipe".into()).spawn(move || {
            let name = wide(PIPE_NAME);
            while !stop.load(Ordering::Acquire) {
                let pipe = unsafe {
                    CreateNamedPipeW(
                        name.as_ptr(),
                        PIPE_ACCESS_INBOUND,
                        PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                        1,
                        MAX_MESSAGE_BYTES as u32,
                        MAX_MESSAGE_BYTES as u32,
                        0,
                        std::ptr::null(),
                    )
                };
                if pipe == INVALID_HANDLE_VALUE || pipe.is_null() {
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }
                let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) != 0 };
                if connected || !stop.load(Ordering::Acquire) {
                    let mut buf = vec![0u8; MAX_MESSAGE_BYTES];
                    let mut read_len: u32 = 0;
                    let ok = unsafe {
                        ReadFile(
                            pipe,
                            buf.as_mut_ptr(),
                            buf.len() as u32,
                            &mut read_len,
                            std::ptr::null_mut(),
                        ) != 0
                    };
                    if ok && read_len > 0 {
                        let text = String::from_utf8_lossy(&buf[..read_len as usize]);
                        if let Some(msg) = decode_message(&text) {
                            on_msg(msg);
                        }
                    }
                    unsafe {
                        DisconnectNamedPipe(pipe);
                    }
                }
                unsafe {
                    CloseHandle(pipe);
                }
            }
        });
        IpcListener { shutdown }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_focus_and_uri_messages() {
        let focus_bytes = encode_message(None);
        assert_eq!(focus_bytes, b"FOCUS\n");
        assert_eq!(decode_message(&String::from_utf8(focus_bytes).unwrap()), Some(IpcMessage::Focus));

        let uri = "acmanager://race/online/join?ip=127.0.0.1&httpPort=8081";
        let uri_bytes = encode_message(Some(uri));
        match decode_message(&String::from_utf8(uri_bytes).unwrap()) {
            Some(IpcMessage::OpenUri(parsed)) => {
                assert_eq!(parsed.host, "127.0.0.1");
                assert_eq!(parsed.http_port, 8081);
            }
            other => panic!("expected OpenUri, got {other:?}"),
        }
    }

    #[test]
    fn invalid_uri_returns_none_instead_of_panicking() {
        assert_eq!(decode_message("not a valid uri"), None);
    }
}

pub mod content;
pub mod drive;
pub mod garage;
pub mod online;
pub mod setup;
pub mod telemetry;

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![ping]
}

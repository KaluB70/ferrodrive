// Release builds are GUI apps on Windows; CLI use attaches to the parent console.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use ferrodrive_core::{Report, Scan};
use serde::Serialize;

fn main() -> ExitCode {
    // Any argument means CLI use: `ferrodrive check` behaves like `ferro check`.
    if std::env::args_os().len() > 1 {
        attach_console();
        return ferrodrive_cli::run(std::env::args_os());
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![scan, check])
        .run(tauri::generate_context!())
        .expect("failed to start the Ferrodrive window");
    ExitCode::SUCCESS
}

#[derive(Serialize)]
struct CheckResult {
    scan: Scan,
    report: Report,
}

#[tauri::command]
async fn scan() -> Result<Scan, String> {
    blocking(|| ferrodrive_core::scan().map_err(|e| e.to_string())).await
}

#[tauri::command]
async fn check() -> Result<CheckResult, String> {
    blocking(|| {
        let scan = ferrodrive_core::scan().map_err(|e| e.to_string())?;
        let report = ferrodrive_core::check(&scan);
        Ok(CheckResult { scan, report })
    })
    .await
}

/// Runs slow scans and network lookups off the UI thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// A GUI-subsystem exe has no console; borrow the terminal it was started from.
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call; failure just means there's no parent console.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
fn attach_console() {}

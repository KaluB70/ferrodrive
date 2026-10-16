use std::process::ExitCode;

fn main() -> ExitCode {
    let code = ferrodrive_cli::run(std::env::args_os());
    if launched_from_explorer() {
        println!("\nPress Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    code
}

/// True when the console window belongs to this process alone, which is what
/// happens when the exe is double-clicked instead of run from a terminal.
#[cfg(windows)]
fn launched_from_explorer() -> bool {
    use windows::Win32::System::Console::GetConsoleProcessList;

    let mut processes = [0u32; 2];
    // SAFETY: the buffer is valid for its length; the call only writes into it.
    unsafe { GetConsoleProcessList(&mut processes) == 1 }
}

#[cfg(not(windows))]
fn launched_from_explorer() -> bool {
    false
}

use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use ferrodrive_core::{Device, Scan, Update};

/// Ferrodrive: find driver, BIOS and firmware updates for this machine.
#[derive(Parser)]
#[command(name = "ferro", version, about)]
struct Cli {
    /// Defaults to `scan` when omitted.
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List this machine's firmware, devices and installed drivers.
    Scan {
        /// Print machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Include devices that have no driver installed.
        #[arg(long)]
        all: bool,
    },
    /// Look up driver updates for this machine (Windows Update for now).
    Check {
        /// Print machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Some(Command::Scan { json, all }) => scan(json, all),
        Some(Command::Check { json }) => check(json),
        None => scan(false, false),
    };
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

fn scan(json: bool, all: bool) -> ExitCode {
    let started = Instant::now();
    let mut scan = match ferrodrive_core::scan() {
        Ok(scan) => scan,
        Err(e) => {
            eprintln!("ferro: scan failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !all {
        scan.devices.retain(|d| d.driver.is_some());
    }
    let elapsed = started.elapsed();

    if json {
        match serde_json::to_string_pretty(&scan) {
            Ok(out) => println!("{out}"),
            Err(e) => {
                eprintln!("ferro: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        print_scan(&scan);
        println!("\n{} devices in {:.0?}", scan.devices.len(), elapsed);
    }
    ExitCode::SUCCESS
}

fn check(json: bool) -> ExitCode {
    let started = Instant::now();
    if !json {
        println!("Searching Windows Update for drivers, this can take a minute...");
    }
    // The online search dominates; scan the hardware meanwhile.
    let (scan, updates) = std::thread::scope(|scope| {
        let updates = scope.spawn(ferrodrive_core::check);
        let scan = ferrodrive_core::scan();
        (scan, updates.join().expect("update search panicked"))
    });
    let (scan, updates) = match (scan, updates) {
        (Ok(scan), Ok(updates)) => (scan, updates),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("ferro: check failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let matches: Vec<(Option<&Device>, &Update)> = updates
        .iter()
        .map(|update| (matching_device(&scan, update), update))
        .collect();

    if json {
        let entries: Vec<_> = matches
            .iter()
            .map(|(device, update)| {
                serde_json::json!({
                    "update": update,
                    "device": device.map(|d| &d.instance_id),
                    "installed": device.and_then(|d| d.driver.as_ref()),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }

    if matches.is_empty() {
        println!("\nNo driver updates offered. ({:.0?})", started.elapsed());
        return ExitCode::SUCCESS;
    }
    println!();
    for (device, update) in &matches {
        let installed = device
            .and_then(|d| d.driver.as_ref())
            .map(|d| {
                format!(
                    "{} ({})",
                    d.version.as_deref().unwrap_or("-"),
                    d.date.as_deref().unwrap_or("-")
                )
            })
            .unwrap_or_else(|| "-".into());
        println!("{}", update.title);
        if let Some(device) = device {
            println!(
                "  device     {}",
                device.name.as_deref().unwrap_or(&device.instance_id)
            );
        }
        println!("  installed  {installed}");
        println!(
            "  offered    {}",
            update.driver_date.as_deref().unwrap_or("-")
        );
    }
    println!("\n{} updates in {:.0?}", matches.len(), started.elapsed());
    ExitCode::SUCCESS
}

/// The device an update targets, matched on hardware id (case-insensitive).
fn matching_device<'a>(scan: &'a Scan, update: &Update) -> Option<&'a Device> {
    let id = update.hardware_id.as_deref()?;
    scan.devices
        .iter()
        .find(|d| d.hardware_ids.iter().any(|h| h.eq_ignore_ascii_case(id)))
}

fn print_scan(scan: &Scan) {
    let or_dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
    let system = &scan.system;
    let firmware = &scan.firmware;

    println!(
        "System    {} {}",
        or_dash(&system.manufacturer),
        or_dash(&system.product)
    );
    println!(
        "Board     {} {}",
        or_dash(&system.board_manufacturer),
        or_dash(&system.board_product)
    );
    println!(
        "BIOS      {} {} ({})",
        or_dash(&firmware.vendor),
        or_dash(&firmware.version),
        or_dash(&firmware.release_date)
    );
    println!();
    println!(
        "{:<12} {:<48} {:<20} {:<10}",
        "CLASS", "DEVICE", "DRIVER", "DATE"
    );

    let mut devices: Vec<&Device> = scan.devices.iter().collect();
    devices.sort_by(|a, b| (&a.class, &a.name).cmp(&(&b.class, &b.name)));
    for device in devices {
        let driver = device.driver.as_ref();
        let name = device
            .name
            .clone()
            .or_else(|| device.hardware_ids.first().cloned())
            .unwrap_or_else(|| device.instance_id.clone());
        println!(
            "{:<12} {:<48} {:<20} {:<10}",
            truncate(&or_dash(&device.class), 12),
            truncate(&name, 48),
            truncate(
                &driver
                    .and_then(|d| d.version.clone().or(d.package.clone()))
                    .unwrap_or_else(|| "-".into()),
                20
            ),
            driver
                .and_then(|d| d.date.clone())
                .unwrap_or_else(|| "-".into()),
        );
    }
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        value.to_owned()
    } else {
        let cut: String = value.chars().take(width - 1).collect();
        format!("{cut}…")
    }
}

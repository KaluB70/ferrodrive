//! The `ferro` command line, shared by the `ferro` binary and the desktop app.

use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use ferrodrive_core::{Device, Kind, Scan};

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
    /// Look up driver and BIOS updates for this machine.
    Check {
        /// Print machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

/// Runs the CLI with these arguments (the first one is the program name).
pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    match cli.command {
        Some(Command::Scan { json, all }) => scan(json, all),
        Some(Command::Check { json }) => check(json),
        None => scan(false, false),
    }
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
        println!("Checking Windows Update and vendor sources, this can take a minute...");
    }
    let scan = match ferrodrive_core::scan() {
        Ok(scan) => scan,
        Err(e) => {
            eprintln!("ferro: scan failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = ferrodrive_core::check(&scan);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }

    for error in &report.errors {
        eprintln!("ferro: {} failed: {}", error.source, error.message);
    }
    if report.updates.is_empty() {
        println!("\nNo updates found. ({:.0?})", started.elapsed());
        return ExitCode::SUCCESS;
    }
    for update in &report.updates {
        let target = match update.kind {
            Kind::Firmware => Some("BIOS / firmware".to_owned()),
            Kind::Driver => update.device.as_deref().map(|id| device_name(&scan, id)),
        };
        println!("\n{}", update.title);
        println!("  source     {}", update.source);
        if let Some(target) = target {
            println!("  for        {target}");
        }
        println!(
            "  installed  {}",
            update.installed.as_deref().unwrap_or("-")
        );
        println!(
            "  offered    {} ({})",
            update.version.as_deref().unwrap_or("-"),
            update.date.as_deref().unwrap_or("-")
        );
        if let Some(url) = &update.url {
            println!("  download   {url}");
        }
    }
    println!(
        "\n{} updates in {:.0?}",
        report.updates.len(),
        started.elapsed()
    );
    ExitCode::SUCCESS
}

fn device_name(scan: &Scan, instance_id: &str) -> String {
    scan.devices
        .iter()
        .find(|d| d.instance_id == instance_id)
        .and_then(|d| d.name.clone())
        .unwrap_or_else(|| instance_id.to_owned())
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

use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use ferrodrive_core::{Device, Scan};

/// Ferrodrive: find driver, BIOS and firmware updates for this machine.
#[derive(Parser)]
#[command(name = "ferro", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan { json, all } => scan(json, all),
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

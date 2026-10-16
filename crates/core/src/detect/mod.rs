//! OS-specific hardware detection behind one [`scan`] entry point.

use crate::model::Scan;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this is not supported on this OS yet")]
    Unsupported,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(windows)]
    #[error("configuration manager error {0:#x}")]
    ConfigManager(u32),
    #[cfg(windows)]
    #[error("Windows API error: {0}")]
    Windows(#[from] ::windows::core::Error),
}

/// Scans this machine's firmware, system identity and devices.
pub fn scan() -> Result<Scan, Error> {
    #[cfg(windows)]
    let scan = windows::scan();
    #[cfg(target_os = "linux")]
    let scan = linux::scan();
    #[cfg(not(any(windows, target_os = "linux")))]
    let scan: Result<Scan, Error> = Err(Error::Unsupported);
    scan.map(pretend)
}

/// Testing aid: `FERRO_PRETEND=manufacturer|product|board|sku|bios` makes the
/// scan report another machine's identity, so vendor catalogs can be exercised
/// on any computer. Empty fields keep the detected value.
fn pretend(mut scan: Scan) -> Scan {
    let Some(value) = std::env::var_os("FERRO_PRETEND") else {
        return scan;
    };
    let value = value.to_string_lossy().into_owned();
    let mut fields = value
        .split('|')
        .map(|f| (!f.is_empty()).then(|| f.to_owned()));
    let mut set = |target: &mut Option<String>| {
        if let Some(Some(v)) = fields.next() {
            *target = Some(v);
        }
    };
    set(&mut scan.system.manufacturer);
    set(&mut scan.system.product);
    set(&mut scan.system.board_product);
    set(&mut scan.system.sku);
    set(&mut scan.firmware.version);
    scan
}

fn debug_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FERRO_DEBUG").is_some())
}

/// Cumulative per-step timings across threads, printed with `FERRO_DEBUG`.
#[cfg_attr(not(windows), allow(dead_code))]
mod profile {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static TOTALS: Mutex<Vec<(&'static str, Duration, u32)>> = Mutex::new(Vec::new());

    pub struct Timer(Option<(&'static str, Instant)>);

    impl Timer {
        pub fn start(name: &'static str) -> Self {
            Self(super::debug_enabled().then(|| (name, Instant::now())))
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            let Some((name, started)) = self.0 else {
                return;
            };
            let elapsed = started.elapsed();
            let mut totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
            match totals.iter_mut().find(|(n, ..)| *n == name) {
                Some((_, total, calls)) => {
                    *total += elapsed;
                    *calls += 1;
                }
                None => totals.push((name, elapsed, 1)),
            }
        }
    }

    /// Prints the totals, slowest first. Times are summed over all threads.
    pub fn report() {
        let mut totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
        totals.sort_by_key(|&(_, total, _)| std::cmp::Reverse(total));
        for (name, total, calls) in totals.drain(..) {
            eprintln!("ferro:   {name:<16} {total:>10.1?} over {calls} calls");
        }
    }
}

/// Runs `f`, printing how long it took to stderr when `FERRO_DEBUG` is set.
pub(crate) fn timed<T>(step: &str, f: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let result = f();
    if debug_enabled() {
        eprintln!("ferro: {step} took {:.1?}", started.elapsed());
    }
    result
}

/// Days since 1970-01-01 to a (year, month, day) civil date (Howard Hinnant's algorithm).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn converts_days_to_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_797), (2024, 3, 15));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}

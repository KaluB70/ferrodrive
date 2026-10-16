//! OS-specific hardware detection behind one [`scan`] entry point.

use crate::model::Scan;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hardware detection is not supported on this OS yet")]
    Unsupported,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(windows)]
    #[error("configuration manager error {0:#x}")]
    ConfigManager(u32),
}

/// Scans this machine's firmware, system identity and devices.
pub fn scan() -> Result<Scan, Error> {
    #[cfg(windows)]
    return windows::scan();
    #[cfg(target_os = "linux")]
    return linux::scan();
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(Error::Unsupported)
}

/// Runs `f`, printing how long it took to stderr when `FERRO_DEBUG` is set.
fn timed<T>(step: &str, f: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let result = f();
    if std::env::var_os("FERRO_DEBUG").is_some() {
        eprintln!("ferro: {step} took {:.1?}", started.elapsed());
    }
    result
}

/// Days since 1970-01-01 to a (year, month, day) civil date (Howard Hinnant's algorithm).
#[cfg_attr(not(windows), allow(dead_code))]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
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

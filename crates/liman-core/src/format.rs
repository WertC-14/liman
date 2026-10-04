//! Human-readable sizes, item counts and relative dates, in the style of Nautilus.

use std::time::SystemTime;

use chrono::{DateTime, Local};

/// Local wall-clock time. Re-exported so that UI crates do not need their own chrono dependency.
pub type Timestamp = DateTime<Local>;

pub fn now() -> Timestamp {
    Local::now()
}

/// Decimal (SI) units like GUI file managers: `84 bytes`, `13.8 kB`, `17.1 MB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["kB", "MB", "GB", "TB", "PB"];
    match bytes {
        1 => "1 byte".into(),
        b if b < 1000 => format!("{b} bytes"),
        _ => {
            let mut value = bytes as f64 / 1000.0;
            let mut unit = 0;
            while value >= 999.95 && unit < UNITS.len() - 1 {
                value /= 1000.0;
                unit += 1;
            }
            format!("{value:.1} {}", UNITS[unit])
        }
    }
}

pub fn items(count: usize) -> String {
    match count {
        1 => "1 item".into(),
        n => format!("{n} items"),
    }
}

/// Relative modification date: `Today 14:13`, `Yesterday 09:02`, `3 days ago`, `Last week`, ...
pub fn modified(time: SystemTime, now: DateTime<Local>) -> String {
    relative(DateTime::<Local>::from(time), now)
}

pub fn relative(time: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (now.date_naive() - time.date_naive()).num_days();
    match days {
        d if d < 0 => time.format("%Y-%m-%d").to_string(),
        0 => format!("Today {}", time.format("%H:%M")),
        1 => format!("Yesterday {}", time.format("%H:%M")),
        2..=6 => format!("{days} days ago"),
        7..=13 => "Last week".into(),
        14..=30 => format!("{} weeks ago", days / 7),
        31..=59 => "Last month".into(),
        60..=364 => format!("{} months ago", days / 30),
        365..=729 => "Last year".into(),
        _ => format!("{} years ago", days / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 bytes");
        assert_eq!(size(1), "1 byte");
        assert_eq!(size(84), "84 bytes");
        assert_eq!(size(13_800), "13.8 kB");
        assert_eq!(size(58_400), "58.4 kB");
        assert_eq!(size(17_100_000), "17.1 MB");
        assert_eq!(size(999_999), "1.0 MB");
        assert_eq!(size(3_200_000_000), "3.2 GB");
    }

    #[test]
    fn item_counts() {
        assert_eq!(items(0), "0 items");
        assert_eq!(items(1), "1 item");
        assert_eq!(items(72), "72 items");
    }

    #[test]
    fn relative_dates() {
        let now = Local.with_ymd_and_hms(2026, 10, 4, 15, 0, 0).unwrap();
        let ago = |d: i64, h: u32| {
            let day = (now - Duration::days(d)).date_naive();
            let time = day.and_hms_opt(h, 7, 0).unwrap();
            relative(time.and_local_timezone(Local).unwrap(), now)
        };
        assert_eq!(ago(0, 14), "Today 14:07");
        assert_eq!(ago(1, 9), "Yesterday 09:07");
        assert_eq!(ago(4, 12), "4 days ago");
        assert_eq!(ago(9, 12), "Last week");
        assert_eq!(ago(21, 12), "3 weeks ago");
        assert_eq!(ago(45, 12), "Last month");
        assert_eq!(ago(120, 12), "4 months ago");
        assert_eq!(ago(400, 12), "Last year");
        assert_eq!(ago(800, 12), "2 years ago");
        assert_eq!(ago(-3, 12), "2026-10-07");
    }
}

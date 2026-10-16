//! Shared text formatting (port of `util/format/units.py` + `time.py`).
//! Units use decimal 1024 steps like Python; timestamps normalize
//! milliseconds the same way.

/// `Units.format_file_size`: B → EB, divide by 1024 while ≥ 1024.
pub fn format_file_size(size: i64) -> String {
    format_file_size_f(size as f64)
}

fn format_file_size_f(mut size: f64) -> String {
    const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut index = 0;
    while size >= 1024.0 && index < UNITS.len() - 1 {
        size /= 1024.0;
        index += 1;
    }
    format!("{size:.2} {}", UNITS[index])
}

/// `Units.format_speed`: empty for a stopped download.
pub fn format_speed(speed: i64) -> String {
    if speed == 0 {
        String::new()
    } else {
        format!("{}/s", format_file_size(speed))
    }
}

/// `Time.format_timestamp` with `%Y-%m-%d %H:%M:%S`; epoch-ms values
/// (abs ≥ 10^10) are normalized to seconds first.
pub fn format_timestamp(timestamp: i64) -> String {
    let seconds = if timestamp.abs() >= 10_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_size_walks_units() {
        assert_eq!(format_file_size(0), "0.00 B");
        assert_eq!(format_file_size(512), "512.00 B");
        assert_eq!(format_file_size(2048), "2.00 KB");
        assert_eq!(format_file_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_file_size(3 * 1024 * 1024 * 1024), "3.00 GB");
        let pb: i64 = 1024i64.pow(5);
        assert_eq!(format_file_size(pb), "1.00 PB");
    }

    #[test]
    fn speed_empty_when_idle() {
        assert_eq!(format_speed(0), "");
        assert_eq!(format_speed(2048), "2.00 KB/s");
    }

    #[test]
    fn timestamp_formats_and_normalizes_ms() {
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13:20");
        // ms value: 1_700_000_000_123 → seconds
        assert_eq!(format_timestamp(1_700_000_000_123), "2023-11-14 22:13:20");
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
    }
}

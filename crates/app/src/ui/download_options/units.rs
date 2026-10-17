//! `util/format/units.py` — the three formatters the media-info card uses.
//! File sizes are 1024-based, bitrates 1000-based; zero values render empty
//! so the description join drops them.

pub fn format_file_size(size: i64) -> String {
    const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut size = size as f64;
    let mut index = 0;
    while size >= 1024.0 && index < UNITS.len() - 1 {
        size /= 1024.0;
        index += 1;
    }
    format!("{size:.2} {}", UNITS[index])
}

pub fn format_bitrate(bitrate: i64) -> String {
    if bitrate == 0 {
        return String::new();
    }
    const UNITS: [&str; 7] = ["bps", "Kbps", "Mbps", "Gbps", "Tbps", "Pbps", "Ebps"];
    let mut bitrate = bitrate as f64;
    let mut index = 0;
    while bitrate >= 1000.0 && index < UNITS.len() - 1 {
        bitrate /= 1000.0;
        index += 1;
    }
    format!("{bitrate:.2} {}", UNITS[index])
}

pub fn format_frame_rate(frame_rate: f64) -> String {
    if frame_rate == 0.0 {
        return String::new();
    }
    format!("{frame_rate:.1} fps")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_size_uses_1024_base() {
        assert_eq!(format_file_size(0), "0.00 B");
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1536), "1.50 KB");
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn bitrate_uses_1000_base_and_zero_is_empty() {
        assert_eq!(format_bitrate(0), "");
        assert_eq!(format_bitrate(1000), "1.00 Kbps");
        assert_eq!(format_bitrate(1_500_000), "1.50 Mbps");
    }

    #[test]
    fn frame_rate_one_decimal_and_zero_is_empty() {
        assert_eq!(format_frame_rate(0.0), "");
        assert_eq!(format_frame_rate(29.97), "30.0 fps");
        assert_eq!(format_frame_rate(59.94), "59.9 fps");
    }
}

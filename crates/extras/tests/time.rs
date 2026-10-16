use extras::time::{format_ass_time_by_ms, format_ass_time_by_seconds, format_srt_time};

#[test]
fn srt_time() {
    assert_eq!(format_srt_time(0.0), "00:00:00,000");
    assert_eq!(format_srt_time(3661.5), "01:01:01,500");
    assert_eq!(format_srt_time(59.9999), "00:01:00,000");
}

#[test]
fn ass_time_by_ms() {
    assert_eq!(format_ass_time_by_ms(0), "0:00:00.00");
    assert_eq!(format_ass_time_by_ms(3661999), "1:01:01.99");
}

#[test]
fn ass_time_by_seconds() {
    assert_eq!(format_ass_time_by_seconds(0.0), "0:00:00.00");
    assert_eq!(format_ass_time_by_seconds(59.999), "0:01:00.00");
    assert_eq!(format_ass_time_by_seconds(3661.5), "1:01:01.50");
}

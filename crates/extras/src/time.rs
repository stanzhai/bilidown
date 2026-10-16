//! Time formatting ports of `src/util/format/time.py` (ticket 18).

/// `Time.format_srt_time` — `H:MM:SS,mmm` with full carry on ms==1000.
pub fn format_srt_time(seconds: f64) -> String {
    let mut hour = (seconds / 3600.0) as i64;
    let mut minute = ((seconds % 3600.0) / 60.0) as i64;
    let mut second = (seconds % 60.0) as i64;
    let mut milli = ((seconds - seconds.floor()) * 1000.0).round() as i64;
    if milli == 1000 {
        second += 1;
        milli = 0;
    }
    if second >= 60 {
        minute += second / 60;
        second %= 60;
    }
    if minute >= 60 {
        hour += minute / 60;
        minute %= 60;
    }
    format!("{hour:02}:{minute:02}:{second:02},{milli:03}")
}

/// `Time.format_ass_time_by_ms` — `H:MM:SS.cc`, centiseconds clamped to 99.
pub fn format_ass_time_by_ms(ms: i64) -> String {
    let hour = ms / 3600000;
    let minute = (ms % 3600000) / 60000;
    let second = (ms % 60000) / 1000;
    let mut centis = ((ms % 1000) as f64 / 10.0).round() as i64;
    if centis == 100 {
        centis = 99;
    }
    format!("{hour}:{minute:02}:{second:02}.{centis:02}")
}

/// `Time.format_ass_time_by_seconds` — `H:MM:SS.cc` with carry on cs==100.
pub fn format_ass_time_by_seconds(seconds: f64) -> String {
    let mut hour = (seconds / 3600.0) as i64;
    let mut minute = ((seconds % 3600.0) / 60.0) as i64;
    let mut second = (seconds % 60.0) as i64;
    let mut centis = ((seconds - seconds.floor()) * 100.0).round() as i64;
    if centis == 100 {
        second += 1;
        centis = 0;
    }
    if second >= 60 {
        minute += second / 60;
        second %= 60;
    }
    if minute >= 60 {
        hour += minute / 60;
        minute %= 60;
    }
    format!("{hour}:{minute:02}:{second:02}.{centis:02}")
}

//! Log viewer tests. Vectors copy the Python LOG_PATTERN and proxy-model
//! filter semantics from `src/gui/dialog/log.py` / `log_list/proxy_model.py`.

use services::log::{LevelFilter, LogFilter, LogRecord};

const SAMPLE: &str = "\
[2026-01-02 03:04:05.123456] - bili23.parse - INFO - at parse.py:120 in parse: start parsing https://www.bilibili.com/video/BV1xx411c7mD
Traceback (most recent call last):
  File \"x.py\", line 1
[2026-01-02 03:04:06.000001] - root - ERROR - at main.py:88 in run: boom";

#[test]
fn parses_records_with_callsite() {
    let recs = LogRecord::parse_all(SAMPLE);
    assert_eq!(recs.len(), 2);

    let r = &recs[0];
    assert_eq!(r.timestamp, "2026-01-02 03:04:05.123456");
    assert_eq!(r.name, "bili23.parse");
    assert_eq!(r.level, "INFO");
    assert_eq!(r.callsite, "parse.py:120 in parse");
    assert!(r.message.starts_with("start parsing https://"));

    // continuation lines append to previous message with \n
    assert!(recs[0].message.contains('\n'));
    assert!(recs[0].message.contains("Traceback"));
    assert!(recs[0].message.contains("File \"x.py\", line 1"));

    assert_eq!(recs[1].level, "ERROR");
    assert_eq!(recs[1].name, "root");
}

#[test]
fn continuation_only_lines_are_dropped() {
    // garbage with no preceding record disappears
    let recs = LogRecord::parse_all("not a record\n[2026-01-01 00:00:00.000000] - a - INFO - at f.py:1 in g: m\n  indented continuation");
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].message, "m\n  indented continuation");
}

#[test]
fn level_filter_is_exact() {
    let recs = LogRecord::parse_all(SAMPLE);
    let f = LogFilter {
        level: LevelFilter::Error,
        ..Default::default()
    };
    assert_eq!(f.apply(&recs).len(), 1);
    assert_eq!(f.apply(&recs)[0].level, "ERROR");

    let all = LogFilter {
        level: LevelFilter::All,
        ..Default::default()
    };
    assert_eq!(all.apply(&recs).len(), 2);

    let info = LogFilter {
        level: LevelFilter::Info,
        ..Default::default()
    };
    assert_eq!(info.apply(&recs).len(), 1); // INFO only, not WARNING/ERROR
}

#[test]
fn text_filter_matches_joined_fields_casefolded() {
    let recs = LogRecord::parse_all(SAMPLE);
    // matches timestamp
    let f = |t: &str| LogFilter {
        level: LevelFilter::All,
        text: t.to_owned(),
    };
    assert_eq!(f("03:04:06").apply(&recs).len(), 1);
    // matches name
    assert_eq!(f("BILI23.PARSE").apply(&recs).len(), 1); // casefold both ways
                                                         // matches message content incl. continuation lines
    assert_eq!(f("traceback").apply(&recs).len(), 1);
    // does NOT match the callsite (proxy model joins ts/name/level/msg only)
    assert_eq!(f("parse.py:120").apply(&recs).len(), 0);
    // empty text = no filtering
    assert_eq!(f("").apply(&recs).len(), 2);
    assert_eq!(f("no such substring").apply(&recs).len(), 0);
}

#[test]
fn parse_file_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("app.log");
    std::fs::write(&p, SAMPLE).unwrap();
    let recs = LogRecord::parse_file(&p).unwrap();
    assert_eq!(recs.len(), 2);
    // missing file → io error (Python returns []; caller checks exists)
    assert!(LogRecord::parse_file(&dir.path().join("nope.log")).is_err());
}

//! Timezone-aware time helpers.
//!
//! `Config.timezone` is an IANA name (e.g. "Asia/Shanghai"). Empty means "use
//! the system local timezone" (chrono::Local). All timestamps stored as
//! RFC3339; template date variables (yyyy / yyyy-mm / yyyy-mm-dd) are computed
//! in the configured tz so the on-disk folder tree matches the user's wall
//! clock, not UTC.

use chrono::{DateTime, Local, Utc};
use chrono_tz::Tz as IanaTz;
use std::path::Path;

/// Parse an IANA timezone string. Empty/whitespace → None (system local).
/// Invalid → None (caller should treat as "fall back to system").
pub fn parse_tz(s: &str) -> Option<IanaTz> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<IanaTz>().ok()
}

/// Current time as RFC3339 in the configured tz (None = system local).
pub fn now_rfc3339(tz: Option<IanaTz>) -> String {
    let utc = Utc::now();
    match tz {
        Some(t) => utc.with_timezone(&t).to_rfc3339(),
        None => utc.with_timezone(&Local).to_rfc3339(),
    }
}

/// Current (year "2026", month "01".."12") in the configured tz.
pub fn now_ym(tz: Option<IanaTz>) -> (String, String) {
    let utc = Utc::now();
    match tz {
        Some(t) => {
            let dt = utc.with_timezone(&t);
            (dt.format("%Y").to_string(), dt.format("%m").to_string())
        }
        None => {
            let dt = utc.with_timezone(&Local);
            (dt.format("%Y").to_string(), dt.format("%m").to_string())
        }
    }
}

/// Format an RFC3339 timestamp as "YYYY-MM-DD HH:MM" in the configured tz.
pub fn fmt_in_tz(rfc: &str, tz: Option<IanaTz>) -> String {
    match DateTime::parse_from_rfc3339(rfc) {
        Ok(dt) => match tz {
            Some(t) => dt.with_timezone(&t).format("%Y-%m-%d %H:%M").to_string(),
            None => dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
        },
        Err(_) => rfc.to_string(),
    }
}

/// Date template variables for the current moment in the configured tz.
/// Returns (placeholder, value) pairs: yyyy, yyyy-mm, yyyy-mm-dd, plus
/// time-only hh and hhmm for finer-grained filename disambiguation.
pub fn date_vars(tz: Option<IanaTz>) -> Vec<(&'static str, String)> {
    let utc = Utc::now();
    // Format in each branch — DateTime<Tz> and DateTime<Local> can't unify.
    let (y, ym, ymd, h, hm) = match tz {
        Some(t) => {
            let dt = utc.with_timezone(&t);
            (
                dt.format("%Y").to_string(),
                dt.format("%Y-%m").to_string(),
                dt.format("%Y-%m-%d").to_string(),
                dt.format("%H").to_string(),
                dt.format("%H%M").to_string(),
            )
        }
        None => {
            let dt = utc.with_timezone(&Local);
            (
                dt.format("%Y").to_string(),
                dt.format("%Y-%m").to_string(),
                dt.format("%Y-%m-%d").to_string(),
                dt.format("%H").to_string(),
                dt.format("%H%M").to_string(),
            )
        }
    };
    vec![
        ("${yyyy}", y),
        ("${yyyy-mm}", ym),
        ("${yyyy-mm-dd}", ymd),
        ("${hh}", h),
        ("${hhmm}", hm),
    ]
}

/// Days since the most recent of (last_opened, last_reviewed, filed). Used by
/// the 回顾 view's memory-curve staleness ranking.
pub fn staleness_days(opened: &str, reviewed: &str, filed: &str) -> i64 {
    let now = Utc::now();
    let mut best: Option<DateTime<Utc>> = None;
    for s in [opened, reviewed, filed] {
        let s = s.trim();
        if s.is_empty() { continue; }
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            let u = dt.with_timezone(&Utc);
            best = Some(match best {
                None => u,
                Some(b) => if u > b { u } else { b },
            });
        }
    }
    match best {
        Some(dt) => (now - dt).num_days().max(0),
        None => 0,
    }
}

/// An RFC3339 timestamp `days_before` before now (for the stale-count cutoff).
pub fn cutoff_rfc(days_before: i64) -> String {
    let now = Utc::now();
    let dt = now - chrono::Duration::days(days_before);
    dt.to_rfc3339()
}

/// A file's mtime as RFC3339 (UTC), or None if it can't be stat'd.
pub fn mtime_rfc(path: &Path) -> Option<String> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md.modified().ok()?;
    let dt: DateTime<Utc> = mtime.into();
    Some(dt.to_rfc3339())
}

/// True if `current_rfc` differs from `baseline_rfc` by more than 1 second
/// (i.e. the file was modified after the baseline was recorded).
pub fn mtime_changed(current_rfc: &str, baseline_rfc: &str) -> bool {
    let a = DateTime::parse_from_rfc3339(current_rfc).ok().map(|d| d.with_timezone(&Utc));
    let b = DateTime::parse_from_rfc3339(baseline_rfc).ok().map(|d| d.with_timezone(&Utc));
    match (a, b) {
        (Some(a), Some(b)) => (a - b).num_seconds().abs() > 1,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_is_none() {
        assert!(parse_tz("").is_none());
        assert!(parse_tz("  ").is_none());
    }

    #[test]
    fn parse_valid_iana() {
        assert_eq!(parse_tz("Asia/Shanghai"), Some(IanaTz::Asia__Shanghai));
        assert_eq!(parse_tz("America/New_York"), Some(IanaTz::America__New_York));
        assert_eq!(parse_tz("UTC"), Some(IanaTz::UTC));
    }

    #[test]
    fn parse_invalid_falls_back_to_none() {
        assert!(parse_tz("Mars/Olympus").is_none());
    }

    #[test]
    fn now_rfc3339_is_parseable() {
        let s = now_rfc3339(parse_tz("UTC"));
        assert!(DateTime::parse_from_rfc3339(&s).is_ok());
    }

    #[test]
    fn date_vars_have_expected_keys() {
        let vars = date_vars(parse_tz("UTC"));
        let keys: Vec<_> = vars.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"${yyyy}"));
        assert!(keys.contains(&"${yyyy-mm}"));
        assert!(keys.contains(&"${yyyy-mm-dd}"));
        // values are the right width
        let y = vars.iter().find(|(k, _)| *k == "${yyyy}").unwrap().1.clone();
        assert_eq!(y.len(), 4);
        let ym = vars.iter().find(|(k, _)| *k == "${yyyy-mm}").unwrap().1.clone();
        assert_eq!(ym.len(), 7);
    }

    #[test]
    fn fmt_in_tz_handles_bad_input() {
        assert_eq!(fmt_in_tz("not-a-date", None), "not-a-date");
        assert_eq!(fmt_in_tz("not-a-date", parse_tz("UTC")), "not-a-date");
    }

    #[test]
    fn fmt_in_tz_formats_valid() {
        let s = fmt_in_tz("2026-07-06T15:30:00+00:00", parse_tz("UTC"));
        assert_eq!(s, "2026-07-06 15:30");
        // Asia/Shanghai is +08:00
        let s = fmt_in_tz("2026-07-06T15:30:00+00:00", parse_tz("Asia/Shanghai"));
        assert_eq!(s, "2026-07-06 23:30");
    }

    #[test]
    fn staleness_picks_most_recent_touch() {
        // opened > reviewed > filed → staleness counts from opened.
        let d = staleness_days(
            "2020-01-01T00:00:00+00:00", // opened long ago
            "2019-01-01T00:00:00+00:00", // reviewed older
            "2018-01-01T00:00:00+00:00", // filed oldest
        );
        assert!(d > 2000); // 2020-01-01 is > 5 years ago
        // empty opened/reviewed → falls back to filed.
        let d2 = staleness_days("", "", "2020-01-01T00:00:00+00:00");
        assert_eq!(d, d2);
    }

    #[test]
    fn staleness_all_empty_is_zero() {
        assert_eq!(staleness_days("", "", ""), 0);
    }

    #[test]
    fn mtime_changed_detects_diff() {
        assert!(!mtime_changed("2026-07-06T10:00:00+00:00", "2026-07-06T10:00:00+00:00"));
        assert!(mtime_changed("2026-07-06T10:00:05+00:00", "2026-07-06T10:00:00+00:00"));
        // unparseable → not flagged as changed
        assert!(!mtime_changed("garbage", "2026-07-06T10:00:00+00:00"));
    }

    #[test]
    fn cutoff_rfc_is_in_the_past() {
        let c = cutoff_rfc(180);
        let parsed = DateTime::parse_from_rfc3339(&c).unwrap().with_timezone(&Utc);
        let now = Utc::now();
        assert!(parsed < now);
        assert!((now - parsed).num_days() >= 179);
    }
}

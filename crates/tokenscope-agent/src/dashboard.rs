//! Read-only dashboard projection. The Agent owns date/timezone semantics and
//! opens SQLite in read-only mode; the desktop never queries the database.
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::{path::Path, str::FromStr, time::{SystemTime, UNIX_EPOCH}};
use tokenscope_local_store::{DashboardSnapshot, LocalStore};

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    Ok(i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?)
}

fn local_midnight(date: NaiveDate, zone: Tz) -> Result<i64, Box<dyn std::error::Error>> {
    let midnight = date.and_hms_opt(0, 0, 0).ok_or("invalid local midnight")?;
    match zone.from_local_datetime(&midnight) {
        chrono::LocalResult::Single(value) => Ok(value.timestamp_millis()),
        chrono::LocalResult::Ambiguous(a, b) => Ok(a.min(b).timestamp_millis()),
        chrono::LocalResult::None => {
            // Some historical timezone transitions skipped local midnight.
            // Move forward to the first valid minute rather than assume 24h.
            for minute in 1..=180 {
                let candidate = midnight + Duration::minutes(minute);
                match zone.from_local_datetime(&candidate) {
                    chrono::LocalResult::Single(value) => return Ok(value.timestamp_millis()),
                    chrono::LocalResult::Ambiguous(a, b) => return Ok(a.min(b).timestamp_millis()),
                    chrono::LocalResult::None => {}
                }
            }
            Err("could not resolve local day boundary".into())
        }
    }
}

pub fn read_snapshot(path: &Path, range: &str) -> Result<DashboardSnapshot, Box<dyn std::error::Error>> {
    if !["today", "7d", "30d", "all"].contains(&range) {
        return Err("range must be today, 7d, 30d or all".into());
    }
    let store = LocalStore::open_readonly(path)?;
    let identity = store.identity()?;
    let requested_zone = identity.as_ref().map(|v| v.time_zone.as_str()).unwrap_or("UTC");
    let parsed_zone = Tz::from_str(requested_zone);
    let zone_valid = parsed_zone.is_ok();
    let zone = parsed_zone.unwrap_or(chrono_tz::UTC);
    let now = now_ms()?;
    let now_utc = DateTime::<Utc>::from_timestamp_millis(now).ok_or("invalid system clock")?;
    let today = now_utc.with_timezone(&zone).date_naive();

    let days_back = match range {
        "today" => 0,
        "7d" => 6,
        "30d" | "all" => 29,
        _ => unreachable!(),
    };
    let range_start_date = today - Duration::days(days_back);
    let from_ms = if range == "all" { None } else { Some(local_midnight(range_start_date, zone)?) };
    let to_ms = if range == "all" { None } else { Some(now.saturating_add(1)) };

    // The trend follows the selected range, except `all` which remains bounded
    // to 30 days. Each boundary is resolved independently so DST transitions
    // do not shift a day.
    let chart_days_back = match range {
        "today" => 0,
        "7d" => 6,
        _ => 29,
    };
    let chart_start = today - Duration::days(chart_days_back);
    let mut daily_ranges = Vec::with_capacity(30);
    for offset in 0..30 {
        let date = chart_start + Duration::days(offset);
        if date > today { break; }
        let start = local_midnight(date, zone)?;
        let next = local_midnight(date + Duration::days(1), zone)?;
        let end = next.min(now.saturating_add(1));
        if end > start {
            daily_ranges.push((date.format("%Y-%m-%d").to_string(), start, end));
        }
    }

    let zone_name = if zone_valid { requested_zone.to_string() } else { "UTC".into() };
    Ok(store.dashboard_snapshot(
        range, from_ms, to_ms, &daily_ranges, now, zone_name, zone_valid,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_is_strictly_allowlisted() {
        assert!(read_snapshot(Path::new("/path/does/not/exist"), "../../etc/passwd").is_err());
    }

    #[test]
    fn dst_day_boundaries_are_not_forced_to_24_hours() {
        let zone = chrono_tz::America::Los_Angeles;
        let spring = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let a = local_midnight(spring, zone).unwrap();
        let b = local_midnight(spring + Duration::days(1), zone).unwrap();
        assert_eq!(b - a, 23 * 60 * 60 * 1000);
        let autumn = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let c = local_midnight(autumn, zone).unwrap();
        let d = local_midnight(autumn + Duration::days(1), zone).unwrap();
        assert_eq!(d - c, 25 * 60 * 60 * 1000);
    }
}

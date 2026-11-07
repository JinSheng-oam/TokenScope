//! Read-only dashboard projection. The Agent owns date/timezone semantics and
//! opens SQLite in read-only mode; the desktop never queries the database.
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::{
    path::Path,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_local_store::{DashboardQuery, DashboardSnapshot, LocalStore};

struct ResolvedRange {
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    chart_start: NaiveDate,
    chart_end: NaiveDate,
}

pub struct SessionAnalysisOptions<'a> {
    pub range: &'a str,
    pub session_offset: u64,
    pub session_limit: u32,
    pub search: Option<&'a str>,
    pub tool_id: Option<&'a str>,
    pub model_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub start_date: Option<&'a str>,
    pub end_date: Option<&'a str>,
    pub include_filter_options: bool,
}

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
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

fn resolve_range(
    range: &str,
    start_date: Option<&str>,
    end_date: Option<&str>,
    zone: Tz,
    now: i64,
    earliest_event_ms: Option<i64>,
) -> Result<ResolvedRange, Box<dyn std::error::Error>> {
    let now_utc = DateTime::<Utc>::from_timestamp_millis(now).ok_or("invalid system clock")?;
    let today = now_utc.with_timezone(&zone).date_naive();
    if range == "custom" {
        let start = NaiveDate::parse_from_str(
            start_date.ok_or("custom range requires --start-date")?,
            "%Y-%m-%d",
        )?;
        let end = NaiveDate::parse_from_str(
            end_date.ok_or("custom range requires --end-date")?,
            "%Y-%m-%d",
        )?;
        if end < start || (end - start).num_days() > 92 {
            return Err("custom range must be 1..93 days".into());
        }
        let from = local_midnight(start, zone)?;
        let to = local_midnight(end + Duration::days(1), zone)?.min(now.saturating_add(1));
        if to <= from {
            return Err("custom range is empty".into());
        }
        Ok(ResolvedRange {
            from_ms: Some(from),
            to_ms: Some(to),
            chart_start: start,
            chart_end: end.min(today),
        })
    } else {
        let days_back = match range {
            "today" => 0,
            "7d" => 6,
            "30d" | "all" => 29,
            _ => return Err("range must be today, 7d, 30d, all or custom".into()),
        };
        let start = if range == "all" {
            earliest_event_ms
                .and_then(DateTime::<Utc>::from_timestamp_millis)
                .map(|event| event.with_timezone(&zone).date_naive())
                .unwrap_or(today - Duration::days(days_back))
        } else {
            today - Duration::days(days_back)
        };
        let from = if range == "all" {
            None
        } else {
            Some(local_midnight(start, zone)?)
        };
        let to = if range == "all" {
            None
        } else {
            Some(now.saturating_add(1))
        };
        Ok(ResolvedRange {
            from_ms: from,
            to_ms: to,
            chart_start: start,
            chart_end: today,
        })
    }
}

pub fn read_snapshot(
    path: &Path,
    range: &str,
    session_offset: u64,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<DashboardSnapshot, Box<dyn std::error::Error>> {
    if !["today", "7d", "30d", "all", "custom"].contains(&range) {
        return Err("range must be today, 7d, 30d, all or custom".into());
    }
    let store = LocalStore::open_readonly(path)?;
    let identity = store.identity()?;
    let requested_zone = identity
        .as_ref()
        .map(|v| v.time_zone.as_str())
        .unwrap_or("UTC");
    let parsed_zone = Tz::from_str(requested_zone);
    let zone_valid = parsed_zone.is_ok();
    let zone = parsed_zone.unwrap_or(chrono_tz::UTC);
    let now = now_ms()?;
    let earliest_event_ms = match (range, identity.as_ref()) {
        ("all", Some(identity)) => store.earliest_dashboard_event_ms(&identity.device_id)?,
        _ => None,
    };
    let resolved = resolve_range(range, start_date, end_date, zone, now, earliest_event_ms)?;

    // Each boundary is resolved independently so DST transitions do not shift
    // a day. `all` starts at the earliest timestamp still retained locally.
    let day_count = (resolved.chart_end - resolved.chart_start).num_days() + 1;
    let mut daily_ranges = Vec::with_capacity(usize::try_from(day_count.max(1))?);
    let mut date = resolved.chart_start;
    while date <= resolved.chart_end {
        let start = local_midnight(date, zone)?;
        let next = local_midnight(date + Duration::days(1), zone)?;
        let end = next.min(now.saturating_add(1));
        if end > start {
            daily_ranges.push((date.format("%Y-%m-%d").to_string(), start, end));
        }
        if daily_ranges.len() > 3_660 {
            return Err("chart range too long".into());
        }
        date += Duration::days(1);
    }

    let zone_name = if zone_valid {
        requested_zone.to_string()
    } else {
        "UTC".into()
    };
    Ok(store.dashboard_snapshot(DashboardQuery {
        range,
        from_ms: resolved.from_ms,
        to_ms: resolved.to_ms,
        daily_ranges: &daily_ranges,
        generated_at_ms: now,
        time_zone: zone_name,
        time_zone_valid: zone_valid,
        session_offset,
    })?)
}

pub fn read_session_analysis(
    path: &Path,
    options: SessionAnalysisOptions<'_>,
) -> Result<tokenscope_local_store::SessionAnalysisPage, Box<dyn std::error::Error>> {
    if !["today", "7d", "30d", "all", "custom"].contains(&options.range) {
        return Err("range must be today, 7d, 30d, all or custom".into());
    }
    let store = LocalStore::open_readonly(path)?;
    let identity = store
        .identity()?
        .ok_or("local identity is not initialized")?;
    let parsed_zone = Tz::from_str(&identity.time_zone);
    let zone = parsed_zone.unwrap_or(chrono_tz::UTC);
    let now = now_ms()?;
    let resolved = resolve_range(
        options.range,
        options.start_date,
        options.end_date,
        zone,
        now,
        None,
    )?;
    Ok(store.session_analysis_page_with_filter_options(
        &identity.device_id,
        resolved.from_ms,
        resolved.to_ms,
        options.session_offset,
        options.session_limit,
        options.search,
        options.tool_id,
        options.model_id,
        options.project_id,
        options.include_filter_options,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_is_strictly_allowlisted() {
        assert!(read_snapshot(
            Path::new("/path/does/not/exist"),
            "../../etc/passwd",
            0,
            None,
            None
        )
        .is_err());
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

    #[test]
    fn all_range_starts_at_earliest_retained_event() {
        let zone = chrono_tz::Asia::Shanghai;
        let now = Utc
            .with_ymd_and_hms(2026, 11, 4, 4, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let earliest = Utc
            .with_ymd_and_hms(2026, 3, 30, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();

        let resolved = resolve_range("all", None, None, zone, now, Some(earliest)).unwrap();

        assert_eq!(
            resolved.chart_start,
            NaiveDate::from_ymd_opt(2026, 3, 30).unwrap()
        );
        assert_eq!(
            resolved.chart_end,
            NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()
        );
        assert_eq!(resolved.from_ms, None);
        assert_eq!(resolved.to_ms, None);
    }
}

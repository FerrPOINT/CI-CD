use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

const SEARCH_WINDOW_DAYS: i64 = 366 * 5;

#[derive(Debug, Clone)]
pub(crate) struct CronSchedule {
    minutes: CronField,
    hours: CronField,
    days_of_month: CronField,
    months: CronField,
    days_of_week: CronField,
}

#[derive(Debug, Clone)]
struct CronField {
    values: Vec<bool>,
    any: bool,
}

#[derive(Debug, Clone, Copy)]
enum CronFieldKind {
    Minute,
    Hour,
    DayOfMonth,
    Month,
    DayOfWeek,
}

pub(crate) fn parse_cron(expression: &str) -> Result<CronSchedule, String> {
    let parts: Vec<&str> = expression.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron must have five fields".to_owned());
    }
    Ok(CronSchedule {
        minutes: parse_field(parts[0], CronFieldKind::Minute)?,
        hours: parse_field(parts[1], CronFieldKind::Hour)?,
        days_of_month: parse_field(parts[2], CronFieldKind::DayOfMonth)?,
        months: parse_field(parts[3], CronFieldKind::Month)?,
        days_of_week: parse_field(parts[4], CronFieldKind::DayOfWeek)?,
    })
}

pub(crate) fn next_fire_after(
    schedule: &CronSchedule,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let mut candidate = after + Duration::minutes(1);
    candidate = candidate
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .expect("zero second and nanosecond are valid");
    let deadline = after + Duration::days(SEARCH_WINDOW_DAYS);
    while candidate <= deadline {
        if schedule.matches(candidate) {
            return Some(candidate);
        }
        candidate += Duration::minutes(1);
    }
    None
}

pub(crate) fn next_fire_after_expr(
    expression: &str,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    let schedule = parse_cron(expression)?;
    next_fire_after(&schedule, after).ok_or_else(|| {
        "cron expression has no matching fire time in the next five years".to_owned()
    })
}

impl CronSchedule {
    fn matches(&self, value: DateTime<Utc>) -> bool {
        let day_of_month = value.day();
        let day_of_week = value.weekday().num_days_from_sunday();
        let day_matches = if !self.days_of_month.any && !self.days_of_week.any {
            self.days_of_month.matches(day_of_month) || self.days_of_week.matches(day_of_week)
        } else {
            self.days_of_month.matches(day_of_month) && self.days_of_week.matches(day_of_week)
        };
        self.minutes.matches(value.minute())
            && self.hours.matches(value.hour())
            && day_matches
            && self.months.matches(value.month())
    }
}

impl CronField {
    fn matches(&self, value: u32) -> bool {
        self.values.get(value as usize).copied().unwrap_or(false)
    }
}

fn parse_field(raw: &str, kind: CronFieldKind) -> Result<CronField, String> {
    if raw.is_empty() {
        return Err("empty cron field".to_owned());
    }
    let (_, normalized_max) = normalized_bounds(kind);
    let mut values = vec![false; normalized_max as usize + 1];
    for part in raw.split(',') {
        if part.is_empty() {
            return Err("empty cron list item".to_owned());
        }
        let (base, step) = parse_step(part)?;
        let ranges = expand_base(base, step.is_some(), kind)?;
        for (start, end) in ranges {
            if start > end {
                return Err("cron range start must be before range end".to_owned());
            }
            let mut value = start;
            while value <= end {
                let normalized = normalize_value(kind, value)?;
                values[normalized as usize] = true;
                value = match value.checked_add(step.unwrap_or(1)) {
                    Some(next) => next,
                    None => break,
                };
            }
        }
    }
    let (normalized_min, normalized_max) = normalized_bounds(kind);
    let any = (normalized_min..=normalized_max).all(|value| values[value as usize]);
    if !values.iter().any(|allowed| *allowed) {
        return Err("cron field has no allowed values".to_owned());
    }
    Ok(CronField { values, any })
}

fn parse_step(part: &str) -> Result<(&str, Option<u32>), String> {
    let Some((base, step)) = part.split_once('/') else {
        return Ok((part, None));
    };
    if base.is_empty() || step.is_empty() || step.contains('/') {
        return Err("invalid cron step".to_owned());
    }
    let step = step
        .parse::<u32>()
        .map_err(|_| "cron step must be a positive number".to_owned())?;
    if step == 0 {
        return Err("cron step must be positive".to_owned());
    }
    Ok((base, Some(step)))
}

fn expand_base(base: &str, has_step: bool, kind: CronFieldKind) -> Result<Vec<(u32, u32)>, String> {
    if base == "*" {
        let (min, max) = expansion_bounds(kind);
        return Ok(vec![(min, max)]);
    }
    if let Some((start, end)) = base.split_once('-') {
        if start.is_empty() || end.is_empty() || end.contains('-') {
            return Err("invalid cron range".to_owned());
        }
        return Ok(vec![(
            parse_raw_value(start, kind)?,
            parse_raw_value(end, kind)?,
        )]);
    }
    let start = parse_raw_value(base, kind)?;
    let end = if has_step {
        expansion_bounds(kind).1
    } else {
        start
    };
    Ok(vec![(start, end)])
}

fn parse_raw_value(raw: &str, kind: CronFieldKind) -> Result<u32, String> {
    let upper = raw.to_ascii_uppercase();
    if let Some(value) = named_value(&upper, kind) {
        return Ok(value);
    }
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("invalid cron value `{raw}`"))?;
    let (min, max) = raw_bounds(kind);
    if value < min || value > max {
        return Err(format!("cron value `{raw}` is outside {min}..{max}"));
    }
    Ok(value)
}

fn named_value(raw: &str, kind: CronFieldKind) -> Option<u32> {
    match kind {
        CronFieldKind::Month => match raw {
            "JAN" => Some(1),
            "FEB" => Some(2),
            "MAR" => Some(3),
            "APR" => Some(4),
            "MAY" => Some(5),
            "JUN" => Some(6),
            "JUL" => Some(7),
            "AUG" => Some(8),
            "SEP" => Some(9),
            "OCT" => Some(10),
            "NOV" => Some(11),
            "DEC" => Some(12),
            _ => None,
        },
        CronFieldKind::DayOfWeek => match raw {
            "SUN" => Some(0),
            "MON" => Some(1),
            "TUE" => Some(2),
            "WED" => Some(3),
            "THU" => Some(4),
            "FRI" => Some(5),
            "SAT" => Some(6),
            _ => None,
        },
        _ => None,
    }
}

fn normalize_value(kind: CronFieldKind, value: u32) -> Result<u32, String> {
    let normalized = match kind {
        CronFieldKind::DayOfWeek if value == 7 => 0,
        _ => value,
    };
    let (min, max) = normalized_bounds(kind);
    if normalized < min || normalized > max {
        return Err(format!("cron value `{value}` is outside {min}..{max}"));
    }
    Ok(normalized)
}

fn raw_bounds(kind: CronFieldKind) -> (u32, u32) {
    match kind {
        CronFieldKind::Minute => (0, 59),
        CronFieldKind::Hour => (0, 23),
        CronFieldKind::DayOfMonth => (1, 31),
        CronFieldKind::Month => (1, 12),
        CronFieldKind::DayOfWeek => (0, 7),
    }
}

fn normalized_bounds(kind: CronFieldKind) -> (u32, u32) {
    match kind {
        CronFieldKind::DayOfWeek => (0, 6),
        _ => raw_bounds(kind),
    }
}

fn expansion_bounds(kind: CronFieldKind) -> (u32, u32) {
    match kind {
        CronFieldKind::DayOfWeek => (0, 6),
        _ => raw_bounds(kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_ranges_steps_lists_and_names() {
        assert!(parse_cron("*/15 8-18 * JAN,MAR MON-FRI").is_ok());
        assert!(parse_cron("0 4 1,15 * 0,7").is_ok());
    }

    #[test]
    fn rejects_invalid_cron_values() {
        assert!(parse_cron("60 * * * *").is_err());
        assert!(parse_cron("* 24 * * *").is_err());
        assert!(parse_cron("* * 0 * *").is_err());
        assert!(parse_cron("* * * 13 *").is_err());
        assert!(parse_cron("* * * * FRI-MON").is_err());
        assert!(parse_cron("* * * * */0").is_err());
    }

    #[test]
    fn next_fire_honors_minute_hour_and_weekday() {
        let schedule = parse_cron("0 4 * * MON").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 8, 31, 3, 58, 30).unwrap();
        assert_eq!(
            next_fire_after(&schedule, after).unwrap(),
            Utc.with_ymd_and_hms(2026, 8, 31, 4, 0, 0).unwrap()
        );
    }

    #[test]
    fn day_of_month_and_day_of_week_use_cron_or_semantics() {
        let schedule = parse_cron("0 0 13 * MON").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
        assert_eq!(
            next_fire_after(&schedule, after).unwrap(),
            Utc.with_ymd_and_hms(2026, 8, 13, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn impossible_calendar_schedule_has_no_next_fire() {
        let schedule = parse_cron("0 0 31 FEB *").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert!(next_fire_after(&schedule, after).is_none());
    }
}

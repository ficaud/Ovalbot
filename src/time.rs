use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub fn now() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap()
}

pub fn in_days(days: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::days(days))
        .format(&Rfc3339)
        .unwrap()
}

pub fn is_past(rfc3339: &str) -> bool {
    match OffsetDateTime::parse(rfc3339, &Rfc3339) {
        Ok(t) => OffsetDateTime::now_utc() > t,
        Err(_) => true,
    }
}

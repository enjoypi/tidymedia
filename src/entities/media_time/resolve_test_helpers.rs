use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;

use crate::entities::media_time::candidate::Candidate;
use crate::entities::media_time::priority::Source;

pub(super) fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 17, 0, 0, 0).unwrap()
}

pub(super) fn cand(source: Source, secs: i64) -> Candidate {
    Candidate {
        utc: Utc.timestamp_opt(secs, 0).single().unwrap(),
        offset: None,
        source,
        inferred_offset: false,
    }
}

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;

use super::candidate::Candidate;
use super::priority::Source;

#[must_use]
pub fn from_modified(modified: Option<SystemTime>) -> Option<Candidate> {
    let t = modified?;
    let secs_u64 = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    convert_secs_to_candidate(secs_u64)
}

fn convert_secs_to_candidate(secs_u64: u64) -> Option<Candidate> {
    let signed = i64::try_from(secs_u64).ok()?;
    let delta = TimeDelta::try_seconds(signed)?;
    let utc = DateTime::<Utc>::UNIX_EPOCH.checked_add_signed(delta)?;
    Some(Candidate {
        utc,
        offset: None,
        source: Source::FsMtime,
        inferred_offset: false,
    })
}

#[cfg(test)]
#[path = "fs_time_tests.rs"]
mod tests;

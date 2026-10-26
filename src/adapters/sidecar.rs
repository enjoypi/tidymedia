#[path = "sidecar_io.rs"]
mod sidecar_io;

use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use serde_derive::Deserialize;
use tracing::debug;

use crate::entities::backend::Backend;
use crate::entities::media_time::Candidate;
use crate::entities::media_time::Source;
use crate::entities::uri::Location;
use crate::entities::xmp;

#[cfg(test)]
use crate::adapters::backend::local::LocalBackend;
pub use sidecar_io::discover;

const FEATURE_SIDECAR: &str = "sidecar";

pub fn discover_with_backend(media_loc: &Location, backend: &Arc<dyn Backend>) -> Vec<Candidate> {
    let mut out = Vec::new();
    if let Some(c) = try_xmp(media_loc, backend.as_ref()) {
        out.push(c);
    }
    if let Some(c) = try_takeout(media_loc, backend.as_ref()) {
        out.push(c);
    }
    out
}

fn try_xmp(media_loc: &Location, backend: &dyn Backend) -> Option<Candidate> {
    let xmp_loc = with_extension(media_loc, "xmp");
    let content = read_sidecar(&xmp_loc, backend, "read_xmp")?;
    let Some(utc) = parse_xmp_date(&content) else {
        log_parse_failure("parse_xmp", &xmp_loc);
        return None;
    };
    Some(Candidate {
        utc,
        offset: None,
        source: Source::XmpSidecar,
        inferred_offset: false,
    })
}

fn try_takeout(media_loc: &Location, backend: &dyn Backend) -> Option<Candidate> {
    let json_loc = append_suffix(media_loc, ".json");
    let content = read_sidecar(&json_loc, backend, "read_takeout")?;
    let Some(utc) = parse_takeout_json(&content) else {
        log_parse_failure("parse_takeout", &json_loc);
        return None;
    };
    Some(Candidate {
        utc,
        offset: None,
        source: Source::GoogleTakeoutJson,
        inferred_offset: false,
    })
}

fn with_extension(loc: &Location, ext: &str) -> Location {
    let mut pp = loc.path().to_path_buf();
    pp.set_extension(ext);
    loc.with_path(pp)
}

fn append_suffix(loc: &Location, sfx: &str) -> Location {
    let p = loc.path();
    loc.with_path(Utf8PathBuf::from(format!("{p}{sfx}")))
}

fn read_sidecar(loc: &Location, backend: &dyn Backend, operation: &'static str) -> Option<String> {
    match sidecar_io::read(loc, backend) {
        Ok(content) => Some(content),
        Err(e) => {
            if should_log_read_error(e.kind()) {
                let path = loc.display();
                let err = e.to_string();
                debug!(
                    feature = FEATURE_SIDECAR,
                    operation,
                    path,
                    result = "error",
                    err,
                    "cannot read sidecar"
                );
            }
            None
        }
    }
}

fn should_log_read_error(kind: io::ErrorKind) -> bool {
    kind != io::ErrorKind::NotFound
}

fn log_parse_failure(operation: &'static str, loc: &Location) {
    let path = loc.display();
    debug!(
        feature = FEATURE_SIDECAR,
        operation,
        path,
        result = "error",
        "cannot parse sidecar"
    );
}

pub(crate) fn parse_xmp_date(content: &str) -> Option<DateTime<Utc>> {
    xmp::parse_xmp_dates(content)
        .photoshop_date_created
        .map(|dt| dt.with_timezone(&Utc))
}

#[derive(Deserialize)]
struct TakeoutTime {
    timestamp: serde_json::Value,
}

#[derive(Deserialize)]
struct TakeoutEnvelope {
    #[serde(rename = "photoTakenTime")]
    photo_taken_time: TakeoutTime,
}

pub(crate) fn parse_takeout_json(content: &str) -> Option<DateTime<Utc>> {
    let env: TakeoutEnvelope = serde_json::from_str(content).ok()?;
    let secs: i64 = match env.photo_taken_time.timestamp {
        serde_json::Value::String(s) => s.parse().ok()?,
        serde_json::Value::Number(n) => n.as_i64()?,
        _ => return None,
    };
    let delta = TimeDelta::try_seconds(secs)?;
    DateTime::<Utc>::UNIX_EPOCH.checked_add_signed(delta)
}

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod tests;

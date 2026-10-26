use std::collections::HashSet;
use std::io;

use camino::Utf8Path;
use tracing::debug;

use crate::entities::backend::{Entry, EntryKind};

use super::map_and_log;
use super::{RemoteAdapter, RemoteTarget};

pub(crate) fn walk_recursive<A: RemoteAdapter>(
    adapter: &A,
    root: &A::Target,
    out: &mut Vec<io::Result<Entry>>,
) {
    let mut stack: Vec<A::Target> = Vec::with_capacity(16);
    let mut visited: HashSet<String> = HashSet::new();
    stack.push(root.clone());
    visited.insert(root.path().as_str().to_owned());
    while let Some(target) = stack.pop() {
        let listed = adapter
            .client()
            .list(&target)
            .map_err(|e| map_and_log(A::scheme(), "list", target.path(), A::map_error, e));
        let entries = match listed {
            Ok(v) => v,
            Err(e) => {
                out.push(Err(e));
                continue;
            }
        };
        for entry in entries {
            if entry.kind == EntryKind::Dir {
                match A::Target::from_location(&entry.location, adapter.ctx()) {
                    Ok(sub) => {
                        if visited.insert(sub.path().as_str().to_owned()) {
                            stack.push(sub);
                        }
                    }
                    Err(e) => {
                        out.push(Err(e));
                        continue;
                    }
                }
            }
            out.push(Ok(entry));
        }
    }
}

pub(crate) fn log_mkparent_err(scheme: &str, path: &Utf8Path, e: &io::Error) {
    debug!(
        feature = "backend",
        scheme,
        operation = "mkparent",
        path = %path,
        result = "error",
        error = %e,
        "mkparent best-effort failed; subsequent write will surface error"
    );
}

use std::fmt::Debug;
use std::io::Write;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::{Interest, Subscriber};
use tracing::{Event, Metadata};
use tracing_subscriber::filter::LevelFilter;

struct OwnCrateSink;

struct DebugSink;

impl Visit for DebugSink {
    fn record_debug(&mut self, _field: &Field, value: &dyn Debug) {
        let _ = write!(std::io::sink(), "{value:?}");
    }
}

fn is_own(meta: &Metadata<'_>) -> bool {
    meta.target().starts_with("tidymedia")
}

impl Subscriber for OwnCrateSink {
    fn register_callsite(&self, meta: &'static Metadata<'static>) -> Interest {
        if is_own(meta) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        is_own(meta)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::TRACE)
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, values: &Record<'_>) {
        values.record(&mut DebugSink);
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        event.record(&mut DebugSink);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[ctor::ctor(unsafe)]
fn install_own_crate_tracing() {
    let _ = tracing::subscriber::set_global_default(OwnCrateSink);
}

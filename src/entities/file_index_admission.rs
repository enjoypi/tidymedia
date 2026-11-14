use crate::entities::backend::Entry;
use crate::entities::file_info::Info;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Take,
    Skip,
    Overflow,
}

pub trait Admission: Send + Sync {
    fn admit(&self, entry: &Entry) -> Admit;
    fn counts_shared_stats(&self) -> bool;
    fn keep_info(&self, info: &Info) -> bool {
        let _ = info;
        true
    }
}

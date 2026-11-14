use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use super::retained::{EntryCost, Histogram, Tally, bucket_of};
use crate::entities::backend::Entry;
use crate::entities::file_index::{Admission, Admit, VisitStats};
use crate::entities::file_info::Info;
use crate::entities::memory::CapacityExceeded;

const OVERSIZED: u32 = u32::MAX;
const SUB_BITS: u32 = 16;
const SUBS: usize = 1 << SUB_BITS;

#[must_use]
pub(crate) fn sub_of(fast_hash: u64) -> usize {
    usize::try_from(fast_hash >> (64 - SUB_BITS)).unwrap_or(0)
}

#[must_use]
fn path_bucket(path: &str) -> usize {
    sub_of(xxhash_rust::xxh3::xxh3_64(path.as_bytes()))
}

#[derive(Debug, PartialEq, Eq)]
enum Sub {
    One(u32),
    Chunks(Vec<u32>),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    bucket_group: Vec<u32>,
    exact: HashMap<u64, u32>,
    split: HashMap<u64, Vec<Sub>>,
    groups_of_size: HashMap<u64, HashSet<u32>>,
    pub groups: u32,
}

enum Slot<'a> {
    Group(u32),
    Split(u64, &'a [Sub]),
}

impl Plan {
    fn slot(&self, size: u64) -> Slot<'_> {
        let group = self.bucket_group[bucket_of(size)];
        if group != OVERSIZED {
            return Slot::Group(group);
        }
        if let Some(group) = self.exact.get(&size) {
            return Slot::Group(*group);
        }
        self.split
            .get(&size)
            .map_or(Slot::Group(0), |subs| Slot::Split(size, subs))
    }

    #[must_use]
    pub(crate) fn filter(self: &Arc<Self>, group: u32) -> Arc<GroupFilter> {
        Arc::new(GroupFilter {
            plan: Arc::clone(self),
            group,
            output: false,
        })
    }

    #[must_use]
    pub(crate) fn output_filter(self: &Arc<Self>, group: u32) -> Arc<GroupFilter> {
        Arc::new(GroupFilter {
            plan: Arc::clone(self),
            group,
            output: true,
        })
    }

    #[must_use]
    pub(crate) fn is_chunked(&self, info: &Info) -> bool {
        matches!(
            self.slot(info.size),
            Slot::Split(_, subs) if matches!(subs[sub_of(info.fast_hash)], Sub::Chunks(_))
        )
    }
}

struct Packer {
    capacity: u64,
    groups: u32,
    used: u64,
}

impl Packer {
    fn try_place(&mut self, bytes: u64) -> Result<u32, CapacityExceeded> {
        if bytes > self.capacity {
            return Err(CapacityExceeded {
                need: bytes,
                capacity: self.capacity,
            });
        }
        Ok(self.place(bytes))
    }

    fn place(&mut self, bytes: u64) -> u32 {
        if self.used + bytes > self.capacity {
            self.groups += 1;
            self.used = 0;
        }
        self.used += bytes;
        self.groups
    }
}

pub(crate) type Walk<'a> = &'a dyn Fn(&dyn Fn(EntryCost) -> Arc<dyn Admission>) -> VisitStats;

pub(crate) struct Planned {
    pub plan: Arc<Plan>,
    pub shared: VisitStats,
}

pub(crate) fn plan_groups(
    capacity: u64,
    names: EntryCost,
    walk: Walk<'_>,
) -> Result<Planned, CapacityExceeded> {
    let hist = Histogram::new();
    let shared = walk(&|cost| {
        Arc::new(Tally {
            hist: Arc::clone(&hist),
            cost,
            name_cost: names,
        })
    });
    let names_bytes = hist.names();
    let capacity = capacity.saturating_sub(names_bytes);
    let mut packer = Packer {
        capacity,
        groups: 0,
        used: 0,
    };
    let charge = hist.snapshot();
    let oversized: Arc<Vec<bool>> = Arc::new(charge.iter().map(|c| *c > capacity).collect());
    let mut bucket_group = Vec::with_capacity(charge.len());
    for bytes in &charge {
        bucket_group.push(if *bytes > capacity {
            OVERSIZED
        } else {
            packer.place(*bytes)
        });
    }
    let (exact, split) = if oversized.iter().any(|o| *o) {
        plan_oversized(&mut packer, &oversized, walk)?
    } else {
        (HashMap::new(), HashMap::new())
    };
    let groups_of_size = split
        .iter()
        .map(|(size, subs)| (*size, subs.iter().flat_map(groups_of_sub).collect()))
        .collect();
    Ok(Planned {
        plan: Arc::new(Plan {
            bucket_group,
            exact,
            split,
            groups_of_size,
            groups: packer.groups + 1,
        }),
        shared,
    })
}

fn groups_of_sub(sub: &Sub) -> Vec<u32> {
    match sub {
        Sub::One(group) => vec![*group],
        Sub::Chunks(groups) => groups.clone(),
    }
}

type Charges = HashMap<u64, Vec<AtomicU64>>;
type Floods = HashMap<(u64, usize), Vec<AtomicU64>>;
type Levels = (HashMap<u64, u32>, HashMap<u64, Vec<Sub>>);

fn zeroed() -> Vec<AtomicU64> {
    (0..SUBS).map(|_| AtomicU64::new(0)).collect()
}

fn plan_oversized(
    packer: &mut Packer,
    oversized: &Arc<Vec<bool>>,
    walk: Walk<'_>,
) -> Result<Levels, CapacityExceeded> {
    let sizes = Arc::new(Mutex::new(BTreeMap::new()));
    walk(&|cost| {
        Arc::new(SizeTally {
            oversized: Arc::clone(oversized),
            sizes: Arc::clone(&sizes),
            cost,
        })
    });
    let sizes = std::mem::take(&mut *sizes.lock());
    let mut exact = HashMap::new();
    let mut heavy = HashMap::new();
    for (size, bytes) in sizes {
        if bytes > packer.capacity {
            heavy.insert(size, zeroed());
        } else {
            exact.insert(size, packer.place(bytes));
        }
    }
    let heavy: Arc<Charges> = Arc::new(heavy);
    let floods: Arc<Floods> = Arc::new(tally_heavy(&heavy, packer.capacity, walk));
    let mut ordered: Vec<_> = heavy.iter().collect();
    ordered.sort_by_key(|(size, _)| **size);
    let mut split = HashMap::new();
    for (size, subs) in ordered {
        let mut planned = Vec::with_capacity(SUBS);
        for (sub, charge) in subs.iter().enumerate() {
            planned.push(match floods.get(&(*size, sub)) {
                Some(chunks) => Sub::Chunks(
                    chunks
                        .iter()
                        .map(|c| packer.try_place(c.load(Ordering::Relaxed)))
                        .collect::<Result<_, _>>()?,
                ),
                None => Sub::One(packer.place(charge.load(Ordering::Relaxed))),
            });
        }
        split.insert(*size, planned);
    }
    Ok((exact, split))
}

fn tally_heavy(heavy: &Arc<Charges>, capacity: u64, walk: Walk<'_>) -> Floods {
    if heavy.is_empty() {
        return HashMap::new();
    }
    walk(&|cost| {
        Arc::new(HashTally {
            sizes: Arc::clone(heavy),
            floods: None,
            cost,
        })
    });
    let floods: Floods = heavy
        .iter()
        .flat_map(|(size, subs)| {
            subs.iter()
                .enumerate()
                .filter(|(_, c)| c.load(Ordering::Relaxed) > capacity)
                .map(|(sub, _)| ((*size, sub), zeroed()))
                .collect::<Vec<_>>()
        })
        .collect();
    if floods.is_empty() {
        return floods;
    }
    let floods = Arc::new(floods);
    walk(&|cost| {
        Arc::new(HashTally {
            sizes: Arc::clone(heavy),
            floods: Some(Arc::clone(&floods)),
            cost,
        })
    });
    Arc::try_unwrap(floods).unwrap_or_default()
}

struct SizeTally {
    oversized: Arc<Vec<bool>>,
    sizes: Arc<Mutex<BTreeMap<u64, u64>>>,
    cost: EntryCost,
}

impl Admission for SizeTally {
    fn admit(&self, entry: &Entry) -> Admit {
        if self.oversized[bucket_of(entry.size)] {
            *self.sizes.lock().entry(entry.size).or_default() += self.cost.of(entry);
        }
        Admit::Skip
    }

    fn counts_shared_stats(&self) -> bool {
        false
    }
}

struct HashTally {
    sizes: Arc<Charges>,
    floods: Option<Arc<Floods>>,
    cost: EntryCost,
}

impl Admission for HashTally {
    fn admit(&self, entry: &Entry) -> Admit {
        if self.sizes.contains_key(&entry.size) {
            Admit::Take
        } else {
            Admit::Skip
        }
    }

    fn counts_shared_stats(&self) -> bool {
        false
    }

    fn keep_info(&self, info: &Info) -> bool {
        let path = info.full_path.as_str();
        let charge = self.cost.of_path(path);
        let sub = sub_of(info.fast_hash);
        match &self.floods {
            None => self.sizes.get(&info.size).into_iter().for_each(|subs| {
                subs[sub].fetch_add(charge, Ordering::Relaxed);
            }),
            Some(floods) => {
                if let Some(chunks) = floods.get(&(info.size, sub)) {
                    chunks[path_bucket(path)].fetch_add(charge, Ordering::Relaxed);
                }
            }
        }
        false
    }
}

pub(crate) struct GroupFilter {
    plan: Arc<Plan>,
    group: u32,
    output: bool,
}

impl GroupFilter {
    #[must_use]
    pub(crate) fn owns_entry(&self, entry: &Entry) -> bool {
        match self.plan.slot(entry.size) {
            Slot::Group(group) => group == self.group,
            Slot::Split(size, _) => self.plan.groups_of_size[&size].contains(&self.group),
        }
    }

    #[must_use]
    pub(crate) fn owns_info(&self, info: &Info) -> bool {
        match self.plan.slot(info.size) {
            Slot::Group(_) => true,
            Slot::Split(_, subs) => match &subs[sub_of(info.fast_hash)] {
                Sub::One(group) => *group == self.group,
                Sub::Chunks(groups) if self.output => groups.contains(&self.group),
                Sub::Chunks(groups) => groups[path_bucket(info.full_path.as_str())] == self.group,
            },
        }
    }
}

impl Admission for GroupFilter {
    fn admit(&self, entry: &Entry) -> Admit {
        if self.owns_entry(entry) {
            Admit::Take
        } else {
            Admit::Skip
        }
    }

    fn counts_shared_stats(&self) -> bool {
        false
    }

    fn keep_info(&self, info: &Info) -> bool {
        self.owns_info(info)
    }
}

#[cfg(test)]
#[path = "retained_plan_tests.rs"]
mod tests;

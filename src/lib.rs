use std::cmp::Reverse;
use std::ops::Deref;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::Duration;

use crate::items::{Item, ItemCache};
use crate::worker::Worker;
use parking_lot::lock_api::ArcMutexGuard;
use rayon::ThreadPool;

pub use crate::query::{CaseMatching, MultiPattern, Pattern, PatternKind};
pub use crate::utf32_string::Utf32String;

mod items;
mod query;
mod utf32_string;
mod worker;
pub use nucleo_matcher::{chars, Matcher, MatcherConfig, Utf32Str};

use parking_lot::{Mutex, MutexGuard, RawMutex};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Match {
    pub score: u32,
    pub idx: u32,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Status {
    pub changed: bool,
    pub running: bool,
}

#[derive(Clone)]
pub struct Items<T> {
    cache: Arc<Mutex<ItemCache>>,
    items: Arc<Mutex<Vec<T>>>,
    notify: Arc<(dyn Fn() + Sync + Send)>,
}

impl<T: Sync + Send> Items<T> {
    pub fn clear(&mut self) {
        self.items.lock().clear();
        self.cache.lock().clear();
    }

    pub fn append(&mut self, items: impl Iterator<Item = (T, Box<[Utf32String]>)>) {
        let mut cache = self.cache.lock();
        let mut items_ = self.items.lock();
        items_.extend(items.map(|(item, text)| {
            cache.push(text);
            item
        }));
        // notify that a new tick will be necessary
        (self.notify)();
    }

    pub fn get(&self) -> impl Deref<Target = [T]> + '_ {
        MutexGuard::map(self.items.lock(), |items| items.as_mut_slice())
    }

    pub fn get_matcher_items(&self) -> impl Deref<Target = [Item]> + '_ {
        MutexGuard::map(self.cache.lock(), |items| items.get())
    }
}

pub struct Nucleo<T: Sync + Send> {
    // the way the API is build we totally don't actually neeed these to be Arcs
    // but this lets us avoid some unsafe
    worker: Arc<Mutex<Worker>>,
    canceled: Arc<AtomicBool>,
    pool: ThreadPool,
    pub items: Items<T>,
    pub matches: Vec<Match>,
    pub pattern: MultiPattern,
    should_notify: Arc<AtomicBool>,
}

impl<T: Sync + Send> Nucleo<T> {
    pub fn new(
        config: MatcherConfig,
        notify: Arc<(dyn Fn() + Sync + Send)>,
        num_threads: Option<usize>,
        case_matching: CaseMatching,
        cols: usize,
        items: impl Iterator<Item = (T, Box<[Utf32String]>)>,
    ) -> Self {
        let mut cache = ItemCache::new();
        let items: Vec<_> = items
            .map(|(item, text)| {
                cache.push(text);
                item
            })
            .collect();
        let matches: Vec<_> = (0..items.len())
            .map(|i| Match {
                score: 0,
                idx: i as u32,
            })
            .collect();
        let (pool, worker) =
            Worker::new(notify.clone(), num_threads, config, matches.clone(), &cache);
        Self {
            canceled: worker.canceled.clone(),
            should_notify: worker.should_notify.clone(),
            items: Items {
                cache: Arc::new(Mutex::new(cache)),
                items: Arc::new(Mutex::new(items)),
                notify,
            },
            pool,
            matches,
            pattern: MultiPattern::new(&config, case_matching, cols),
            worker: Arc::new(Mutex::new(worker)),
        }
    }

    pub fn update_config(&mut self, config: MatcherConfig) {
        self.worker.lock().update_config(config)
    }

    pub fn tick(&mut self, timeout: u64) -> Status {
        self.should_notify.store(false, atomic::Ordering::Relaxed);
        let status = self.pattern.status();
        let items = self.items.cache.lock_arc();
        let canceled = status != query::Status::Unchanged || items.cleared();
        let res = self.tick_inner(timeout, canceled, items, status);
        if !canceled {
            self.should_notify.store(true, atomic::Ordering::Relaxed);
            return res;
        }
        let items = self.items.cache.lock_arc();
        let res = self.tick_inner(timeout, false, items, query::Status::Unchanged);
        self.should_notify.store(true, atomic::Ordering::Relaxed);
        res
    }

    fn tick_inner(
        &mut self,
        timeout: u64,
        canceled: bool,
        items: ArcMutexGuard<RawMutex, ItemCache>,
        status: query::Status,
    ) -> Status {
        let mut inner = if canceled {
            self.pattern.reset_status();
            self.canceled.store(true, atomic::Ordering::Relaxed);
            self.worker.lock_arc()
        } else {
            let Some(worker) = self.worker.try_lock_arc_for(Duration::from_millis(timeout)) else {
                return Status{ changed: false, running: true };
            };
            worker
        };

        let changed = inner.running;
        if inner.running {
            inner.running = false;
            self.matches.clone_from(&inner.matches);
        }

        let running = canceled || inner.items.outdated(&items);
        if running {
            inner.pattern.clone_from(&self.pattern);
            self.canceled.store(false, atomic::Ordering::Relaxed);
            self.pool.spawn(move || unsafe { inner.run(items, status) })
        }
        Status { changed, running }
    }
}

impl<T: Sync + Send> Drop for Nucleo<T> {
    fn drop(&mut self) {
        // we ensure the worker quits before dropping items to ensure that
        // the worker can always assume the items outlife it
        self.canceled.store(true, atomic::Ordering::Relaxed);
        let lock = self.worker.try_lock_for(Duration::from_secs(1));
        if lock.is_none() {
            unreachable!("thread pool failed to shutdown properly")
        }
    }
}
/// convenicne function to easily fuzzy match
/// on a (relatievly small list of inputs). This is not recommended for building a full tui
/// application that can match large numbers of matches as all matching is done on the current
/// thread, effectively blocking the UI
pub fn fuzzy_match<T: AsRef<str>>(
    matcher: &mut Matcher,
    pattern: &str,
    items: impl IntoIterator<Item = T>,
    case_matching: CaseMatching,
) -> Vec<(T, u32)> {
    let mut pattern_ = Pattern::new(&matcher.config, case_matching);
    pattern_.set_literal(pattern, PatternKind::Fuzzy, false);
    let mut buf = Vec::new();
    let mut items: Vec<_> = items
        .into_iter()
        .filter_map(|item| {
            pattern_
                .score(Utf32Str::new(item.as_ref(), &mut buf), matcher)
                .map(|score| (item, score))
        })
        .collect();
    items.sort_by_key(|(item, score)| (Reverse(*score), item.as_ref().len()));
    items
}

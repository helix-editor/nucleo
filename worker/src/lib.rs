use std::ops::Deref;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::Duration;

use crate::items::{Item, ItemCache};
use crate::worker::Worker;
use rayon::ThreadPool;

pub use crate::query::{CaseMatching, Pattern, PatternKind, Query};
pub use crate::utf32_string::Utf32String;

mod items;
mod query;
mod utf32_string;
mod worker;
pub use nucleo_matcher::{chars, Matcher, MatcherConfig, Utf32Str};

use parking_lot::{Mutex, MutexGuard};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Match {
    pub score: u32,
    pub idx: u32,
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
    pub query: Query,
}

impl<T: Sync + Send> Nucleo<T> {
    pub fn new(
        config: MatcherConfig,
        notify: Arc<(dyn Fn() + Sync + Send)>,
        num_threads: Option<usize>,
        case_matching: CaseMatching,
        cols: usize,
    ) -> Self {
        let (pool, worker) = Worker::new(notify.clone(), num_threads, config);
        Self {
            canceled: worker.canceled.clone(),
            items: Items {
                cache: Arc::new(Mutex::new(ItemCache::new())),
                items: Arc::new(Mutex::new(Vec::with_capacity(1024))),
                notify,
            },
            pool,
            matches: Vec::with_capacity(1024),
            query: Query::new(&config, case_matching, cols),
            worker: Arc::new(Mutex::new(worker)),
        }
    }

    pub fn tick(&mut self, timeout: u64) -> bool {
        let status = self.query.status();
        let items = self.items.cache.lock_arc();
        let canceled = status != query::Status::Unchanged || items.cleared();
        let mut inner = if canceled {
            self.query.reset_status();
            self.canceled.store(true, atomic::Ordering::Relaxed);
            self.worker.lock_arc()
        } else {
            let Some(worker) = self.worker.try_lock_arc_for(Duration::from_millis(timeout)) else {
                return true;
            };
            worker
        };

        if inner.running {
            inner.running = false;
            self.matches.clone_from(&inner.matches);
        } else if !canceled {
            // nothing has changed
            return false;
        }

        if canceled || inner.items.outdated(&items) {
            self.pool.spawn(move || unsafe { inner.run(items, status) })
        }
        true
    }
}

impl<T: Sync + Send> Drop for Nucleo<T> {
    fn drop(&mut self) {
        // we ensure the worker quits before dropping items to ensure that
        // the worker can always assume the items outlife it
        self.canceled.store(true, atomic::Ordering::Relaxed);
        drop(self.worker.lock());
    }
}

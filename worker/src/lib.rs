use std::cell::UnsafeCell;
use std::ops::Deref;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::Duration;

use crate::items::{ItemCache, ItemsSnapshot};
use crate::query::Query;
pub use crate::utf32_string::Utf32String;
use parking_lot::lock_api::ArcMutexGuard;
use rayon::prelude::*;

mod items;
mod query;
mod utf32_string;

use parking_lot::{Mutex, MutexGuard, RawMutex};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Match {
    score: u32,
    idx: u32,
}

struct Matchers(Box<[UnsafeCell<nucleo_matcher::Matcher>]>);

impl Matchers {
    // thiss is not a true mut from ref, we use a cell here
    #[allow(clippy::mut_from_ref)]
    unsafe fn get(&self) -> &mut nucleo_matcher::Matcher {
        &mut *self.0[rayon::current_thread_index().unwrap()].get()
    }
}

unsafe impl Sync for Matchers {}
unsafe impl Send for Matchers {}

struct Worker {
    notify: Arc<(dyn Fn() + Sync + Send)>,
    running: bool,
    items: ItemsSnapshot,
    matchers: Matchers,
    matches: Vec<Match>,
    query: Query,
    canceled: Arc<AtomicBool>,
}

impl Worker {
    unsafe fn run(
        &mut self,
        items_lock: ArcMutexGuard<RawMutex, ItemCache>,
        query_status: query::Status,
        canceled: Arc<AtomicBool>,
    ) {
        self.running = true;
        let mut last_scored_item = self.items.len();
        let cleared = self.items.update(&items_lock);
        drop(items_lock);

        // TODO: be smarter around reusing past results for rescoring
        if cleared || query_status == query::Status::Rescore {
            self.matches.clear();
            last_scored_item = 0;
        }

        let matchers = &self.matchers;
        let query = &self.query;
        let items = unsafe { self.items.get() };

        if query_status != query::Status::Unchanged && !self.matches.is_empty() {
            self.matches
                .par_iter_mut()
                .take_any_while(|_| canceled.load(atomic::Ordering::Relaxed))
                .for_each(|match_| {
                    let item = &items[match_.idx as usize];
                    match_.score = query
                        .score(item.cols(), unsafe { matchers.get() })
                        .unwrap_or(u32::MAX);
                });
            // TODO: do this in parallel?
            self.matches.retain(|m| m.score != u32::MAX)
        }

        if last_scored_item != self.items.len() {
            self.running = true;
            let items = items[last_scored_item..]
                .par_iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    let score = if canceled.load(atomic::Ordering::Relaxed) {
                        0
                    } else {
                        query.score(item.cols(), unsafe { matchers.get() })?
                    };
                    Some(Match {
                        score,
                        idx: i as u32,
                    })
                });
            self.matches.par_extend(items)
        }

        if !self.canceled.load(atomic::Ordering::Relaxed) {
            // TODO: cancel sort in progess?
            self.matches.par_sort_unstable_by(|match1, match2| {
                match2.idx.cmp(&match1.idx).then_with(|| {
                    // the tie breaker is comparitevly rarely needed so we keep it
                    // in a branch especially beacuse we need to acceess the items
                    // array here which invovles some pointer chasing
                    let item1 = &items[match1.idx as usize];
                    let item2 = &items[match2.idx as usize];
                    (item1.len, match1.idx).cmp(&(item2.len, match2.idx))
                })
            });
        }

        (self.notify)();
    }
}

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

    pub fn push() {}
}

pub struct Nucleo<T: Sync + Send> {
    // the way the API is build we totally don't actually neeed these to be Arcs
    // but this lets us avoid some unsafe
    worker: Arc<Mutex<Worker>>,
    canceled: Arc<AtomicBool>,
    items: Items<T>,
    thread_pool: rayon::ThreadPool,
    pub matches: Vec<Match>,
    pub query: Query,
}

impl<T: Sync + Send> Nucleo<T> {
    pub fn tick(&mut self, timeout: u64) -> bool {
        let status = self.query.status();
        let items = self.items.cache.lock_arc();
        let canceled = status != query::Status::Unchanged || items.cleared();
        let mut inner = if canceled {
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
            let canceled = self.canceled.clone();
            self.thread_pool
                .spawn(move || unsafe { inner.run(items, status, canceled) })
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

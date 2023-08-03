use std::cmp::Reverse;
use std::sync::atomic::{self, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rayon::ThreadPool;

pub use crate::pattern::{CaseMatching, MultiPattern, Pattern, PatternKind};
pub use crate::utf32_string::Utf32String;
use crate::worker::Woker;
pub use nucleo_matcher::{chars, Matcher, MatcherConfig, Utf32Str};

mod boxcar;
mod pattern;
mod utf32_string;
mod worker;

pub struct Item<'a, T> {
    pub data: &'a T,
    pub matcher_columns: &'a [Utf32String],
}

pub struct Injector<T> {
    items: Arc<boxcar::Vec<T>>,
    notify: Arc<(dyn Fn() + Sync + Send)>,
}

impl<T> Clone for Injector<T> {
    fn clone(&self) -> Self {
        Injector {
            items: self.items.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl<T> Injector<T> {
    /// Appends an element to the back of the vector.
    pub fn push(&self, value: T, fill_columns: impl FnOnce(&mut [Utf32String])) -> u32 {
        let idx = self.items.push(value, fill_columns);
        (self.notify)();
        idx
    }

    /// Returns the total number of items in the current
    /// queue
    pub fn injected_items(&self) -> u32 {
        self.items.count()
    }

    /// Returns a reference to the item at the given index.
    ///
    /// # Safety
    ///
    /// Item at `index` must be initialized. That means you must have observed
    /// `push` returning this value or `get` retunring `Some` for this value.
    /// Just because a later index is initialized doesn't mean that this index
    /// is initialized
    pub unsafe fn get_unchecked(&self, index: u32) -> Item<'_, T> {
        self.items.get_unchecked(index)
    }

    /// Returns a reference to the element at the given index.
    pub fn get(&self, index: u32) -> Option<Item<'_, T>> {
        self.items.get(index)
    }
}

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

pub struct Nucleo<T: Sync + Send + 'static> {
    // the way the API is build we totally don't actually need these to be Arcs
    // but this lets us avoid some unsafe
    canceled: Arc<AtomicBool>,
    should_notify: Arc<AtomicBool>,
    worker: Arc<Mutex<Woker<T>>>,
    pool: ThreadPool,
    cleared: bool,
    item_count: u32,
    pub matches: Vec<Match>,
    pub pattern: MultiPattern,
    pub last_matched_pattern: MultiPattern,
    pub notify: Arc<(dyn Fn() + Sync + Send)>,
    items: Arc<boxcar::Vec<T>>,
}

impl<T: Sync + Send + 'static> Nucleo<T> {
    pub fn new(
        config: MatcherConfig,
        notify: Arc<(dyn Fn() + Sync + Send)>,
        num_threads: Option<usize>,
        case_matching: CaseMatching,
        columns: u32,
    ) -> Self {
        let (pool, worker) = Woker::new(num_threads, config, notify.clone(), columns);
        Self {
            canceled: worker.canceled.clone(),
            should_notify: worker.should_notify.clone(),
            items: worker.items.clone(),
            pool,
            matches: Vec::with_capacity(2 * 1024),
            pattern: MultiPattern::new(&config, case_matching, columns as usize),
            last_matched_pattern: MultiPattern::new(&config, case_matching, columns as usize),
            worker: Arc::new(Mutex::new(worker)),
            cleared: false,
            item_count: 0,
            notify,
        }
    }

    /// Returns the total number of items
    pub fn item_count(&self) -> u32 {
        self.item_count
    }

    pub fn injector(&self) -> Injector<T> {
        Injector {
            items: self.items.clone(),
            notify: self.notify.clone(),
        }
    }

    /// Returns a reference to the item at the given index.
    ///
    /// # Safety
    ///
    /// Item at `index` must be initialized. That means you must have observed
    /// `push` returning this value or `get` retunring `Some` for this value.
    /// Just because a later index is initialized doesn't mean that this index
    /// is initialized
    pub unsafe fn get_unchecked(&self, index: u32) -> Item<'_, T> {
        self.items.get_unchecked(index)
    }

    /// Returns a reference to the element at the given index.
    pub fn get(&self, index: u32) -> Option<Item<'_, T>> {
        self.items.get(index)
    }

    /// Clears all items
    pub fn clear(&mut self) {
        self.canceled.store(true, Ordering::Relaxed);
        self.items = Arc::new(boxcar::Vec::with_capacity(1024, self.items.columns()));
        self.cleared = true
    }

    pub fn update_config(&mut self, config: MatcherConfig) {
        self.worker.lock().update_config(config)
    }

    pub fn push(&self, value: T, fill_columns: impl FnOnce(&mut [Utf32String])) -> u32 {
        let idx = self.items.push(value, fill_columns);
        (self.notify)();
        idx
    }

    pub fn tick(&mut self, timeout: u64) -> Status {
        self.should_notify.store(false, atomic::Ordering::Relaxed);
        let status = self.pattern.status();
        let canceled = status != pattern::Status::Unchanged || self.cleared;
        let res = self.tick_inner(timeout, canceled, status);
        self.cleared = false;
        if !canceled {
            return res;
        }
        self.tick_inner(timeout, false, pattern::Status::Unchanged)
    }

    fn tick_inner(&mut self, timeout: u64, canceled: bool, status: pattern::Status) -> Status {
        let mut inner = if canceled {
            self.pattern.reset_status();
            self.canceled.store(true, atomic::Ordering::Relaxed);
            self.worker.lock_arc()
        } else {
            let Some(worker) = self.worker.try_lock_arc_for(Duration::from_millis(timeout)) else {
                self.should_notify.store(true, Ordering::Release);
                return Status{ changed: false, running: true };
            };
            worker
        };

        let changed = inner.running;

        let running = canceled || self.items.count() > inner.item_count();
        if inner.running {
            inner.running = false;
            if !inner.was_canceled {
                self.item_count = inner.item_count();
                self.last_matched_pattern.clone_from(&inner.pattern);
                self.matches.clone_from(&inner.matches);
            }
        }
        if running {
            inner.pattern.clone_from(&self.pattern);
            self.canceled.store(false, atomic::Ordering::Relaxed);
            if !canceled {
                self.should_notify.store(true, atomic::Ordering::Release);
            }
            let cleared = self.cleared;
            self.pool
                .spawn(move || unsafe { inner.run(status, cleared) })
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
/// on a (relatively small list of inputs). This is not recommended for building a full tui
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

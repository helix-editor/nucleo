use std::cmp::Reverse;
use std::ops::{Bound, RangeBounds};
use std::sync::atomic::{self, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rayon::ThreadPool;

pub use crate::pattern::{CaseMatching, MultiPattern, Pattern, PatternKind};
pub use crate::utf32_string::Utf32String;
use crate::worker::Worker;
pub use nucleo_matcher::{chars, Matcher, MatcherConfig, Utf32Str};

mod boxcar;
mod par_sort;
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

pub struct Snapshot<T: Sync + Send + 'static> {
    item_count: u32,
    matches: Vec<Match>,
    pattern: MultiPattern,
    items: Arc<boxcar::Vec<T>>,
}

impl<T: Sync + Send + 'static> Snapshot<T> {
    fn clear(&mut self, new_items: Arc<boxcar::Vec<T>>) {
        self.item_count = 0;
        self.matches.clear();
        self.items = new_items
    }

    fn update(&mut self, worker: &Worker<T>) {
        self.item_count = worker.item_count();
        self.pattern.clone_from(&worker.pattern);
        self.matches.clone_from(&worker.matches);
        if !Arc::ptr_eq(&worker.items, &self.items) {
            self.items = worker.items.clone()
        }
    }

    /// Returns that total number of items
    pub fn item_count(&self) -> u32 {
        self.item_count
    }

    /// Returns the pattern which items were matched against
    pub fn pattern(&self) -> &MultiPattern {
        &self.pattern
    }

    /// Returns that number of items that matched the pattern
    pub fn matched_item_count(&self) -> u32 {
        self.matches.len() as u32
    }

    /// Returns an iteror over the items that correspond to a subrange of
    /// all the matches in this snapshot.
    ///
    /// # Panics
    /// Panics if `range` has a range bound that is larger than
    /// the matched item count
    pub fn matched_items(
        &self,
        range: impl RangeBounds<u32>,
    ) -> impl Iterator<Item = Item<'_, T>> + ExactSizeIterator + DoubleEndedIterator + '_ {
        // TODO: use TAIT
        let start = match range.start_bound() {
            Bound::Included(&start) => start as usize,
            Bound::Excluded(&start) => start as usize + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end as usize + 1,
            Bound::Excluded(&end) => end as usize,
            Bound::Unbounded => self.matches.len(),
        };
        self.matches[start..end]
            .iter()
            .map(|&m| unsafe { self.items.get_unchecked(m.idx) })
    }

    /// Returns a reference to the item at the given index.
    ///
    /// # Safety
    ///
    /// Item at `index` must be initialized. That means you must have observed
    /// match with the corresponding index in this exact snapshot. Observing
    /// a higher index is not enough as item indices can be non-contigously
    /// initialized
    #[inline]
    pub unsafe fn get_item_unchecked(&self, index: u32) -> Item<'_, T> {
        self.items.get_unchecked(index)
    }

    /// Returns a reference to the item at the given index.
    ///
    /// Returns `None` if the given `index` is not initialized. This function
    /// is only guarteed to return `Some` for item indices that can be found in
    /// the `matches` of this struct. Both small and larger indices may returns
    /// `None`
    #[inline]
    pub fn get_item(&self, index: u32) -> Option<Item<'_, T>> {
        self.items.get(index)
    }

    /// Returns a reference to the nth match.
    ///
    /// Returns `None` if the given `index` is not initialized. This function
    /// is only guarteed to return `Some` for item indices that can be found in
    /// the `matches` of this struct. Both small and larger indices may returns
    /// `None`
    #[inline]
    pub fn get_matched_item(&self, n: u32) -> Option<Item<'_, T>> {
        self.get_item(self.matches.get(n as usize)?.idx)
    }
}

pub struct Nucleo<T: Sync + Send + 'static> {
    // the way the API is build we totally don't actually need these to be Arcs
    // but this lets us avoid some unsafe
    canceled: Arc<AtomicBool>,
    should_notify: Arc<AtomicBool>,
    worker: Arc<Mutex<Worker<T>>>,
    pool: ThreadPool,
    cleared: bool,
    items: Arc<boxcar::Vec<T>>,
    notify: Arc<(dyn Fn() + Sync + Send)>,
    snapshot: Snapshot<T>,
    pub pattern: MultiPattern,
}

impl<T: Sync + Send + 'static> Nucleo<T> {
    pub fn new(
        config: MatcherConfig,
        notify: Arc<(dyn Fn() + Sync + Send)>,
        num_threads: Option<usize>,
        case_matching: CaseMatching,
        columns: u32,
    ) -> Self {
        let (pool, worker) = Worker::new(num_threads, config, notify.clone(), columns);
        Self {
            canceled: worker.canceled.clone(),
            should_notify: worker.should_notify.clone(),
            items: worker.items.clone(),
            pool,
            pattern: MultiPattern::new(&config, case_matching, columns as usize),
            snapshot: Snapshot {
                matches: Vec::with_capacity(2 * 1024),
                pattern: MultiPattern::new(&config, case_matching, columns as usize),
                item_count: 0,
                items: worker.items.clone(),
            },
            worker: Arc::new(Mutex::new(worker)),
            cleared: false,
            notify,
        }
    }

    /// Returns a snapshot of all items
    pub fn snapshot(&self) -> &Snapshot<T> {
        &self.snapshot
    }

    pub fn injector(&self) -> Injector<T> {
        Injector {
            items: self.items.clone(),
            notify: self.notify.clone(),
        }
    }

    /// Restart the the item stream. Removes all items  disconnects all
    /// previously created injectors from this instance. If `clear_snapshot` is
    /// `true` then all items and matched are removed from the
    /// [`Snapshot`](crate::Snapshot) immediately. Otherwise the snapshot will
    /// keep the current matches until the matcher has run again.
    ///
    /// # Note
    ///
    /// The injectors will continue to function but they will not affect this
    /// instance anymore. The old items will only be dropped when all injectors
    /// were dropped.
    pub fn restart(&mut self, clear_snapshot: bool) {
        self.canceled.store(true, Ordering::Relaxed);
        self.items = Arc::new(boxcar::Vec::with_capacity(1024, self.items.columns()));
        self.cleared = true;
        if clear_snapshot {
            self.snapshot.clear(self.items.clone());
        }
    }

    pub fn update_config(&mut self, config: MatcherConfig) {
        self.worker.lock().update_config(config)
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
            if !inner.was_canceled && !self.cleared {
                self.snapshot.update(&inner)
            }
        }
        if running {
            inner.pattern.clone_from(&self.pattern);
            self.canceled.store(false, atomic::Ordering::Relaxed);
            if !canceled {
                self.should_notify.store(true, atomic::Ordering::Release);
            }
            let cleared = self.cleared;
            if cleared {
                inner.items = self.items.clone();
            }
            self.pool
                .spawn(move || unsafe { inner.run(status, cleared) })
        }
        Status { changed, running }
    }
}

impl<T: Sync + Send> Drop for Nucleo<T> {
    fn drop(&mut self) {
        // we ensure the worker quits before dropping items to ensure that
        // the worker can always assume the items outlive it
        self.canceled.store(true, atomic::Ordering::Relaxed);
        let lock = self.worker.try_lock_for(Duration::from_secs(1));
        if lock.is_none() {
            unreachable!("thread pool failed to shutdown properly")
        }
    }
}

/// convenience function to easily fuzzy match
/// on a (relatively small) list of inputs. This is not recommended for building a full tui
/// application that can match large numbers of matches as all matching is done on the current
/// thread, effectively blocking the UI
pub fn fuzzy_match<T: AsRef<str>>(
    matcher: &mut Matcher,
    pattern: &str,
    items: impl IntoIterator<Item = T>,
    case_matching: CaseMatching,
) -> Vec<(T, u32)> {
    let mut pattern_ = Pattern::new(&matcher.config, case_matching);
    if pattern_.is_empty() {
        return items.into_iter().map(|item| (item, 0)).collect();
    }
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

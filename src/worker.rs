use std::cell::UnsafeCell;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;

use nucleo_matcher::MatcherConfig;
use parking_lot::Mutex;
use rayon::{prelude::*, ThreadPool};

use crate::pattern::{self, MultiPattern};
use crate::{boxcar, Match};

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

pub(crate) struct Woker<T: Sync + Send + 'static> {
    pub(crate) running: bool,
    matchers: Matchers,
    pub(crate) matches: Vec<Match>,
    pub(crate) pattern: MultiPattern,
    pub(crate) canceled: Arc<AtomicBool>,
    pub(crate) should_notify: Arc<AtomicBool>,
    pub(crate) was_canceled: bool,
    pub(crate) last_snapshot: u32,
    notify: Arc<(dyn Fn() + Sync + Send)>,
    pub(crate) items: Arc<boxcar::Vec<T>>,
    in_flight: Vec<u32>,
}

impl<T: Sync + Send + 'static> Woker<T> {
    pub(crate) fn item_count(&self) -> u32 {
        self.last_snapshot - self.in_flight.len() as u32
    }
    pub(crate) fn update_config(&mut self, config: MatcherConfig) {
        for matcher in self.matchers.0.iter_mut() {
            matcher.get_mut().config = config;
        }
    }

    pub(crate) fn new(
        worker_threads: Option<usize>,
        config: MatcherConfig,
        notify: Arc<(dyn Fn() + Sync + Send)>,
        cols: u32,
    ) -> (ThreadPool, Self) {
        let worker_threads = worker_threads
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |it| it.get()));
        let pool = rayon::ThreadPoolBuilder::new()
            .thread_name(|i| format!("nucleo worker {i}"))
            .num_threads(worker_threads)
            .build()
            .expect("creating threadpool failed");
        let matchers = (0..worker_threads)
            .map(|_| UnsafeCell::new(nucleo_matcher::Matcher::new(config)))
            .collect();
        let worker = Woker {
            running: false,
            matchers: Matchers(matchers),
            last_snapshot: 0,
            matches: Vec::new(),
            // just a placeholder
            pattern: MultiPattern::new(&config, crate::CaseMatching::Ignore, 0),
            canceled: Arc::new(AtomicBool::new(false)),
            should_notify: Arc::new(AtomicBool::new(false)),
            was_canceled: false,
            notify,
            items: Arc::new(boxcar::Vec::with_capacity(2 * 1024, cols)),
            in_flight: Vec::with_capacity(64),
        };
        (pool, worker)
    }

    unsafe fn process_new_items(&mut self) {
        let matchers = &self.matchers;
        let pattern = &self.pattern;
        self.matches.reserve(self.in_flight.len());
        self.in_flight.retain(|&idx| {
            let Some(item) = self.items.get(idx) else {
                return true;
            };
            let Some(score) = pattern.score(item.matcher_columns, matchers.get()) else {
                return false;
            };
            self.matches.push(Match { score, idx });
            false
        });
        let new_snapshot = self.items.par_snapshot(self.last_snapshot);
        if new_snapshot.end() != self.last_snapshot {
            let end = new_snapshot.end();
            let in_flight = Mutex::new(&mut self.in_flight);
            let items = new_snapshot.filter_map(|(idx, item)| {
                let Some(item) = item else {
                    in_flight.lock().push(idx);
                    return None;
                };
                let score = if self.canceled.load(atomic::Ordering::Relaxed) {
                    0
                } else {
                    pattern.score(item.matcher_columns, matchers.get())?
                };
                Some(Match { score, idx })
            });
            self.matches.par_extend(items);
            self.last_snapshot = end;
        }
    }

    fn remove_in_flight_matches(&mut self) {
        let mut off = 0;
        self.in_flight.retain(|&i| {
            let is_in_flight = self.items.get(i).is_none();
            if is_in_flight {
                self.matches.remove((i - off) as usize);
                off += 1;
            }
            is_in_flight
        });
    }

    unsafe fn process_new_items_trivial(&mut self) {
        let new_snapshot = self.items.snapshot(self.last_snapshot);
        if new_snapshot.end() != self.last_snapshot {
            let end = new_snapshot.end();
            let items = new_snapshot.filter_map(|(idx, item)| {
                if item.is_none() {
                    self.in_flight.push(idx);
                    return None;
                };
                Some(Match { score: 0, idx })
            });
            self.matches.extend(items);
            self.last_snapshot = end;
        }
    }

    pub(crate) unsafe fn run(&mut self, pattern_status: pattern::Status, cleared: bool) {
        self.running = true;
        self.was_canceled = false;

        if cleared {
            self.last_snapshot = 0;
        }

        // TODO: be smarter around reusing past results for rescoring
        let empty_pattern = self.pattern.cols.iter().all(|pat| pat.is_empty());
        if empty_pattern {
            self.matches.clear();
            self.matches
                .extend((0..self.last_snapshot).map(|idx| Match { score: 0, idx }));
            // there are usually only very few in flight items (one for each writer)
            self.remove_in_flight_matches();
            self.process_new_items_trivial();
            if self.should_notify.load(atomic::Ordering::Acquire) {
                (self.notify)();
            }
            return;
        }

        self.process_new_items();
        if pattern_status == pattern::Status::Rescore {
            self.matches.clear();
            self.matches
                .extend((0..self.last_snapshot).map(|idx| Match { score: 0, idx }));
            self.remove_in_flight_matches();
        }

        let matchers = &self.matchers;
        let pattern = &self.pattern;
        if pattern_status != pattern::Status::Unchanged && !self.matches.is_empty() {
            self.matches
                .par_iter_mut()
                .take_any_while(|_| !self.canceled.load(atomic::Ordering::Relaxed))
                .for_each(|match_| {
                    // safety: in-flight items are never added to the matches
                    let item = self.items.get_unchecked(match_.idx);
                    match_.score = pattern
                        .score(item.matcher_columns, matchers.get())
                        .unwrap_or(u32::MAX);
                });
            // TODO: do this in parallel?
            self.matches.retain(|m| m.score != u32::MAX);
        }

        if self.canceled.load(atomic::Ordering::Relaxed) {
            self.was_canceled = true;
        } else {
            // TODO: cancel sort in progress?
            self.matches.par_sort_unstable_by(|match1, match2| {
                match2.score.cmp(&match1.score).then_with(|| {
                    // the tie breaker is comparitevly rarely needed so we keep it
                    // in a branch especially because we need to access the items
                    // array here which involves some pointer chasing
                    let item1 = self.items.get_unchecked(match1.idx);
                    let item2 = &self.items.get_unchecked(match2.idx);
                    let len1: u32 = item1
                        .matcher_columns
                        .iter()
                        .map(|haystack| haystack.len() as u32)
                        .sum();
                    let len2 = item2
                        .matcher_columns
                        .iter()
                        .map(|haystack| haystack.len() as u32)
                        .sum();
                    (len1, match1.idx).cmp(&(len2, match2.idx))
                })
            });
        }

        if self.should_notify.load(atomic::Ordering::Acquire) {
            (self.notify)();
        }
    }
}

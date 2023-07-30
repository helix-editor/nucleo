use std::cell::UnsafeCell;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;

use nucleo_matcher::MatcherConfig;
use parking_lot::lock_api::ArcMutexGuard;
use parking_lot::RawMutex;
use rayon::{prelude::*, ThreadPool};

use crate::items::{ItemCache, ItemsSnapshot};
use crate::query::{self, MultiPattern};
use crate::Match;

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

pub(crate) struct Worker {
    notify: Arc<(dyn Fn() + Sync + Send)>,
    pub(crate) running: bool,
    pub(crate) items: ItemsSnapshot,
    matchers: Matchers,
    pub(crate) matches: Vec<Match>,
    pub(crate) pattern: MultiPattern,
    pub(crate) canceled: Arc<AtomicBool>,
    pub(crate) should_notify: Arc<AtomicBool>,
}

impl Worker {
    pub(crate) fn update_config(&mut self, config: MatcherConfig) {
        for matcher in self.matchers.0.iter_mut() {
            matcher.get_mut().config = config;
        }
    }

    pub(crate) fn new(
        notify: Arc<(dyn Fn() + Sync + Send)>,
        worker_threads: Option<usize>,
        config: MatcherConfig,
        matches: Vec<Match>,
        items: &ItemCache,
    ) -> (ThreadPool, Worker) {
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
        let worker = Worker {
            notify,
            running: false,
            items: ItemsSnapshot::new(items),
            matchers: Matchers(matchers),
            matches,
            // just a placeholder
            pattern: MultiPattern::new(&config, crate::CaseMatching::Ignore, 0),
            canceled: Arc::new(AtomicBool::new(false)),
            should_notify: Arc::new(AtomicBool::new(false)),
        };
        (pool, worker)
    }

    pub(crate) unsafe fn run(
        &mut self,
        items_lock: ArcMutexGuard<RawMutex, ItemCache>,
        query_status: query::Status,
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
        let pattern = &self.pattern;
        let items = unsafe { self.items.get() };

        if self.pattern.cols.iter().all(|pat| pat.is_empty()) {
            self.matches.clear();
            self.matches.extend((0..items.len()).map(|i| Match {
                score: 0,
                idx: i as u32,
            }));
            if self.should_notify.load(atomic::Ordering::Relaxed) {
                (self.notify)();
            }
            return;
        }
        if query_status != query::Status::Unchanged && !self.matches.is_empty() {
            self.matches
                .par_iter_mut()
                .take_any_while(|_| !self.canceled.load(atomic::Ordering::Relaxed))
                .for_each(|match_| {
                    let item = &items[match_.idx as usize];
                    match_.score = pattern
                        .score(item.cols(), unsafe { matchers.get() })
                        .unwrap_or(u32::MAX);
                });
            // TODO: do this in parallel?
            self.matches.retain(|m| m.score != u32::MAX);
        }
        if last_scored_item != self.items.len() {
            let items = items[last_scored_item..]
                .par_iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    let score = if self.canceled.load(atomic::Ordering::Relaxed) {
                        u32::MAX - 1
                    } else {
                        pattern.score(item.cols(), unsafe { matchers.get() })?
                    };
                    Some(Match {
                        score,
                        idx: i as u32,
                    })
                });
            self.matches.par_extend(items);
        }

        if !self.canceled.load(atomic::Ordering::Relaxed) {
            // TODO: cancel sort in progess?
            self.matches.par_sort_unstable_by(|match1, match2| {
                match2.score.cmp(&match1.score).then_with(|| {
                    // the tie breaker is comparitevly rarely needed so we keep it
                    // in a branch especially beacuse we need to acceess the items
                    // array here which invovles some pointer chasing
                    let item1 = &items[match1.idx as usize];
                    let item2 = &items[match2.idx as usize];
                    (item1.len, match1.idx).cmp(&(item2.len, match2.idx))
                })
            });
        }

        if self.should_notify.load(atomic::Ordering::Relaxed) {
            (self.notify)();
        }
    }
}

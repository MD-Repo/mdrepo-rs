//! Run work items on a fixed number of plain threads.
//!
//! `ticket` used to run its landings with rayon's `into_par_iter` on the
//! global pool. Inside a landing, `reference` runs its UniProt fetches on a
//! second pool (`in_pool`, `ThreadPool::install`). A global-pool worker
//! that calls `install` on another pool waits for it by running other jobs
//! from its own pool, and those include whole new landings, which reach
//! the same wait and take more. So far more landings ran at once than
//! `--num-threads` asked for: 79 at once with `--num-threads 4` on
//! 2026-11-19, each holding a database connection (mdrepo-rs#130).
//! [`map_bounded`] starts each item on one of `n` OS threads, so no more
//! than `n` run at once whatever the items do inside (rayon included).

use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// `f` applied to every item, at most `n` at once (`n` 0 is taken as 1),
/// results in the items' order. Each worker takes the next item when it
/// finishes one, so a slow item does not hold up the rest. A panic in `f`
/// reaches the caller with its own message once every worker has stopped;
/// no worker takes a new item after it.
pub fn map_bounded<T, R, F>(items: Vec<T>, n: usize, f: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Sync,
{
    let len = items.len();
    let workers = n.max(1).min(len);
    let next = AtomicUsize::new(0);
    let panicked = AtomicBool::new(false);
    // Each item is taken out of its slot exactly once, by index
    let slots: Vec<Mutex<Option<T>>> =
        items.into_iter().map(|t| Mutex::new(Some(t))).collect();
    let results: Vec<Mutex<Option<R>>> = (0..len).map(|_| Mutex::new(None)).collect();

    let worker = || {
        while !panicked.load(Ordering::SeqCst) {
            let i = next.fetch_add(1, Ordering::SeqCst);
            if i >= len {
                break;
            }
            let item = slots[i]
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("each item is taken once");
            match catch_unwind(AssertUnwindSafe(|| f(item))) {
                Ok(r) => {
                    *results[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                }
                Err(p) => {
                    panicked.store(true, Ordering::SeqCst);
                    resume_unwind(p);
                }
            }
        }
    };

    let payload = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers).map(|_| s.spawn(worker)).collect();
        // Joined here, so the first panic keeps its own payload (an unjoined
        // scoped thread's panic becomes "a scoped thread panicked")
        handles.into_iter().filter_map(|h| h.join().err()).next()
    });
    if let Some(p) = payload {
        resume_unwind(p);
    }

    results
        .into_iter()
        .map(|m| {
            m.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .expect("every item ran")
        })
        .collect()
}

// --------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;
    use std::{sync::Arc, thread, time::Duration};

    /// Counts how many calls are inside at once, and the most ever seen
    #[derive(Default)]
    struct InFlight {
        now: AtomicUsize,
        max: AtomicUsize,
    }

    impl InFlight {
        fn enter(&self) {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.max.fetch_max(now, Ordering::SeqCst);
        }
        fn leave(&self) {
            self.now.fetch_sub(1, Ordering::SeqCst);
        }
        fn max(&self) -> usize {
            self.max.load(Ordering::SeqCst)
        }
    }

    /// A landing's shape: enter; a nested rayon loop; a wait on a second
    /// pool, as `reference::in_pool` does for UniProt; leave
    fn landing(inflight: &InFlight, other: &rayon::ThreadPool, i: usize) -> usize {
        inflight.enter();
        (0..4)
            .into_par_iter()
            .for_each(|_| thread::sleep(Duration::from_millis(2)));
        other.install(|| thread::sleep(Duration::from_millis(10)));
        inflight.leave();
        i * 10
    }

    /// A "global" pool of 2 and a second pool, as in_pool builds
    fn pools() -> (rayon::ThreadPool, rayon::ThreadPool) {
        let build = || {
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap()
        };
        (build(), build())
    }

    #[test]
    fn keeps_the_items_order() {
        let out = map_bounded((0..50).collect(), 4, |i: usize| {
            // later items finish first
            thread::sleep(Duration::from_micros(((50 - i) * 50) as u64));
            i * 2
        });
        assert_eq!(out, (0..50).map(|i| i * 2).collect::<Vec<_>>());
    }

    #[test]
    fn never_more_than_n_at_once() {
        for n in [1, 2, 3, 4] {
            let inflight = InFlight::default();
            map_bounded((0..20).collect(), n, |_: usize| {
                inflight.enter();
                thread::sleep(Duration::from_millis(3));
                inflight.leave();
            });
            assert!(inflight.max() <= n, "n={n}: max {}", inflight.max());
            assert_eq!(inflight.max(), n, "n={n}: all {n} workers used");
        }
    }

    #[test]
    fn nested_rayon_and_a_second_pool_do_not_add_items() {
        // the #130 shape, started from a rayon worker as ticket's caller
        // may be: bounded stays at 2
        let (global, other) = pools();
        let inflight = InFlight::default();
        let out = global.install(|| {
            map_bounded((0..20).collect(), 2, |i: usize| {
                landing(&inflight, &other, i)
            })
        });
        assert!(inflight.max() <= 2, "max {}", inflight.max());
        assert_eq!(out, (0..20).map(|i| i * 10).collect::<Vec<_>>());
    }

    #[test]
    #[ignore = "timing-dependent: shows the cause of #130, not a property of our code"]
    fn rayon_par_iter_runs_more_landings_than_threads() {
        // What ticket.rs did: landings by par_iter on the global pool, each
        // waiting on a second pool. Waiting workers take whole landings.
        let (global, other) = pools();
        let inflight = InFlight::default();
        global.install(|| {
            (0..20)
                .into_par_iter()
                .for_each(|i| _ = landing(&inflight, &other, i))
        });
        eprintln!(
            "rayon par_iter: {} landings at once on 2 threads",
            inflight.max()
        );
        assert!(inflight.max() > 2, "max {}", inflight.max());
    }

    #[test]
    fn zero_is_taken_as_one() {
        let inflight = InFlight::default();
        let out = map_bounded(vec![1, 2, 3], 0, |i: i32| {
            inflight.enter();
            thread::sleep(Duration::from_millis(2));
            inflight.leave();
            i
        });
        assert_eq!(out, vec![1, 2, 3]);
        assert_eq!(inflight.max(), 1);
    }

    #[test]
    fn more_workers_than_items_and_no_items() {
        assert_eq!(map_bounded(vec![7], 8, |i: i32| i + 1), vec![8]);
        assert_eq!(map_bounded(Vec::<i32>::new(), 4, |i| i), Vec::<i32>::new());
    }

    #[test]
    fn every_item_runs_exactly_once() {
        let seen = Arc::new(Mutex::new(vec![0usize; 100]));
        map_bounded((0..100).collect(), 7, |i: usize| {
            seen.lock().unwrap()[i] += 1;
        });
        assert!(seen.lock().unwrap().iter().all(|&c| c == 1));
    }

    #[test]
    fn items_and_results_need_not_be_copy() {
        let out = map_bounded(vec![String::from("a"), "b".into()], 2, |s| vec![s]);
        assert_eq!(out, vec![vec!["a".to_string()], vec!["b".to_string()]]);
    }

    #[test]
    fn a_panic_stops_new_items() {
        let ran = AtomicUsize::new(0);
        let r = catch_unwind(AssertUnwindSafe(|| {
            map_bounded((0..1000).collect(), 1, |i: usize| {
                ran.fetch_add(1, Ordering::SeqCst);
                if i == 3 {
                    panic!("stop");
                }
            })
        }));
        assert!(r.is_err());
        assert_eq!(ran.load(Ordering::SeqCst), 4);
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn a_panic_reaches_the_caller() {
        map_bounded(vec![1, 2, 3], 2, |i: i32| {
            if i == 2 {
                panic!("boom");
            }
            i
        });
    }
}

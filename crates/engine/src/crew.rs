//! Worker threads for the engine's per-block parallel work (the `parallel`
//! feature, offline renders only).
//!
//! A render forks and joins in every block, around a few dozen microseconds
//! of work: far too often for threads that go to sleep and must be woken. So
//! a crew's workers spin while it lives (one render) and pick up each job the
//! moment it is posted; the posting thread works on it too.
//!
//! The work of a block is split into parts, and part `k` goes to thread `k`
//! first: the same instruments and effects stay on the same core from block
//! to block, with their state in its cache. A thread done with its part takes
//! any part not started yet, so no one waits on a thread that is slow to come.

use crate::{add_channel, play_channel, run_insert, ChannelRt, InsertRt};
use std::any::Any;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::*};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// One job: `f(k)` for every part `k`, each run once.
struct Job {
    /// Borrowed from the posting thread's stack, which outlives every use:
    /// [`Crew::run`] returns only once no worker can still reach the job.
    f: *const (dyn Fn(usize) + Sync),
    taken: Vec<AtomicBool>,
    done: AtomicUsize,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
}

impl Job {
    /// Run part `me` (if no one took it yet), then any other part left.
    fn work(&self, me: usize) {
        let n = self.taken.len();
        for k in (me..n).chain(0..me.min(n)) {
            if self.taken[k].swap(true, Relaxed) {
                continue;
            }
            // SAFETY: see `f`.
            let f = unsafe { &*self.f };
            if let Err(e) = catch_unwind(AssertUnwindSafe(|| f(k))) {
                self.panic
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_or_insert(e);
            }
            self.done.fetch_add(1, Release);
        }
    }
}

#[derive(Default)]
struct Shared {
    /// The posted job, or null.
    job: AtomicPtr<Job>,
    /// Workers that may be looking at the posted job.
    active: AtomicUsize,
    stop: AtomicBool,
}

/// Spinning worker threads; stopped and joined when dropped.
pub(crate) struct Crew {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    /// What each chain of [`mix`] costs, smoothed.
    costs: Vec<f64>,
    /// The thread each chain of [`mix`] goes to.
    plan: Vec<usize>,
    /// Scratch, kept to spare allocations.
    routes: Vec<usize>,
}

impl Crew {
    /// A crew for up to `width` parts: as many threads as that and the
    /// machine allow (the caller is one of them). None if that is only the
    /// caller.
    pub fn new(width: usize) -> Option<Crew> {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let threads = cores.min(width);
        if threads < 2 {
            return None;
        }
        let mut crew = Crew {
            shared: Arc::new(Shared::default()),
            workers: vec![],
            costs: vec![],
            plan: vec![],
            routes: vec![],
        };
        for k in 1..threads {
            let s = crew.shared.clone();
            let w = std::thread::Builder::new()
                .name(format!("rosaclef-render-{k}"))
                .spawn(move || worker(&s, k))
                .ok()?; // (dropping the crew stops the ones started)
            crew.workers.push(w);
        }
        Some(crew)
    }

    /// Threads, the caller included.
    fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// `f(k)` for every part `k < n`, on the crew and this thread (part 0
    /// here, part `k` on worker `k` when it comes in time).
    fn run(&self, n: usize, f: &(dyn Fn(usize) + Sync)) {
        // SAFETY (lifetime): workers reach `f` only through `job`, and this
        // function does not return while one can.
        type Erased = *const (dyn Fn(usize) + Sync + 'static);
        let f: *const (dyn Fn(usize) + Sync + '_) = f;
        let job = Job {
            f: unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), Erased>(f) },
            taken: (0..n).map(|_| AtomicBool::new(false)).collect(),
            done: AtomicUsize::new(0),
            panic: Mutex::new(None),
        };
        let s = &self.shared;
        s.job.store(&job as *const Job as *mut Job, SeqCst);
        job.work(0);
        while job.done.load(Acquire) < n {
            std::hint::spin_loop();
        }
        // Withdraw the job, then wait for the workers that may have seen it.
        // A worker counts itself in `active` and then re-reads `job`: if it
        // still finds this job there, its count came before the withdrawal,
        // so the wait below sees it.
        s.job.store(ptr::null_mut(), SeqCst);
        while s.active.load(SeqCst) != 0 {
            std::hint::spin_loop();
        }
        if let Some(e) = job.panic.into_inner().unwrap_or_else(|e| e.into_inner()) {
            resume_unwind(e);
        }
    }
}

impl Drop for Crew {
    fn drop(&mut self) {
        self.shared.stop.store(true, SeqCst);
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker(s: &Shared, me: usize) {
    let mut idle = 0u32;
    while !s.stop.load(Relaxed) {
        let p = s.job.load(Acquire);
        if p.is_null() {
            // Spin through the sequential parts of a block; ease off when
            // nothing comes for a while.
            idle += 1;
            if idle < 1 << 14 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
            continue;
        }
        idle = 0;
        s.active.fetch_add(1, SeqCst);
        if s.job.load(SeqCst) == p {
            // SAFETY: the job stays alive until `active` drops back (see `run`).
            unsafe { &*p }.work(me);
        }
        s.active.fetch_sub(1, SeqCst);
    }
}

/// Raw access to a slice's items from the crew's threads. The caller keeps
/// the borrows apart.
struct Slots<T>(*mut T);

// SAFETY: see above; `T: Send` lets the items be used on other threads.
unsafe impl<T: Send> Sync for Slots<T> {}

impl<T> Slots<T> {
    /// SAFETY: `i` is in bounds and nothing else borrows item `i` meanwhile.
    #[allow(clippy::mut_from_ref)]
    unsafe fn get(&self, i: usize) -> &mut T {
        &mut *self.0.add(i)
    }
}

/// [`crate::Engine`]'s block on the crew: the same as playing every channel
/// and adding it into its insert, then running every insert but the master
/// (which comes later), one after the other.
///
/// The block's chains are independent: an insert with the channels routed
/// to it (played and added in channel order, then its effects), and each
/// channel routed straight to the master. Each chain runs whole on one
/// thread, so every sum is the same as one after another; the channels
/// routed to the master are added in order afterwards. The chains are dealt
/// out to the threads by what they cost lately, the heaviest first.
pub(crate) fn mix(crew: &mut Crew, channels: &mut [ChannelRt], inserts: &mut [InsertRt], n: usize) {
    // Chains 0..ni: inserts 1..=ni; then the channels routed to the master.
    let ni = inserts.len() - 1;
    crew.routes.clear();
    crew.routes.extend(channels.iter().map(|c| c.mixer.index()));
    let direct: Vec<usize> = (0..channels.len())
        .filter(|c| crew.routes[*c] == 0)
        .collect();
    let chains = ni + direct.len();
    if crew.costs.len() != chains {
        crew.costs = vec![0.0; chains];
    }
    // Deal the chains, the costliest first, each to the least loaded thread.
    let threads = crew.threads().min(chains).max(1);
    let mut order: Vec<usize> = (0..chains).collect();
    order.sort_by(|a, b| crew.costs[*b].total_cmp(&crew.costs[*a]));
    let mut load = vec![0.0f64; threads];
    crew.plan.clear();
    crew.plan.resize(chains, 0);
    for c in order {
        let t = (0..threads)
            .min_by(|a, b| load[*a].total_cmp(&load[*b]))
            .unwrap_or(0);
        load[t] += crew.costs[c].max(1.0);
        crew.plan[c] = t;
    }

    let spent: Vec<AtomicUsize> = (0..chains).map(|_| AtomicUsize::new(0)).collect();
    let (chans, ins) = (Slots(channels.as_mut_ptr()), Slots(inserts.as_mut_ptr()));
    let (plan, routes, direct) = (&crew.plan, &crew.routes, &direct);
    crew.run(threads, &|part| {
        for chain in (0..chains).filter(|c| plan[*c] == part) {
            let t0 = Instant::now();
            if chain < ni {
                let k = chain + 1;
                // SAFETY: only this chain touches insert `k` and the channels
                // routed to it.
                let ins = unsafe { ins.get(k) };
                for c in (0..routes.len()).filter(|c| routes[*c] == k) {
                    let ch = unsafe { chans.get(c) };
                    play_channel(ch, n);
                    add_channel(ins, ch, n);
                }
                run_insert(ins, n);
            } else {
                // SAFETY: only this chain touches this channel.
                play_channel(unsafe { chans.get(direct[chain - ni]) }, n);
            }
            spent[chain].store(t0.elapsed().as_nanos() as usize, Relaxed);
        }
    });
    for &c in direct {
        add_channel(&mut inserts[0], &channels[c], n);
    }
    for (cost, s) in crew.costs.iter_mut().zip(&spent) {
        *cost = 0.9 * *cost + 0.1 * s.load(Relaxed) as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_once() {
        let Some(crew) = Crew::new(4) else {
            return;
        };
        for round in 0..if cfg!(miri) { 30 } else { 2000 } {
            let hits: Vec<AtomicUsize> = (0..round % 9).map(|_| AtomicUsize::new(0)).collect();
            crew.run(hits.len(), &|k| {
                hits[k].fetch_add(1, Relaxed);
            });
            assert!(hits.iter().all(|h| h.load(Relaxed) == 1));
        }
    }

    #[test]
    fn a_panic_reaches_the_caller() {
        let Some(crew) = Crew::new(4) else {
            return;
        };
        let r = catch_unwind(AssertUnwindSafe(|| {
            crew.run(6, &|k| {
                if k == 3 {
                    panic!("boom");
                }
            })
        }));
        assert!(r.is_err());
        // The crew still works afterwards.
        let hits = AtomicUsize::new(0);
        crew.run(6, &|_| {
            hits.fetch_add(1, Relaxed);
        });
        assert_eq!(hits.load(Relaxed), 6);
    }
}

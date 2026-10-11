//! How a connection's thread reaches the window's thread (`11-live-link.md`, §8): a job with a
//! deadline. The connection's thread asks and waits; the window takes the jobs each frame (in
//! `logic`, which runs while the window is hidden), does them and answers each once.
//!
//! A job the window has not taken by its deadline is dropped, never done: `bt` has stopped
//! waiting and said so, and a job done unseen (an edit applied after `bt` reported a timeout)
//! would be worse than one dropped. The asker stops waiting at the deadline when the job is
//! still queued; for a job the window took in time it waits a little longer (the slack), so
//! the answer to a job taken just before its deadline still arrives.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::error::Error;

/// How long a connection waits for a job the window can answer at once: a status, a snapshot,
/// marks, the base of an edit.
pub const JOB_WAIT: Duration = Duration::from_secs(5);

/// How long a connection waits for a job that may wait for the person: applying an edit (it
/// waits for the end of a brush stroke or a drag) or saving.
pub const LONG_JOB_WAIT: Duration = Duration::from_secs(20);

/// How long past its deadline the asker of a job the window took in time waits for the answer:
/// the job is being done, and its answer must not be lost.
const ANSWER_SLACK: Duration = Duration::from_secs(5);

/// The jobs on their way from connection threads to the window: [`Jobs::ask`] on a connection's
/// thread, [`Jobs::take`] on the window's. Clones share the same jobs, so the server's handler
/// and the window each keep one.
pub struct Jobs<Kind, Answer> {
    queue: Arc<Queue<Kind, Answer>>,
}

/// What the clones of a [`Jobs`] share.
struct Queue<Kind, Answer> {
    /// The jobs asked for and not taken yet, oldest first.
    waiting: Mutex<Vec<Job<Kind, Answer>>>,
    /// Gets the window to run a frame (`request_repaint`).
    waker: Box<dyn Fn() + Send + Sync>,
    /// How long an asker waits past the deadline of a job the window took.
    slack: Duration,
}

/// A job the window took: what to do, and where the answer goes. Take it apart to do it:
/// `let Job { kind, reply } = job;`, then [`Reply::send`] the answer.
#[derive(Debug)]
pub struct Job<Kind, Answer> {
    /// What the window is asked to do.
    pub kind: Kind,
    /// Where the answer goes, once.
    pub reply: Reply<Answer>,
}

/// Where a job's answer goes. It is sent once; a reply dropped without an answer tells the
/// asker at once that none is coming.
pub struct Reply<Answer> {
    deadline: Instant,
    slot: Arc<Slot<Answer>>,
}

/// One job's answer and how far it has got.
struct Slot<Answer> {
    state: Mutex<State<Answer>>,
    changed: Condvar,
}

/// How far a job has got.
enum State<Answer> {
    /// Asked for, not taken by the window yet.
    Queued,
    /// Taken by the window, not answered yet.
    Taken,
    /// Answered, the answer not collected yet.
    Answered(Answer),
    /// Let go by the window without an answer, taken first or not.
    LetGo {
        /// Whether the window had taken the job.
        taken: bool,
    },
    /// Over for the asker: it collected the answer or stopped waiting.
    Over,
}

impl<Kind, Answer> Jobs<Kind, Answer> {
    /// Jobs whose askers call `waker` to get the window to run a frame and take them
    /// (`egui::Context::request_repaint`).
    pub fn new(waker: impl Fn() + Send + Sync + 'static) -> Jobs<Kind, Answer> {
        Jobs::with_slack(waker, ANSWER_SLACK)
    }

    /// [`Jobs::new`] with another slack: how long past its deadline the asker of a job the
    /// window took waits for the answer.
    pub(crate) fn with_slack(
        waker: impl Fn() + Send + Sync + 'static,
        slack: Duration,
    ) -> Jobs<Kind, Answer> {
        Jobs {
            queue: Arc::new(Queue {
                waiting: Mutex::new(Vec::new()),
                waker: Box::new(waker),
                slack,
            }),
        }
    }

    /// Asks the window to do `kind` within `wait`, wakes it and waits for the answer, on a
    /// connection's thread (never on the window's, which would wait for itself). A job the
    /// window has not taken by then is [`Error::NotTaken`] and is dropped; one it took and did
    /// not answer in time, or let go, is [`Error::NotFinished`].
    pub fn ask(&self, kind: Kind, wait: Duration) -> Result<Answer, Error> {
        let deadline = deadline_after(wait)?;
        let last = deadline
            .checked_add(self.queue.slack)
            .ok_or(Error::WaitTooLong { wait })?;
        let slot = Arc::new(Slot {
            state: Mutex::new(State::Queued),
            changed: Condvar::new(),
        });
        let reply = Reply {
            deadline,
            slot: Arc::clone(&slot),
        };
        lock(&self.queue.waiting).push(Job { kind, reply });
        (self.queue.waker)();
        slot.wait_for_answer(deadline, last)
    }

    /// Takes the jobs waiting, oldest first, for the window to do and answer. Jobs whose deadline
    /// has passed are left out and dropped: their askers have stopped waiting, or are told none
    /// is coming. A job kept for a later frame (an edit waiting for the end of a stroke) should
    /// be dropped once its [`Reply::is_overdue`].
    pub fn take(&self) -> Vec<Job<Kind, Answer>> {
        let waiting = std::mem::take(&mut *lock(&self.queue.waiting));
        let now = Instant::now();
        waiting
            .into_iter()
            .filter(|job| job.reply.mark_taken(now))
            .collect()
    }
}

impl<Kind, Answer> Clone for Jobs<Kind, Answer> {
    fn clone(&self) -> Jobs<Kind, Answer> {
        Jobs {
            queue: Arc::clone(&self.queue),
        }
    }
}

impl<Kind, Answer> std::fmt::Debug for Jobs<Kind, Answer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jobs")
            .field("waiting", &lock(&self.queue.waiting).len())
            .field("slack", &self.queue.slack)
            .finish_non_exhaustive()
    }
}

impl<Answer> Reply<Answer> {
    /// When the asker stops waiting for the job to be taken.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Whether the deadline has passed: a job not started by now is dropped rather than done.
    pub fn is_overdue(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// Sends the answer to the asker. Whether it was still waiting: an answer later than the
    /// slack after the deadline is dropped.
    pub fn send(self, answer: Answer) -> bool {
        let mut state = lock(&self.slot.state);
        match *state {
            State::Queued | State::Taken => {
                *state = State::Answered(answer);
                self.slot.changed.notify_all();
                true
            }
            State::Answered(_) | State::LetGo { .. } | State::Over => false,
        }
    }

    /// Marks the job taken when it is still queued and `now` is before its deadline. Whether it
    /// was: a job that was not is dropped.
    fn mark_taken(&self, now: Instant) -> bool {
        let mut state = lock(&self.slot.state);
        match *state {
            State::Queued if now < self.deadline => {
                *state = State::Taken;
                true
            }
            _ => false,
        }
    }
}

impl<Answer> Drop for Reply<Answer> {
    fn drop(&mut self) {
        let mut state = lock(&self.slot.state);
        let taken = match *state {
            State::Queued => false,
            State::Taken => true,
            State::Answered(_) | State::LetGo { .. } | State::Over => return,
        };
        *state = State::LetGo { taken };
        self.slot.changed.notify_all();
    }
}

impl<Answer> std::fmt::Debug for Reply<Answer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply")
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl<Answer> Slot<Answer> {
    /// Waits for the answer: until `deadline` while the job is queued, until `last` once the
    /// window has taken it.
    fn wait_for_answer(&self, deadline: Instant, last: Instant) -> Result<Answer, Error> {
        let mut state = lock(&self.state);
        loop {
            let now = Instant::now();
            let until = match *state {
                State::Queued if now < deadline => deadline,
                State::Taken if now < last => last,
                State::Queued => {
                    *state = State::Over;
                    return Err(Error::NotTaken);
                }
                State::Taken => {
                    *state = State::Over;
                    return Err(Error::NotFinished);
                }
                State::Answered(_) | State::LetGo { .. } | State::Over => {
                    return match std::mem::replace(&mut *state, State::Over) {
                        State::Answered(answer) => Ok(answer),
                        State::LetGo { taken: false } => Err(Error::NotTaken),
                        _ => Err(Error::NotFinished),
                    };
                }
            };
            state = self
                .changed
                .wait_timeout(state, until.saturating_duration_since(now))
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
    }
}

/// The moment `wait` from now, or [`Error::WaitTooLong`] when there is no such moment.
pub(crate) fn deadline_after(wait: Duration) -> Result<Instant, Error> {
    Instant::now()
        .checked_add(wait)
        .ok_or(Error::WaitTooLong { wait })
}

/// Locks `mutex`, also after a thread panicked holding it: what it guards is changed in single
/// assignments, so it is never half made.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;

    /// Takes jobs until one comes.
    fn take_one<K, A>(jobs: &Jobs<K, A>) -> Job<K, A> {
        let started = Instant::now();
        loop {
            if let Some(job) = jobs.take().into_iter().next() {
                return job;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "no job came");
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Sleeps until `moment`.
    fn sleep_until(moment: Instant) {
        thread::sleep(moment.saturating_duration_since(Instant::now()));
    }

    #[test]
    fn answers_come_back_and_the_window_is_woken() {
        let woken = Arc::new(AtomicUsize::new(0));
        let jobs: Jobs<u32, u32> = Jobs::new({
            let woken = Arc::clone(&woken);
            move || {
                woken.fetch_add(1, Ordering::SeqCst);
            }
        });
        let window = {
            let jobs = jobs.clone();
            thread::spawn(move || {
                for _ in 0..3 {
                    let Job { kind, reply } = take_one(&jobs);
                    assert!(!reply.is_overdue());
                    assert!(reply.send(kind * 2));
                }
            })
        };
        for n in 1..=3 {
            assert_eq!(jobs.ask(n, JOB_WAIT).unwrap(), n * 2);
        }
        window.join().unwrap();
        assert_eq!(woken.load(Ordering::SeqCst), 3, "woken for each job");
        assert!(jobs.take().is_empty());
    }

    #[test]
    fn a_job_past_its_deadline_is_skipped() {
        let jobs: Jobs<&str, ()> = Jobs::new(|| {});
        let started = Instant::now();
        let error = jobs.ask("status", Duration::from_millis(50)).unwrap_err();
        assert!(matches!(error, Error::NotTaken), "{error:?}");
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(50) && waited < Duration::from_secs(2),
            "stopped at the deadline: {waited:?}"
        );
        assert!(jobs.take().is_empty(), "the window never sees it");

        // The window gets to it after its deadline, before the asker has woken.
        let asker = {
            let jobs = jobs.clone();
            thread::spawn(move || jobs.ask("snapshot", Duration::from_millis(30)))
        };
        let queued = Instant::now();
        while lock(&jobs.queue.waiting).is_empty() {
            thread::sleep(Duration::from_millis(1));
        }
        sleep_until(queued + Duration::from_millis(40));
        assert!(jobs.take().is_empty(), "skipped");
        assert!(matches!(asker.join().unwrap(), Err(Error::NotTaken)));
    }

    #[test]
    fn an_answer_sent_just_before_the_deadline_still_arrives() {
        let jobs: Jobs<u32, &str> = Jobs::with_slack(|| {}, Duration::from_secs(2));
        for after_deadline in [false, true] {
            let window = {
                let jobs = jobs.clone();
                thread::spawn(move || {
                    let Job { reply, .. } = take_one(&jobs);
                    let deadline = reply.deadline();
                    if after_deadline {
                        // Taken in time, done a little after: the slack covers it.
                        sleep_until(deadline + Duration::from_millis(50));
                        assert!(reply.is_overdue());
                    } else {
                        sleep_until(deadline - Duration::from_millis(10));
                    }
                    assert!(reply.send("done"), "the asker was waiting");
                })
            };
            let started = Instant::now();
            assert_eq!(jobs.ask(1, Duration::from_millis(200)).unwrap(), "done");
            assert!(started.elapsed() < Duration::from_secs(2));
            window.join().unwrap();
        }
    }

    #[test]
    fn a_job_let_go_unanswered_ends_the_wait_at_once() {
        let jobs: Jobs<u32, ()> = Jobs::with_slack(|| {}, Duration::from_secs(30));
        let window = {
            let jobs = jobs.clone();
            thread::spawn(move || {
                // Kept for a later frame, as an edit waiting for the end of a stroke is, then
                // dropped once overdue.
                let job = take_one(&jobs);
                while !job.reply.is_overdue() {
                    thread::sleep(Duration::from_millis(1));
                }
                drop(job);
            })
        };
        let started = Instant::now();
        let error = jobs.ask(1, Duration::from_millis(200)).unwrap_err();
        assert!(matches!(error, Error::NotFinished), "{error:?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "not the slack's thirty seconds"
        );
        window.join().unwrap();
    }

    #[test]
    fn an_answer_later_than_the_slack_is_dropped() {
        let jobs: Jobs<u32, u32> = Jobs::with_slack(|| {}, Duration::from_millis(100));
        let window = {
            let jobs = jobs.clone();
            thread::spawn(move || take_one(&jobs))
        };
        let started = Instant::now();
        let error = jobs.ask(1, Duration::from_millis(200)).unwrap_err();
        assert!(matches!(error, Error::NotFinished), "{error:?}");
        assert!(started.elapsed() >= Duration::from_millis(300), "the slack");
        let job = window.join().unwrap();
        assert!(!job.reply.send(2), "no one waits for it");
    }

    #[test]
    fn many_askers_each_get_their_own_answer() {
        let jobs: Jobs<u32, u32> = Jobs::new(|| {});
        let window = {
            let jobs = jobs.clone();
            thread::spawn(move || {
                let mut answered = 0;
                while answered < 8 {
                    for Job { kind, reply } in jobs.take() {
                        assert!(reply.send(kind + 100));
                        answered += 1;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            })
        };
        let askers: Vec<_> = (0..8)
            .map(|n| {
                let jobs = jobs.clone();
                thread::spawn(move || jobs.ask(n, JOB_WAIT))
            })
            .collect();
        for (n, asker) in askers.into_iter().enumerate() {
            assert_eq!(asker.join().unwrap().unwrap(), n as u32 + 100);
        }
        window.join().unwrap();
    }

    #[test]
    fn waits_too_long_to_measure_are_refused() {
        let jobs: Jobs<u32, u32> = Jobs::new(|| {});
        assert!(matches!(
            jobs.ask(1, Duration::MAX),
            Err(Error::WaitTooLong { .. })
        ));
        assert!(jobs.take().is_empty(), "nothing asked");
        assert!(deadline_after(Duration::from_secs(600)).is_ok());
    }
}

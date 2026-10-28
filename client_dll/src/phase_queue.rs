//! Owned requests, bounded admission, and cancellation before execution.
//! A running operation retains its storage after its caller times out.
use crate::api::EngineControl;
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
type Reply = Result<EngineControl, String>;
enum State {
    Pending,
    Running,
    Canceled,
    Done(Box<Reply>),
}
struct Request {
    block: EngineControl,
    state: Mutex<State>,
    ready: Condvar,
}
pub struct Ticket(Arc<Request>);
impl Ticket {
    pub fn wait(&self, timeout: Duration) -> Reply {
        let state = self.0.state.lock().map_err(|_| "phase request poisoned")?;
        let (mut state, _) = self
            .0
            .ready
            .wait_timeout_while(state, timeout, |s| {
                matches!(s, State::Pending | State::Running)
            })
            .map_err(|_| "phase wait poisoned")?;
        match &*state {
            State::Done(_) => {
                let State::Done(reply) = std::mem::replace(&mut *state, State::Canceled) else { unreachable!() };
                *reply
            }
            State::Canceled => Err("phase request canceled before execution".into()),
            State::Pending => { *state = State::Canceled; Err("phase timeout; request canceled before execution".into()) }
            State::Running => Err("phase timeout after execution started; operation still owns its data; inspect status before retry".into()),
        }
    }
}
pub struct Queue {
    pending: Mutex<VecDeque<Arc<Request>>>,
    running: AtomicU32,
    completed: AtomicU32,
}
impl Queue {
    pub const fn new() -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            running: AtomicU32::new(0),
            completed: AtomicU32::new(0),
        }
    }
    pub fn enqueue(&self, block: EngineControl) -> Result<Ticket, String> {
        let mut queue = self.pending.lock().map_err(|_| "phase queue poisoned")?;
        queue.retain(|r| r.state.lock().is_ok_and(|s| matches!(*s, State::Pending)));
        if queue.len() >= 16 {
            return Err("phase queue is full".into());
        }
        let request = Arc::new(Request {
            block,
            state: Mutex::new(State::Pending),
            ready: Condvar::new(),
        });
        queue.push_back(request.clone());
        Ok(Ticket(request))
    }
    /// Runs at most one request. Never hold a queue/state mutex during engine code.
    pub fn run_one(&self, execute: impl FnOnce(EngineControl) -> Reply) {
        let request = self.pending.lock().ok().and_then(|mut q| q.pop_front());
        let Some(request) = request else {
            return;
        };
        {
            let Ok(mut state) = request.state.lock() else {
                return;
            };
            if !matches!(*state, State::Pending) {
                return;
            }
            *state = State::Running;
            self.running.fetch_add(1, Ordering::AcqRel);
        }
        let result =
            catch_unwind(AssertUnwindSafe(|| execute(request.block))).unwrap_or_else(|_| {
                Err("phase command panicked; inspect lifecycle diagnostics".into())
            });
        self.completed.fetch_add(1, Ordering::Release);
        self.running.fetch_sub(1, Ordering::AcqRel);
        if let Ok(mut state) = request.state.lock() {
            *state = State::Done(Box::new(result));
            request.ready.notify_all();
        }
    }
    pub fn cancel_pending(&self) -> Result<(), String> {
        let mut queue = self.pending.lock().map_err(|_| "phase queue poisoned")?;
        for request in queue.drain(..) {
            let mut state = request.state.lock().map_err(|_| "phase request poisoned")?;
            if matches!(*state, State::Pending) {
                *state = State::Canceled;
                request.ready.notify_all();
            }
        }
        Ok(())
    }
    pub fn stats(&self) -> (u32, u32, u32) {
        let pending = self.pending.lock().map_or(0, |q| q.len() as u32);
        (
            pending,
            self.running.load(Ordering::Acquire),
            self.completed.load(Ordering::Acquire),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_queue_cancels_late_work_and_shutdown_wakes_waiters() {
        let queue = Queue::new();
        let mut tickets = Vec::new();
        for _ in 0..16 {
            tickets.push(queue.enqueue(EngineControl::new(12)).unwrap());
        }
        assert!(queue.enqueue(EngineControl::new(12)).is_err());
        assert!(
            tickets[0]
                .wait(Duration::ZERO)
                .unwrap_err()
                .contains("before execution")
        );
        queue.run_one(|_| panic!("canceled request executed"));
        queue.cancel_pending().unwrap();
        assert!(
            tickets[1]
                .wait(Duration::from_secs(1))
                .unwrap_err()
                .contains("canceled")
        );
        assert_eq!(queue.stats(), (0, 0, 0));
    }
    #[test]
    fn running_timeout_keeps_request_alive_until_completion_and_recovers_from_panic() {
        let queue = Arc::new(Queue::new());
        let ticket = queue.enqueue(EngineControl::new(12)).unwrap();
        let (started, start) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let worker_queue = queue.clone();
        let worker = std::thread::spawn(move || {
            worker_queue.run_one(|mut b| {
                started.send(()).unwrap();
                resume.recv().unwrap();
                b.cycles = 77;
                Ok(b)
            })
        });
        start.recv().unwrap();
        assert!(
            ticket
                .wait(Duration::ZERO)
                .unwrap_err()
                .contains("after execution started")
        );
        drop(ticket);
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(queue.stats(), (0, 0, 1));
        let ticket = queue.enqueue(EngineControl::new(12)).unwrap();
        queue.run_one(|_| panic!("own fixture panic"));
        assert!(
            ticket
                .wait(Duration::ZERO)
                .unwrap_err()
                .contains("panicked")
        );
        assert_eq!(queue.stats(), (0, 0, 2));
    }
}

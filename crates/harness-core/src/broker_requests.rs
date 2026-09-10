//! Bounded in-memory registry for shared MCP request cancellation.
//! Parent reserves a unique 64-hex key before dispatch, cancels it across HTTP
//! connections, and confirms reclamation before the session admits the next work.
//! Unknown, expired, or released keys never start backend work. Slots store no
//! payloads, and ordinary errors never print request identifiers.

use crate::process::{Cancellation, Deadline};
use std::{
    collections::HashMap,
    io,
    sync::{Mutex, MutexGuard},
};

const MIN_CAPACITY: usize = 1;
const MAX_CAPACITY: usize = 1024;
const KEY_LEN: usize = 64;

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
fn poison() -> io::Error {
    io::Error::other("broker request lock poisoned")
}
fn valid_key(key: &str) -> bool {
    key.len() == KEY_LEN && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn require_key(key: &str) -> io::Result<()> {
    if valid_key(key) {
        Ok(())
    } else {
        Err(invalid("invalid request key"))
    }
}

/// Outcome of a cancel attempt. Pending means work is still running and the
/// stored connection token was signalled; reclamation is confirmed only later.
#[derive(Debug, PartialEq, Eq)]
pub enum CancelState {
    Pending,
    Reclaimed,
}

pub struct Requests {
    inner: Mutex<Inner>,
}

struct Inner {
    capacity: usize,
    next_seq: u64,
    slots: HashMap<String, Slot>,
}

struct Slot {
    seq: u64,
    deadline: Deadline,
    state: SlotState,
}

enum SlotState {
    Prepared,
    Running { cancellation: Cancellation },
    Completed,
}

impl Requests {
    pub fn new(capacity: usize) -> io::Result<Self> {
        if !(MIN_CAPACITY..=MAX_CAPACITY).contains(&capacity) {
            return Err(invalid("invalid request registry capacity"));
        }
        Ok(Self {
            inner: Mutex::new(Inner {
                capacity,
                next_seq: 0,
                slots: HashMap::with_capacity(capacity),
            }),
        })
    }

    fn lock(&self) -> io::Result<MutexGuard<'_, Inner>> {
        self.inner.lock().map_err(|_| poison())
    }

    pub fn reserve(&self, key: String, deadline: Deadline) -> io::Result<()> {
        require_key(&key)?;
        let mut inner = self.lock()?;
        inner.reap_expired_prepared();
        if inner.slots.contains_key(&key) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "request already reserved",
            ));
        }
        if inner.slots.len() >= inner.capacity && !inner.evict_finished() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "request registry is full",
            ));
        }
        let seq = inner.next_seq;
        inner.next_seq = inner.next_seq.wrapping_add(1);
        inner.slots.insert(
            key,
            Slot {
                seq,
                deadline,
                state: SlotState::Prepared,
            },
        );
        Ok(())
    }

    pub fn start(&self, key: &str, cancellation: Cancellation) -> io::Result<Deadline> {
        require_key(key)?;
        let mut inner = self.lock()?;
        if inner.slots.get(key).is_some_and(|slot| {
            matches!(slot.state, SlotState::Prepared) && slot.deadline.expired()
        }) {
            inner.slots.remove(key);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "request reservation expired",
            ));
        }
        match inner.slots.get_mut(key) {
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "request is unknown",
            )),
            Some(slot) => match slot.state {
                SlotState::Prepared => {
                    let deadline = slot.deadline;
                    slot.state = SlotState::Running { cancellation };
                    Ok(deadline)
                }
                SlotState::Running { .. } => Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "request already started",
                )),
                SlotState::Completed => Err(invalid("request already completed")),
            },
        }
    }

    pub fn finish(&self, key: &str) -> io::Result<()> {
        require_key(key)?;
        let mut inner = self.lock()?;
        match inner.slots.get_mut(key) {
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "request is unknown",
            )),
            Some(slot) => match slot.state {
                SlotState::Running { .. } => {
                    slot.state = SlotState::Completed;
                    Ok(())
                }
                SlotState::Prepared | SlotState::Completed => {
                    Err(invalid("request is not running"))
                }
            },
        }
    }

    pub fn cancel(&self, key: &str) -> io::Result<CancelState> {
        require_key(key)?;
        let mut inner = self.lock()?;
        Ok(match inner.slots.get_mut(key) {
            None => CancelState::Reclaimed,
            Some(slot) => match &slot.state {
                SlotState::Prepared => {
                    slot.state = SlotState::Completed;
                    CancelState::Reclaimed
                }
                SlotState::Running { cancellation } => {
                    cancellation.cancel();
                    CancelState::Pending
                }
                SlotState::Completed => CancelState::Reclaimed,
            },
        })
    }

    pub fn release(&self, key: &str) -> io::Result<bool> {
        require_key(key)?;
        let mut inner = self.lock()?;
        if matches!(
            inner.slots.get(key),
            Some(Slot {
                state: SlotState::Running { .. },
                ..
            })
        ) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "request is still running",
            ));
        }
        Ok(inner.slots.remove(key).is_some())
    }

    pub fn running(&self) -> io::Result<usize> {
        let inner = self.lock()?;
        Ok(inner
            .slots
            .values()
            .filter(|slot| matches!(slot.state, SlotState::Running { .. }))
            .count())
    }
}

impl Inner {
    fn reap_expired_prepared(&mut self) {
        self.slots.retain(|_, slot| {
            !matches!(slot.state, SlotState::Prepared) || !slot.deadline.expired()
        });
    }

    fn evict_finished(&mut self) -> bool {
        let oldest = self
            .slots
            .iter()
            .filter(|(_, slot)| matches!(slot.state, SlotState::Completed))
            .min_by_key(|(_, slot)| slot.seq)
            .map(|(key, _)| key.clone());
        oldest.is_some_and(|key| self.slots.remove(&key).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::{CancelState, Requests};
    use crate::process::{Cancellation, Deadline};
    use std::{
        io,
        sync::Barrier,
        sync::atomic::{AtomicUsize, Ordering},
        thread,
        time::Duration,
    };

    const SECRET: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

    fn key(id: u8) -> String {
        format!("{id:064x}")
    }
    fn live() -> Deadline {
        Deadline::after(Duration::from_secs(60)).unwrap()
    }
    fn expired() -> Deadline {
        Deadline::after(Duration::ZERO).unwrap()
    }
    fn registry(capacity: usize) -> Requests {
        Requests::new(capacity).unwrap()
    }
    fn shown(error: &io::Error) -> String {
        format!("{error} {error:?}")
    }
    fn assert_private(error: &io::Error, leaked: &str) {
        let text = shown(error);
        assert!(!text.contains(leaked), "{text}");
        assert!(!text.contains(SECRET), "{text}");
    }

    #[test]
    fn cancel_before_start_prevents_work() {
        let requests = registry(2);
        let token = Cancellation::default();
        requests.reserve(key(1), live()).unwrap();
        assert_eq!(requests.cancel(&key(1)).unwrap(), CancelState::Reclaimed);
        assert!(!token.is_cancelled());
        let error = requests.start(&key(1), token).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_private(&error, &key(1));
        assert_eq!(requests.running().unwrap(), 0);
    }

    #[test]
    fn start_only_once_including_concurrent() {
        let requests = registry(2);
        requests.reserve(key(1), live()).unwrap();
        let first = requests.start(&key(1), Cancellation::default()).unwrap();
        assert!(!first.expired());
        let duplicate = requests
            .start(&key(1), Cancellation::default())
            .unwrap_err();
        assert_eq!(duplicate.kind(), io::ErrorKind::AlreadyExists);
        assert_private(&duplicate, &key(1));
        requests.finish(&key(1)).unwrap();

        requests.reserve(key(2), live()).unwrap();
        let barrier = Barrier::new(4);
        let wins = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    barrier.wait();
                    if requests.start(&key(2), Cancellation::default()).is_ok() {
                        wins.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(wins.load(Ordering::Relaxed), 1);
        assert_eq!(requests.running().unwrap(), 1);
        requests.finish(&key(2)).unwrap();
    }

    #[test]
    fn active_cancel_signals_original_token_until_finish() {
        let requests = registry(1);
        let token = Cancellation::default();
        requests.reserve(key(1), live()).unwrap();
        requests.start(&key(1), token.clone()).unwrap();
        assert!(!token.is_cancelled());
        assert_eq!(requests.cancel(&key(1)).unwrap(), CancelState::Pending);
        assert!(token.is_cancelled());
        assert_eq!(requests.cancel(&key(1)).unwrap(), CancelState::Pending);
        assert_eq!(requests.running().unwrap(), 1);
        requests.finish(&key(1)).unwrap();
        assert_eq!(requests.cancel(&key(1)).unwrap(), CancelState::Reclaimed);
        assert_eq!(requests.running().unwrap(), 0);
    }

    #[test]
    fn duplicate_finish_wrong_state_and_private_errors() {
        let requests = registry(2);
        assert_eq!(
            Requests::new(0).err().unwrap().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            Requests::new(1025).err().unwrap().kind(),
            io::ErrorKind::InvalidInput
        );
        let invalid_key = "secret-request-key-must-not-leak-in-errors!!!!";
        let invalid = requests.reserve(invalid_key.into(), live()).unwrap_err();
        assert_eq!(invalid.kind(), io::ErrorKind::InvalidInput);
        assert_private(&invalid, invalid_key);

        requests.reserve(SECRET.to_owned(), live()).unwrap();
        let again = requests.reserve(SECRET.to_owned(), live()).unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
        assert_private(&again, SECRET);

        let prepared = requests.finish(SECRET).unwrap_err();
        assert_eq!(prepared.kind(), io::ErrorKind::InvalidInput);
        assert_private(&prepared, SECRET);

        requests.start(SECRET, Cancellation::default()).unwrap();
        requests.finish(SECRET).unwrap();
        let duplicate = requests.finish(SECRET).unwrap_err();
        assert_eq!(duplicate.kind(), io::ErrorKind::InvalidInput);
        assert_private(&duplicate, SECRET);

        let missing = requests.finish(&key(9)).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert_private(&missing, &key(9));

        let unknown_start = requests
            .start(&key(9), Cancellation::default())
            .unwrap_err();
        assert_eq!(unknown_start.kind(), io::ErrorKind::NotFound);
        assert_private(&unknown_start, &key(9));
        assert_eq!(requests.cancel(&key(9)).unwrap(), CancelState::Reclaimed);
    }

    #[test]
    fn release_running_fails() {
        let requests = registry(1);
        requests.reserve(key(1), live()).unwrap();
        requests.start(&key(1), Cancellation::default()).unwrap();
        let error = requests.release(&key(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_private(&error, &key(1));
        assert_eq!(requests.running().unwrap(), 1);
        requests.finish(&key(1)).unwrap();
        assert!(requests.release(&key(1)).unwrap());
        assert!(!requests.release(&key(1)).unwrap());
    }

    #[test]
    fn release_eviction_and_expiry_prevent_late_invoke() {
        let requests = registry(1);
        requests.reserve(key(1), live()).unwrap();
        assert!(requests.release(&key(1)).unwrap());
        let released = requests
            .start(&key(1), Cancellation::default())
            .unwrap_err();
        assert_eq!(released.kind(), io::ErrorKind::NotFound);
        assert_private(&released, &key(1));

        requests.reserve(key(2), expired()).unwrap();
        let expired_start = requests
            .start(&key(2), Cancellation::default())
            .unwrap_err();
        assert_eq!(expired_start.kind(), io::ErrorKind::TimedOut);
        assert_private(&expired_start, &key(2));
        requests.reserve(key(3), live()).unwrap();
        requests.start(&key(3), Cancellation::default()).unwrap();
        requests.finish(&key(3)).unwrap();
    }

    #[test]
    fn full_capacity_never_evicts_running() {
        let requests = registry(2);
        requests.reserve(key(1), live()).unwrap();
        requests.start(&key(1), Cancellation::default()).unwrap();
        requests.reserve(key(2), live()).unwrap();
        let prepared = requests.reserve(key(3), live()).unwrap_err();
        assert_eq!(prepared.kind(), io::ErrorKind::WouldBlock);
        assert_private(&prepared, &key(3));
        requests.start(&key(2), Cancellation::default()).unwrap();
        let full = requests.reserve(key(3), live()).unwrap_err();
        assert_eq!(full.kind(), io::ErrorKind::WouldBlock);
        assert_private(&full, &key(3));
        assert_eq!(requests.running().unwrap(), 2);
        requests.finish(&key(1)).unwrap();
        requests.finish(&key(2)).unwrap();
    }

    #[test]
    fn expired_prepared_space_can_reuse() {
        let requests = registry(1);
        requests.reserve(key(1), expired()).unwrap();
        requests.reserve(key(2), live()).unwrap();
        let late = requests
            .start(&key(1), Cancellation::default())
            .unwrap_err();
        assert_eq!(late.kind(), io::ErrorKind::NotFound);
        assert_private(&late, &key(1));
        let deadline = requests.start(&key(2), Cancellation::default()).unwrap();
        assert!(!deadline.expired());
        requests.finish(&key(2)).unwrap();
    }

    #[test]
    fn completed_entries_evict_without_allowing_late_invoke() {
        let requests = registry(2);
        requests.reserve(key(1), live()).unwrap();
        requests.start(&key(1), Cancellation::default()).unwrap();
        requests.finish(&key(1)).unwrap();
        requests.reserve(key(2), live()).unwrap();
        requests.start(&key(2), Cancellation::default()).unwrap();
        requests.finish(&key(2)).unwrap();
        requests.reserve(key(3), live()).unwrap();
        let evicted = requests
            .start(&key(1), Cancellation::default())
            .unwrap_err();
        assert_eq!(evicted.kind(), io::ErrorKind::NotFound);
        assert_private(&evicted, &key(1));
        let completed = requests
            .start(&key(2), Cancellation::default())
            .unwrap_err();
        assert_eq!(completed.kind(), io::ErrorKind::InvalidInput);
        assert_private(&completed, &key(2));
        requests.start(&key(3), Cancellation::default()).unwrap();
        requests.finish(&key(3)).unwrap();
        requests.reserve(key(4), live()).unwrap();
        let second = requests
            .start(&key(2), Cancellation::default())
            .unwrap_err();
        assert_eq!(second.kind(), io::ErrorKind::NotFound);
        assert_private(&second, &key(2));
        requests.start(&key(4), Cancellation::default()).unwrap();
        requests.reserve(key(5), live()).unwrap();
        requests.start(&key(5), Cancellation::default()).unwrap();
        assert_eq!(requests.running().unwrap(), 2);
        let blocked = requests.reserve(key(6), live()).unwrap_err();
        assert_eq!(blocked.kind(), io::ErrorKind::WouldBlock);
        assert_private(&blocked, &key(6));
        requests.finish(&key(4)).unwrap();
        requests.finish(&key(5)).unwrap();
    }

    #[test]
    fn expired_running_is_not_evicted_or_reclaimed() {
        let requests = registry(1);
        let token = Cancellation::default();
        requests.reserve(key(1), live()).unwrap();
        requests.start(&key(1), token.clone()).unwrap();
        {
            let mut inner = requests.lock().unwrap();
            inner.slots.get_mut(&key(1)).unwrap().deadline = expired();
        }
        let blocked = requests.reserve(key(2), live()).unwrap_err();
        assert_eq!(blocked.kind(), io::ErrorKind::WouldBlock);
        assert_private(&blocked, &key(2));
        let late = requests.release(&key(1)).unwrap_err();
        assert_eq!(late.kind(), io::ErrorKind::WouldBlock);
        assert_private(&late, &key(1));
        assert_eq!(requests.running().unwrap(), 1);
        assert_eq!(requests.cancel(&key(1)).unwrap(), CancelState::Pending);
        assert!(token.is_cancelled());
        requests.finish(&key(1)).unwrap();
        assert_eq!(requests.running().unwrap(), 0);
    }

    #[test]
    fn poisoned_lock_returns_fixed_infrastructure_error() {
        let requests = registry(1);
        let _ = std::panic::catch_unwind(|| {
            let _guard = requests.lock().unwrap();
            panic!("poison request lock");
        });
        let error = requests.running().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "broker request lock poisoned");
        assert_private(&error, &key(1));
        assert_private(&error, SECRET);
    }
}

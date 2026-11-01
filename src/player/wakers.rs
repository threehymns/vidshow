//! The wakers of the windows that draw one player.
//!
//! Each window registers its own as it starts, and a surface push or a state change wakes all of
//! them, read at call time so a window opened later still counts.

use rune::Waker;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Wakers(Mutex<Vec<Waker>>);

impl Wakers {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn add(&self, waker: Waker) {
        // A file loaded before this window existed still has a frame to show it.
        waker();
        self.0.lock().unwrap().push(waker);
    }

    pub fn wake(&self) {
        let list: Vec<Waker> = self.0.lock().unwrap().clone();
        for waker in list {
            waker();
        }
    }

    /// One waker that wakes every registered window.
    pub fn combined(self: &Arc<Self>) -> Waker {
        let wakers = self.clone();
        Arc::new(move || wakers.wake())
    }
}

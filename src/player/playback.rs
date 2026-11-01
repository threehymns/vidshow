//! Where playback is in the clip: what it is doing, where the clock is, and the one seek the decode
//! thread has not picked up yet.
//!
//! They are one value behind one lock, so what the windows draw, what the sound does and what the
//! decode thread reads cannot drift apart.

use std::time::Instant;

/// What the clip is doing. A file that cannot be decoded is not a state here: it never becomes a
/// [`crate::player::Player`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    /// Loaded, clock parked.
    #[default]
    Paused,
    Playing,
    /// It ran to its end, and stays there until something seeks or plays it again.
    Ended,
    /// A decode pass stopped with this problem; the last frame stays up.
    Failed(String),
}

impl State {
    fn running(&self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// Where playback is in the clip, and when it last moved.
#[derive(Clone, Copy)]
struct Clock {
    base: f64,
    at: Instant,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            base: 0.0,
            at: Instant::now(),
        }
    }
}

impl Clock {
    fn now(&self, running: bool) -> f64 {
        if running {
            self.base + self.at.elapsed().as_secs_f64()
        } else {
            self.base
        }
    }

    fn set(&mut self, seconds: f64) {
        self.base = seconds.max(0.0);
        self.at = Instant::now();
    }
}

/// Everything the windows and the decode thread have to agree on.
#[derive(Default)]
pub struct Playback {
    state: State,
    clock: Clock,
    seek: Option<f64>,
}

impl Playback {
    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn time(&self) -> f64 {
        self.clock.now(self.state.running())
    }

    pub fn paused(&self) -> bool {
        !self.state.running()
    }

    pub fn take_seek(&mut self) -> Option<f64> {
        self.seek.take()
    }

    /// Moves to `state`, parking or restarting the clock with it so the two stay in step. Returns
    /// whether that changed anything.
    pub fn set_state(&mut self, state: State) -> bool {
        if self.state == state {
            return false;
        }
        let now = self.time();
        self.state = state;
        self.clock.set(now);
        true
    }

    /// Moves the clock and queues the seek that makes the decode thread go there.
    pub fn seek_to(&mut self, seconds: f64) {
        self.clock.set(seconds);
        self.seek = Some(seconds);
    }

    /// Moves the clock without asking for a seek, for a pass that is already at that point.
    pub fn set_time(&mut self, seconds: f64) {
        self.clock.set(seconds);
    }
}

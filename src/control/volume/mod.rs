//! The control window's volume section: a vertical fader on a rail, a numbered dB scale, and a pair
//! of stereo meters for the app's one output, with MUTE at the head and the value above them.
//!
//! It is wired to the app, not to an invented strip: the fader is `cfg.volume`, MUTE is
//! `cfg.muted`, and both go through the same `apply` as every other control, so a shortcut and a
//! drag end in the same place.
//!
//! The meters read the output's real peaks: the audio callback records the loudest sample each
//! channel played since the last frame, measured after the volume but before the mute, and
//! [`Volume::tick`] gives those values a meter's ballistics — a fast jump up, a slow fall, and a
//! peak marker that hangs for a moment and then slides back. A file with no audio track, or a
//! platform without sound, reads as silence.
//!
//! The peaks are read before the meter clamps them to full scale, so a lamp above the tracks can
//! latch on anything that came back over 0 dBFS. It is latched because a clip is an event and not
//! a level, and a click on the lamp clears it.
//!
//! The drawing is split by part: [`cap`] moulds the cap, [`fader`] owns the law and the rail,
//! [`scale`] prints the numbers the cap is read against, [`meters`] draws the tracks and the clip
//! lamp, and [`strip`] assembles them with the key and the value.

mod cap;
mod fader;
mod meters;
mod scale;
mod strip;

pub use fader::stepped;

use crate::control::Presenter;
use meters::Meters;
use rune::{El, el, row};
use rune_kit::Kit;
use strip::{STRIP_H, strip};

/// The section's state: where each meter's level and peak are while the clip plays.
#[derive(Default)]
pub struct Volume {
    levels: [f32; 2],
    peaks: [f32; 2],
    /// Seconds each peak marker stays put before it slides back down.
    holds: [f32; 2],
    /// How far the meters are faded by the mute, 0 while playing and 1 when fully muted.
    dim: f32,
    /// Set when either channel's peak came back over full scale, and kept until the lamp above
    /// the meters is clicked. A clip is an event, not a level, so it latches.
    clipped: bool,
    last: Option<f64>,
}

impl Volume {
    /// Moves each channel toward the peak the output reported for it. Levels jump up quickly and
    /// fall slowly, with a peak marker that hangs and then slides back, the way a real meter
    /// behaves.
    pub fn tick(&mut self, now: f64, peaks: [f32; 2], muted: bool) {
        let dt = self
            .last
            .map(|last| (now - last).clamp(0.0, 0.1) as f32)
            .unwrap_or(0.016);
        self.last = Some(now);
        // The mute fades the bars instead of emptying them: the levels keep moving behind the
        // fade, so unmuting picks up where the signal actually is rather than from silence.
        let faded = if muted { 1.0 } else { 0.0 };
        self.dim += (faded - self.dim) * (1.0 - (-dt * 16.0).exp());
        for (channel, peak) in peaks.iter().enumerate() {
            // Anything past full scale is a clip. This is the only place that can tell: below,
            // the peak is clamped, and from then on a clip and a signal sitting exactly at 0 dBFS
            // draw the same bar.
            if !peak.is_finite() || *peak > 1.0 {
                self.clipped = true;
            }
            let target = peak.clamp(0.0, 1.0);
            let follow = if target > self.levels[channel] {
                1.0 - (-dt * 26.0).exp()
            } else {
                1.0 - (-dt * 7.0).exp()
            };
            self.levels[channel] += (target - self.levels[channel]) * follow;
            if self.levels[channel] >= self.peaks[channel] {
                self.peaks[channel] = self.levels[channel];
                self.holds[channel] = 0.55;
            } else {
                self.holds[channel] = (self.holds[channel] - dt).max(0.0);
                if self.holds[channel] == 0.0 {
                    self.peaks[channel] =
                        (self.peaks[channel] - dt * 0.35).max(self.levels[channel]);
                }
            }
        }
    }

    /// Clears the clip lamp, after the clip has been seen and dealt with.
    pub fn clear_clip(&mut self) {
        self.clipped = false;
    }

    /// Whether the meters still have something to finish: a level or peak on its way down, or a
    /// mute fade in progress. False once they have settled, so the window stops asking for frames.
    pub fn settling(&self) -> bool {
        self.dim > 0.001
            || self.levels.iter().any(|level| *level > 0.0005)
            || self.peaks.iter().any(|peak| *peak > 0.0005)
    }

    /// The section where it belongs: docked under the player bar, against the right edge of the
    /// window.
    pub fn section(&self, s: &Presenter, k: &Kit) -> El {
        let meters = Meters {
            levels: self.levels,
            peaks: self.peaks,
            dim: self.dim,
            clipped: self.clipped,
        };
        el().key("volume-section").w_full().h(STRIP_H).child(
            row()
                .w_full()
                .h_full()
                .center_y()
                .justify_end()
                .child(strip(s, k, meters)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Volume;
    use super::fader::{along_of, gain_at};
    use super::scale::MAX_DB;
    use crate::settings::MAX_VOLUME;

    #[test]
    fn a_peak_over_full_scale_latches_until_it_is_cleared() {
        let mut volume = Volume::default();
        volume.tick(0.0, [0.5, 0.25], false);
        assert!(!volume.clipped, "a signal under full scale is not a clip");
        volume.tick(0.02, [1.2, 0.25], false);
        assert!(volume.clipped, "a peak past full scale latches the lamp");
        volume.tick(0.04, [0.1, 0.1], false);
        assert!(
            volume.clipped,
            "the latch holds after the signal falls back"
        );
        volume.clear_clip();
        assert!(!volume.clipped, "a click clears it");
    }

    #[test]
    fn the_fader_runs_from_the_boost_to_silence() {
        assert!(
            (gain_at(0.0) - MAX_VOLUME).abs() < 1e-4,
            "the top of the travel is the +6 dB ceiling"
        );
        assert_eq!(gain_at(1.0), 0.0, "the bottom of the travel is silence");
        let unity = along_of(1.0);
        assert!(
            (gain_at(unity) - 1.0).abs() < 1e-4,
            "unity is reachable, and not only at the stop"
        );
        assert!(
            (MAX_DB - 20.0 * MAX_VOLUME.log10()).abs() < 0.01,
            "the ceiling and the fader's top agree"
        );
    }

    #[test]
    fn a_clip_exactly_at_full_scale_does_not_latch() {
        let mut volume = Volume::default();
        volume.tick(0.0, [1.0, 1.0], false);
        assert!(!volume.clipped);
    }
}

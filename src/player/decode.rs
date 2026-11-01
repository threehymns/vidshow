//! The decode thread: one ffmpeg at a time, reopened for seeks and for looping.
//!
//! A pass is one ffmpeg process and how far into the clip it has read. Each pass ends with an
//! [`Outcome`] that says what to do next, so the loop's state is named rather than carried in
//! loose flags.

use super::{Inner, State};
use crate::media::{self, MediaInfo, Tools};
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

/// One ffmpeg process: where it started in the clip, how many frames it has given us, and whether
/// the next frame must be shown whatever the clock or a pause says.
struct Pass {
    start: f64,
    index: u64,
    force_first: bool,
}

impl Pass {
    fn at(start: f64) -> Self {
        Self {
            start,
            index: 0,
            force_first: true,
        }
    }
}

/// What ended a pass.
enum Outcome {
    /// A seek: the next ffmpeg starts here.
    Seek(f64),
    /// The clip looped: start again from the beginning.
    Restart,
    /// The clip ran out, or the app is closing.
    Done,
}

/// What to do with the frame that has just come due.
enum Due {
    Show,
    /// Paused, or too late to be worth showing: a slow decode skips instead of dragging the
    /// picture behind the sound.
    Skip,
    Seek(f64),
    Done,
}

pub(super) fn decode_loop(inner: Arc<Inner>, tools: Tools, info: MediaInfo) {
    let mut pass = Pass::at(inner.now());
    loop {
        let outcome = run_pass(&inner, &tools, &info, &mut pass);
        // Whatever ended the pass, its ffmpeg has nothing more to give.
        inner.kill_video();
        match outcome {
            Ok(Outcome::Seek(target)) => pass = Pass::at(target),
            Ok(Outcome::Restart) => {
                pass = Pass::at(0.0);
                pass.force_first = false;
            }
            Ok(Outcome::Done) => return,
            Err(error) => {
                inner.set_state(State::Failed(error));
                return;
            }
        }
    }
}

fn run_pass(
    inner: &Arc<Inner>,
    tools: &Tools,
    info: &MediaInfo,
    pass: &mut Pass,
) -> Result<Outcome, String> {
    let (mut child, log) = media::spawn_video(tools, info, pass.start)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("ffmpeg gave no output".into());
    };
    inner.set_video_process(child);
    let mut reader = BufReader::with_capacity(1 << 16, stdout);
    let frame_bytes = info.frame_bytes();

    loop {
        if inner.stopped() {
            return Ok(Outcome::Done);
        }
        // Pick up a seek before touching the pipe: the seeker already killed ffmpeg.
        if let Some(target) = inner.take_seek() {
            return Ok(Outcome::Seek(target));
        }
        if inner.paused() {
            inner.idle(Duration::from_millis(50));
            continue;
        }

        let mut frame = vec![0u8; frame_bytes];
        match media::read_frame(&mut reader, &mut frame) {
            Ok(true) => {}
            Ok(false) => return Ok(end_of_stream(inner)),
            Err(error) => {
                // A decode error after some frames is usually a damaged tail; report what ffmpeg
                // said rather than the bare read error.
                let said = log
                    .lock()
                    .map(|text| text.trim().to_string())
                    .unwrap_or_default();
                return Err(if said.is_empty() { error } else { said });
            }
        }

        let pts = pass.start + pass.index as f64 / info.fps.value;
        pass.index += 1;
        match due(inner, pass, pts) {
            Due::Show => {}
            Due::Skip => continue,
            Due::Seek(target) => return Ok(Outcome::Seek(target)),
            Due::Done => return Ok(Outcome::Done),
        }
        inner.publish(frame);
        pass.force_first = false;
        inner.promote();
    }
}

/// Sleeps until the frame's presentation time, and decides whether it is still wanted.
fn due(inner: &Inner, pass: &Pass, pts: f64) -> Due {
    loop {
        if inner.stopped() {
            return Due::Done;
        }
        if let Some(target) = inner.take_seek() {
            return Due::Seek(target);
        }
        if inner.paused() {
            return Due::Skip;
        }
        let now = inner.now();
        if now + 0.001 >= pts {
            let late = now - pts > 0.20;
            return if late && !pass.force_first {
                Due::Skip
            } else {
                Due::Show
            };
        }
        std::thread::sleep(Duration::from_secs_f64((pts - now).clamp(0.001, 0.02)));
    }
}

/// The decoder reached the end: loop, park at the end until something asks for a seek, or stop.
fn end_of_stream(inner: &Inner) -> Outcome {
    // A seek kills ffmpeg mid-stream, so a short read is not always the end of the clip.
    if let Some(target) = inner.take_seek() {
        return Outcome::Seek(target);
    }
    if inner.stopped() {
        return Outcome::Done;
    }
    if inner.looping() {
        inner.rewind();
        return Outcome::Restart;
    }
    inner.finish();
    // Wait for a replay, a seek or the app closing.
    loop {
        if inner.stopped() {
            return Outcome::Done;
        }
        if let Some(target) = inner.take_seek() {
            return Outcome::Seek(target);
        }
        inner.idle(Duration::from_millis(100));
    }
}

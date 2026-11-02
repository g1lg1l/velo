//! Ableton Link (docs/control.md §4): tempo and beat shared with Live, DJ apps and phones on the
//! network. In builds with the `link` feature (the default: the app is GPL for it, ARCHITECTURE.md
//! D11), off until switched on from the tempo menu. While it's on the beat clock
//! follows the session's beat on a 4-beat bar, and a tempo set in Velo (typed, tapped, heard, a
//! MIDI clock's) goes to the session.

use std::cell::Cell;
use std::time::{Duration, Instant};

use rusty_link::{AblLink, SessionState};

/// Beats to a bar: the phase peers agree on.
const QUANTUM: f64 = 4.0;
/// A tap's tempo reaches the beat clock (on the render thread) a few frames later: read then.
const SETTLE: Duration = Duration::from_millis(80);

pub struct Link {
    link: AblLink,
    /// A tap here, waiting for the beat clock to have its tempo.
    tapped: Cell<Option<Instant>>,
}

impl Link {
    pub fn new() -> Self {
        Self { link: AblLink::new(120.0), tapped: Cell::new(None) }
    }

    /// On or off. Alone on the network, the session starts at the beat clock's tempo and beat
    /// (`beats` now): switching on doesn't jump; with peers already there, Velo joins theirs.
    pub fn enable(&self, on: bool, bpm: f32, beats: f64) {
        self.link.enable(on);
        if on && self.link.num_peers() == 0 {
            let mut state = SessionState::new();
            self.link.capture_app_session_state(&mut state);
            let time = self.link.clock_micros();
            state.set_tempo(f64::from(bpm), time);
            state.request_beat_at_time(beats, time, QUANTUM);
            self.link.commit_app_session_state(&state);
        }
    }

    /// The peers on the network while it's on.
    pub fn peers(&self) -> Option<u64> {
        self.link.is_enabled().then(|| self.link.num_peers())
    }

    /// A tempo set here: the session's from now.
    pub fn set_tempo(&self, bpm: f32) {
        if !self.link.is_enabled() {
            return;
        }
        let mut state = SessionState::new();
        self.link.capture_app_session_state(&mut state);
        state.set_tempo(f64::from(bpm), self.link.clock_micros());
        self.link.commit_app_session_state(&state);
    }

    /// A tap here: its tempo goes to the session once the beat clock has it.
    pub fn tapped(&self) {
        self.tapped.set(Some(Instant::now()));
    }

    /// Once a frame, with the beat clock's tempo now: the session's tempo and beat now, for the
    /// beat clock to follow; none while it's off, or while a tap settles.
    pub fn frame(&self, clock_bpm: f32) -> Option<(f32, f64, Instant)> {
        if !self.link.is_enabled() {
            return None;
        }
        if let Some(at) = self.tapped.get() {
            if at.elapsed() < SETTLE {
                return None;
            }
            self.tapped.set(None);
            self.set_tempo(clock_bpm);
        }
        let mut state = SessionState::new();
        self.link.capture_app_session_state(&mut state);
        let (time, now) = (self.link.clock_micros(), Instant::now());
        Some((state.tempo() as f32, state.beat_at_time(time, QUANTUM), now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two Links in one process find each other and share a tempo and a bar. Run by hand
    /// (`--features link -- --ignored`): it uses the network (macOS may ask to allow it).
    #[test]
    #[ignore]
    fn two_peers_share_tempo_and_beat() {
        let (a, b) = (Link::new(), Link::new());
        a.enable(true, 120.0, 0.0);
        b.enable(true, 97.0, 0.0);
        let deadline = Instant::now() + Duration::from_secs(10);
        while (a.peers() != Some(1) || b.peers() != Some(1)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!((a.peers(), b.peers()), (Some(1), Some(1)), "they found each other");
        b.set_tempo(133.0);
        let deadline = Instant::now() + Duration::from_secs(5);
        while a.frame(0.0).is_some_and(|f| (f.0 - 133.0).abs() > 0.01) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let ((ta, beat_a, at_a), (tb, beat_b, at_b)) = (a.frame(0.0).unwrap(), b.frame(0.0).unwrap());
        assert!((ta - 133.0).abs() < 0.01 && (tb - 133.0).abs() < 0.01, "{ta} {tb}");
        // the same place in the bar, given the moments they were read
        let beat_b_at_a = beat_b - (at_b - at_a).as_secs_f64() * 133.0 / 60.0;
        let apart = (beat_a - beat_b_at_a).rem_euclid(QUANTUM);
        assert!(apart.min(QUANTUM - apart) < 0.01, "a {beat_a}, b {beat_b_at_a}");
    }
}

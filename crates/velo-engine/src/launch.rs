//! Launching scenes (docs/scenes.md §3): a launch waits for its graph, then for its quantize
//! boundary on the beat clock, then cuts or crossfades. Pure state, no GPU: the render thread
//! steps it every frame and renders what it says. Graphs are named by the key the UI installed
//! them with; 0 is none.

use velo_core::{Quantize, Transition};

/// The first boundary after `beats` on a grid of `every` beats; `beats` itself without a grid.
pub fn boundary(beats: f64, every: f64) -> f64 {
    if every > 0.0 { ((beats / every).floor() + 1.0) * every } else { beats }
}

/// A crossfade's length in seconds: `beats` at `bpm`.
pub fn fade_secs(beats: f32, bpm: f32) -> f64 {
    f64::from(beats.max(0.0)) * 60.0 / f64::from(bpm.max(1.0))
}

/// What goes out: the live graph, a launch waiting for its moment, a crossfade under way.
#[derive(Debug, Default)]
pub struct Show {
    /// On the output: the incoming graph as soon as a crossfade starts.
    pub live: u64,
    armed: Option<Armed>,
    fade: Option<Fade>,
}

#[derive(Debug)]
struct Armed {
    key: u64,
    transition: Transition,
    beats: f32,
    every: f64,
    /// Once its graph is there: where on the clock the wait began, and the boundary it ends on.
    wait: Option<(f64, f64)>,
}

#[derive(Debug)]
struct Fade {
    from: u64,
    start: f64,
    secs: f64,
    transition: Transition,
}

impl Show {
    /// Launches `key`: it replaces any launch still waiting. The live graph again calls the
    /// waiting one off.
    pub fn launch(&mut self, key: u64, transition: Transition, beats: f32, quantize: Quantize) {
        self.armed = (key != self.live).then_some(Armed { key, transition, beats, every: quantize.beats(), wait: None });
    }

    /// One frame at `t` seconds, `beats` on the clock. A launch waits until `ready` says its
    /// graph is built (never a hitch), then for the boundary after that; there it cuts, or
    /// starts a crossfade (one still under way ends at once: its incoming graph is the live one).
    pub fn step(&mut self, t: f64, beats: f64, bpm: f32, ready: impl Fn(u64) -> bool) {
        if let Some(a) = &mut self.armed
            && ready(a.key)
        {
            // the clock jumped back (a resync, taps): wait for the next boundary from here
            match a.wait {
                Some((since, _)) if beats >= since => {}
                _ => a.wait = Some((beats, boundary(beats, a.every))),
            }
            if a.wait.is_some_and(|(_, at)| beats >= at) {
                let fades = a.transition != Transition::Cut && a.beats > 0.0 && self.live != 0;
                self.fade = fades.then(|| Fade { from: self.live, start: t, secs: fade_secs(a.beats, bpm), transition: a.transition });
                self.live = a.key;
                self.armed = None;
            }
        }
        if self.fade.as_ref().is_some_and(|f| t - f.start >= f.secs) {
            self.fade = None;
        }
    }

    /// The transition under way: the outgoing graph and how far it is (0..1).
    pub fn fade(&self, t: f64) -> Option<(u64, f32)> {
        self.fade.as_ref().map(|f| (f.from, ((t - f.start) / f.secs).clamp(0.0, 1.0) as f32))
    }

    /// Which transition is under way (Crossfade with none).
    pub fn transition(&self) -> Transition {
        self.fade.as_ref().map_or(Transition::Crossfade, |f| f.transition)
    }

    /// The launch waiting, and how far along its wait is (0..1; 0 while its graph builds).
    pub fn armed(&self, beats: f64) -> Option<(u64, f32)> {
        self.armed.as_ref().map(|a| {
            let progress = a.wait.map_or(0.0, |(since, at)| if at > since { ((beats - since) / (at - since)).clamp(0.0, 1.0) as f32 } else { 1.0 });
            (a.key, progress)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Clock;

    const CROSSFADE: Transition = Transition::Crossfade;

    #[test]
    fn launches_wait_for_the_next_beat_bar_or_four_bars() {
        assert_eq!(boundary(0.0, 1.0), 1.0, "on a beat: the next one, not this one");
        assert_eq!(boundary(4.3, 1.0), 5.0);
        assert_eq!(boundary(4.3, 4.0), 8.0);
        assert_eq!(boundary(7.99, 4.0), 8.0);
        assert_eq!(boundary(4.3, 16.0), 16.0);
        assert_eq!(boundary(17.0, 16.0), 32.0);
        assert_eq!(boundary(4.3, 0.0), 4.3, "Off: now");
        assert_eq!(boundary(-0.5, 4.0), 0.0, "before beat 0 too");
        // against the clock: 120 BPM from t = 0, a beat every half second, a bar every two
        let clock = Clock::default();
        for (q, at) in [(Quantize::Off, 1.3), (Quantize::Beat, 1.5), (Quantize::Bar, 2.0), (Quantize::Bars4, 8.0)] {
            let b = boundary(clock.beats(1.3), q.beats());
            assert!((b * 0.5 - at).abs() < 1e-9, "{q:?} from 1.3 s lands at {at} s, not {}", b * 0.5);
        }
    }

    /// Steps a show at 60 Hz from `t0` for `secs` on a 120 BPM clock; every graph is built.
    fn run(show: &mut Show, t0: f64, secs: f64) -> f64 {
        let clock = Clock::default();
        let mut t = t0;
        while t < t0 + secs {
            show.step(t, clock.beats(t), clock.bpm(), |_| true);
            t += 1.0 / 60.0;
        }
        t
    }

    #[test]
    fn every_transition_but_a_cut_takes_its_beats() {
        for transition in Transition::ALL {
            let mut show = Show::default();
            show.launch(1, Transition::Cut, 4.0, Quantize::Off);
            let t = run(&mut show, 0.0, 0.1);
            show.launch(2, transition, 4.0, Quantize::Off);
            run(&mut show, t, 0.5);
            assert_eq!(show.fade(t + 0.5).is_some(), transition != Transition::Cut, "{transition:?}");
            if transition != Transition::Cut {
                assert_eq!(show.transition(), transition);
            }
        }
    }

    #[test]
    fn a_cut_swaps_on_the_boundary_and_a_crossfade_blends_over_its_beats() {
        let mut show = Show::default();
        // the first launch: nothing to fade from, so it cuts
        show.launch(1, CROSSFADE, 4.0, Quantize::Off);
        run(&mut show, 0.0, 0.01);
        assert_eq!((show.live, show.fade(0.0)), (1, None));
        // a cut on the bar: armed until 2 s, then swapped at once
        show.launch(2, Transition::Cut, 4.0, Quantize::Bar);
        let t = run(&mut show, 0.6, 1.0);
        let (key, wait) = show.armed(Clock::default().beats(t)).unwrap();
        assert!(key == 2 && wait > 0.6 && wait < 0.9, "a countdown, {wait}");
        assert_eq!(show.live, 1, "still waiting at {t} s");
        let t = run(&mut show, t, 0.5);
        assert_eq!((show.live, show.armed(0.0), show.fade(t)), (2, None, None), "cut at 2 s");
        // a crossfade of 2 beats on the next beat: 1 s at 120 BPM, both render meanwhile
        show.launch(3, CROSSFADE, 2.0, Quantize::Beat);
        let t = run(&mut show, t, 0.5);
        assert_eq!(show.live, 3, "the incoming graph is live once the fade starts");
        let (from, p) = show.fade(t).unwrap();
        assert!(from == 2 && p > 0.0 && p < 0.5, "{p}");
        let (_, half) = show.fade(t + 0.5 - p as f64).unwrap();
        assert!((half - 0.5).abs() < 0.02, "half way after half a second: {half}");
        let t = run(&mut show, t, 1.0);
        assert_eq!(show.fade(t), None, "done after 2 beats: the old graph is out");
    }

    #[test]
    fn a_crossfade_lasts_its_beats_at_the_tempo() {
        assert_eq!(fade_secs(4.0, 120.0), 2.0);
        assert_eq!(fade_secs(4.0, 60.0), 4.0);
        assert_eq!(fade_secs(1.0, 150.0), 0.4);
        let mut show = Show { live: 1, ..Default::default() };
        show.launch(2, CROSSFADE, 8.0, Quantize::Off);
        show.step(10.0, 0.0, 140.0, |_| true);
        let secs = fade_secs(8.0, 140.0);
        for (dt, p) in [(0.0, 0.0), (secs / 4.0, 0.25), (secs * 0.9, 0.9)] {
            assert!((show.fade(10.0 + dt).unwrap().1 - p).abs() < 1e-5);
        }
        show.step(10.0 + secs, 0.0, 140.0, |_| true);
        assert_eq!(show.fade(10.0 + secs), None);
    }

    #[test]
    fn a_launch_waits_for_its_graph_then_for_the_next_boundary() {
        let mut show = Show { live: 1, ..Default::default() };
        show.launch(2, Transition::Cut, 4.0, Quantize::Bar);
        // building: beat 6 passes and so does the bar at 8, nothing happens
        for beats in [6.0, 8.5] {
            show.step(beats / 2.0, beats, 120.0, |k| k != 2);
            assert_eq!((show.live, show.armed(beats)), (1, Some((2, 0.0))));
        }
        // built at beat 9: the next bar is 12
        show.step(4.5, 9.0, 120.0, |_| true);
        assert_eq!(show.armed(10.0), Some((2, 1.0 / 3.0)));
        show.step(5.5, 11.0, 120.0, |_| true);
        assert_eq!(show.live, 1);
        show.step(6.0, 12.0, 120.0, |_| true);
        assert_eq!((show.live, show.armed(12.0)), (2, None));
    }

    #[test]
    fn a_new_launch_replaces_the_waiting_one_and_the_live_scene_calls_it_off() {
        let mut show = Show { live: 1, ..Default::default() };
        show.launch(2, Transition::Cut, 4.0, Quantize::Bar);
        show.launch(3, Transition::Cut, 4.0, Quantize::Bar);
        assert_eq!(show.armed(0.0).map(|a| a.0), Some(3));
        show.launch(1, Transition::Cut, 4.0, Quantize::Bar);
        assert_eq!(show.armed(0.0), None);
        // a launch landing during a crossfade ends it: its incoming graph fades out next
        show.launch(2, CROSSFADE, 4.0, Quantize::Off);
        show.step(0.0, 0.0, 120.0, |_| true);
        show.launch(3, CROSSFADE, 4.0, Quantize::Off);
        show.step(0.1, 0.2, 120.0, |_| true);
        assert_eq!((show.live, show.fade(0.1).map(|f| f.0)), (3, Some(2)));
        // the clock jumping back (a resync) restarts the wait from where it is
        show.launch(4, Transition::Cut, 4.0, Quantize::Bar);
        show.step(1.0, 7.0, 120.0, |_| true);
        show.step(1.1, 0.2, 120.0, |_| true);
        assert_eq!(show.live, 3, "the bar at 8 isn't next any more");
        show.step(3.0, 4.0, 120.0, |_| true);
        assert_eq!(show.live, 4);
    }
}

//! Live pictures from outside, for the Live node: an NDI source, or (Windows) a Spout sender or
//! a camera (a capture card too). A reader thread of its own per source keeps the newest frame,
//! BGRA at the size of the first, in a mailbox the render thread takes from without waiting;
//! the thread ends when nobody holds the feed any more. A source is named `ndi:<name>`,
//! `spout:<name>` or `camera:<name>`.

use crate::media::Media;
use std::cell::{Cell, RefCell};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// How long a source gets to send its first frame.
const FIRST_FRAME: Duration = Duration::from_secs(10);

/// What a reader keeps for the render thread.
pub struct Feed {
    pub size: [u32; 2],
    /// The newest frame, BGRA, rows top first at `size`, and whether it's new since it was taken.
    ready: Mutex<(Vec<u8>, bool)>,
}

impl Feed {
    /// The newest frame, if one came since the last: swapped into `into`, never copied, and
    /// never waited for.
    pub fn take(&self, into: &mut Vec<u8>) -> bool {
        let Ok(mut ready) = self.ready.try_lock() else { return false };
        if !ready.1 {
            return false;
        }
        ready.1 = false;
        std::mem::swap(&mut ready.0, into);
        true
    }

    fn put(&self, frame: &mut Vec<u8>) {
        if let Ok(mut ready) = self.ready.lock() {
            std::mem::swap(&mut ready.0, frame);
            ready.1 = true;
        }
    }
}

/// `frame` (BGRA, `stride` bytes a row, `from` big) as `to` big into `out`: rows copied when
/// the sizes agree, else the nearest pixel (a source that changes size keeps the first's).
fn fit(frame: &[u8], stride: usize, from: [u32; 2], to: [u32; 2], out: &mut Vec<u8>) {
    let ([fw, fh], [w, h]) = ([from[0] as usize, from[1] as usize], [to[0] as usize, to[1] as usize]);
    out.clear();
    out.reserve(w * h * 4);
    if from == to {
        for row in frame.chunks(stride).take(h) {
            out.extend_from_slice(&row[..w * 4]);
        }
        return;
    }
    for y in 0..h {
        let row = &frame[(y * fh / h) * stride..];
        out.extend((0..w).flat_map(|x| row[(x * fw / w) * 4..][..4].iter().copied()));
    }
}

/// A source named `ndi:<name>`, `spout:<name>` or `camera:<name>`: waits for its first frame, which makes the
/// texture's size, then leaves its reader running while the returned media is held.
pub fn open(source: &str) -> Result<Media, String> {
    let (kind, name) = source.split_once(':').ok_or("a live source is ndi:<name> or spout:<name>")?;
    let (kind, name) = (kind.to_string(), name.to_string());
    let (tx, rx) = channel();
    std::thread::Builder::new().name("velo live".into()).spawn(move || read(&kind, &name, tx)).map_err(|e| e.to_string())?;
    let (feed, first) = rx.recv_timeout(FIRST_FRAME).map_err(|_| format!("nothing came from {source} in {} s", FIRST_FRAME.as_secs()))??;
    Ok(Media { size: feed.size, frames: vec![first], ends: vec![1.0], movie: None, video: None, live: Some(feed) })
}

type First = Result<(Arc<Feed>, Vec<u8>), String>;

/// The reader: the source's frames into a feed, the first one also sent to `open`.
fn read(kind: &str, name: &str, first: Sender<First>) {
    let first = RefCell::new(Some(first));
    let feed: RefCell<Option<Weak<Feed>>> = RefCell::new(None);
    // `open` gave up (it dropped its end), or nobody holds the feed any more
    let gone = Cell::new(false);
    let mut spare = Vec::new();
    let started = Instant::now();
    // `open` is long gone when the first frame still hasn't come
    let stop = || gone.get() || feed.borrow().as_ref().is_some_and(|f| f.strong_count() == 0) || (feed.borrow().is_none() && started.elapsed() > FIRST_FRAME + Duration::from_secs(2));
    let each = |size: [u32; 2], pixels: &[u8], stride: usize| {
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        let known = feed.borrow().as_ref().map(Weak::upgrade);
        match known {
            None => {
                let mut frame = Vec::new();
                fit(pixels, stride, size, size, &mut frame);
                let made = Arc::new(Feed { size, ready: Mutex::new((Vec::new(), false)) });
                *feed.borrow_mut() = Some(Arc::downgrade(&made));
                if let Some(tx) = first.borrow_mut().take() {
                    gone.set(tx.send(Ok((made, frame))).is_err());
                }
            }
            Some(Some(live)) => {
                fit(pixels, stride, size, live.size, &mut spare);
                live.put(&mut spare);
            }
            Some(None) => {}
        }
    };
    let done = match kind {
        "ndi" => crate::share::ndi::receive(name, stop, each),
        #[cfg(windows)]
        "spout" => crate::share::spout::receive(name, stop, each),
        #[cfg(windows)]
        "camera" => crate::video::camera(name, stop, each),
        other => Err(format!("{other}: no live source of that kind here")),
    };
    // a failure before the first frame is what `open` tells
    if let (Err(e), Some(tx)) = (done, first.borrow_mut().take()) {
        let _ = tx.send(Err(e));
    }
}

/// The live sources there are, as `kind:name`, from the last look; a new look starts in the
/// background when the last is old, so this never waits (the first call is empty).
pub fn sources() -> Vec<String> {
    static LAST: Mutex<(Option<Instant>, bool, Vec<String>)> = Mutex::new((None, false, Vec::new()));
    let Ok(mut last) = LAST.lock() else { return Vec::new() };
    if !last.1 && last.0.is_none_or(|at| at.elapsed() > Duration::from_secs(4)) {
        last.1 = true;
        let started = std::thread::Builder::new().name("velo live sources".into()).spawn(|| {
            let mut all: Vec<String> = crate::share::ndi::sources(2).unwrap_or_default().into_iter().map(|n| format!("ndi:{n}")).collect();
            #[cfg(windows)]
            all.extend(crate::share::spout::senders().into_iter().map(|n| format!("spout:{n}")));
            #[cfg(windows)]
            all.extend(crate::video::cameras().into_iter().map(|n| format!("camera:{n}")));
            if let Ok(mut last) = LAST.lock() {
                *last = (Some(Instant::now()), false, all);
            }
        });
        if started.is_err() {
            last.1 = false;
        }
    }
    last.2.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_fits_the_size_the_feed_was_made_at() {
        // 2 × 2, a row padded to 12 bytes: red, green / blue, white
        let frame = [[255, 0, 0, 255], [0, 255, 0, 255], [9, 9, 9, 9], [0, 0, 255, 255], [255; 4], [9, 9, 9, 9]].concat();
        let mut out = Vec::new();
        fit(&frame, 12, [2, 2], [2, 2], &mut out);
        assert_eq!(out, [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255; 4]].concat(), "the padding is dropped");
        // bigger: each pixel twice a side; smaller: the first pixel
        fit(&frame, 12, [2, 2], [4, 4], &mut out);
        assert_eq!((out.len(), &out[..8], &out[8..16]), (64, &[[255, 0, 0, 255]; 2].concat()[..], &[[0, 255, 0, 255]; 2].concat()[..]));
        fit(&frame, 12, [2, 2], [1, 1], &mut out);
        assert_eq!(out, [255, 0, 0, 255]);
    }

    #[test]
    fn a_frame_taken_is_not_taken_twice() {
        let feed = Feed { size: [1, 1], ready: Mutex::new((Vec::new(), false)) };
        let (mut mine, mut taken) = (vec![1, 2, 3, 4], Vec::new());
        assert!(!feed.take(&mut taken), "nothing yet");
        feed.put(&mut mine);
        assert!(feed.take(&mut taken) && taken == [1, 2, 3, 4]);
        assert!(!feed.take(&mut taken), "the same frame once");
    }

    /// A sender of our own, opened as a live source: its size and color come through, then
    /// newer frames in the mailbox. Needs the NDI runtime, and the firewall may ask: run by
    /// hand (`--ignored`).
    #[test]
    #[ignore]
    fn an_ndi_sender_comes_through_as_a_live_media() {
        use crate::share::Sender;
        use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
        let Ok(mut sender) = crate::share::ndi::Ndi::new("Velo live test") else {
            return eprintln!("no NDI runtime: skipped");
        };
        let (size, stride) = ([64u32, 32], 256u32);
        let pixels = [40u8, 120, 220, 255].repeat((stride / 4 * size[1]) as usize);
        let done = AtomicBool::new(false);
        let media = std::thread::scope(|s| {
            s.spawn(|| {
                while !done.load(Relaxed) {
                    sender.send(&pixels, size, stride).unwrap();
                    std::thread::sleep(Duration::from_millis(16));
                }
            });
            let media = open("ndi:Velo live test");
            let more = media.as_ref().ok().and_then(|m| m.live.clone()).map(|feed| {
                let (mut frame, start) = (Vec::new(), Instant::now());
                while !feed.take(&mut frame) && start.elapsed() < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                frame
            });
            done.store(true, Relaxed);
            media.map(|m| (m, more))
        });
        let (media, more) = media.expect("our sender is found and its frames come");
        assert_eq!(media.size, size);
        for frame in [&media.frames[0], &more.expect("a feed")] {
            assert_eq!(frame.len(), (size[0] * size[1] * 4) as usize);
            for (a, b) in frame[..4].iter().zip(&pixels[..4]) {
                assert!(a.abs_diff(*b) <= 8, "sent {:?}, got {:?}", &pixels[..4], &frame[..4]);
            }
        }
    }

    /// A Spout sender of our own, opened as a live source: its size and pixels come through,
    /// and a source that isn't running says so. (Only a loopback: real Spout apps are another matter.)
    #[cfg(windows)]
    #[test]
    fn a_spout_sender_comes_through_as_a_live_media() {
        use crate::share::Sender;
        let mut sender = crate::share::spout::Spout::new("Velo live spout");
        let (size, stride) = ([64u32, 32], 256u32);
        // a gradient: blue across, green down
        let pixels: Vec<u8> = (0..size[1]).flat_map(|y| (0..stride / 4).flat_map(move |x| [(x * 4) as u8, (y * 8) as u8, 200, 255])).collect();
        if let Err(e) = sender.send(&pixels, size, stride) {
            return eprintln!("no DirectX 11 here: skipped ({e})");
        }
        let media = open("spout:Velo live spout").expect("our sender is found and its frames come");
        assert_eq!(media.size, [64, 32]);
        assert_eq!(&media.frames[0][..8], &pixels[..8], "the first pixels");
        assert_eq!(&media.frames[0][31 * 256 + 40..31 * 256 + 48], &pixels[31 * 256 + 40..31 * 256 + 48], "a pixel of the last row");
        assert!(open("spout:no such sender").unwrap_err().contains("no Spout sender"));
    }

    #[test]
    fn a_source_with_no_kind_is_an_error_and_a_missing_one_says_so() {
        assert!(open("nothing").is_err());
        assert!(open("projector:x").unwrap_err().contains("no live source"));
    }
}

//! Spike S3: listens to an input and prints what the analysis hears.
//!
//! ```text
//! cargo run -p velo-audio --release --example listen                # computer audio (play Spotify)
//! cargo run -p velo-audio --release --example listen -- --mic
//! cargo run -p velo-audio --release --example listen -- --device "Scarlett"
//! cargo run -p velo-audio --release --example listen -- --synth     # the test loop, scored
//! cargo run -p velo-audio --release --example listen -- --seconds 30
//! cargo run -p velo-audio --release --example listen -- --write loop.wav   # the test loop as a file
//! cargo run -p velo-audio --release --example listen -- --seconds 40 --record take.wav  # what it heard, kept
//! cargo run -p velo-audio --release --example listen -- --file take.wav     # onsets in a recording, as CSV
//! cargo run -p velo-audio --release --example listen -- --file take.wav --model   # the same, from the drum model
//! cargo run -p velo-audio --release --example listen -- --file take.wav --features  # every hop's features, as CSV
//! cargo run -p velo-audio --release --example listen -- --file take.wav --detect 6,20,150,0.1/6,400,5000,0.08/6,6000,20000,0.05 --gate -60
//! ```
//!
//! Simple mode with its defaults unless `--model`.

use std::time::{Duration, Instant};

use velo_audio::{Analyzer, HITS, HOP, Mode, Settings, Source, analysis, capture, synth};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let seconds: f64 = value("--seconds").and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let mut settings = Settings { mode: if has("--model") { Mode::Model } else { Mode::Simple }, ..Default::default() };
    // Simple mode's detectors (threshold dB, low Hz, high Hz, gap s; kick/snare/hat) and gate, to tune them
    if let Some(detect) = value("--detect") {
        for (d, text) in settings.detect.iter_mut().zip(detect.split('/')) {
            let v: Vec<f32> = text.split(',').filter_map(|x| x.parse().ok()).collect();
            if let [threshold_db, low_hz, high_hz, gap_s] = v[..] {
                *d = velo_audio::Detect { threshold_db, low_hz, high_hz, gap_s };
            }
        }
    }
    if let Some(gate) = value("--gate").and_then(|g| g.parse().ok()) {
        settings.gate_db = gate;
    }
    if has("--synth") {
        return scored(seconds as f32, settings);
    }
    if let Some(path) = value("--file") {
        let run = if has("--beats") {
            beats_of
        } else if has("--features") {
            features_of
        } else {
            onsets_of
        };
        return run(&path, settings).unwrap_or_else(|e| eprintln!("{path}: {e}"));
    }
    if let Some(path) = value("--write") {
        let take = synth::techno(48_000.0, 128.0, seconds as f32, 7);
        return write_wav(&path, &take.samples, 48_000).unwrap_or_else(|e| eprintln!("{path}: {e}"));
    }
    let source = if has("--mic") {
        Source::DefaultInput
    } else if let Some(name) = value("--device") {
        Source::Named(name)
    } else {
        Source::Computer
    };
    let mut cap = match capture::open(&source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("couldn't open the input: {e}");
            std::process::exit(1);
        }
    };
    println!("listening to {:?}: {} Hz, {} channels, for {seconds} s (hop {:.1} ms)", cap.name, cap.rate, cap.channels, HOP as f32 / cap.rate * 1000.0);
    let mut analyzer = Analyzer::with(cap.rate, settings);
    let mut kicks: Vec<f64> = Vec::new();
    let mut counts = [0usize; 3];
    let mut loudest = f32::NEG_INFINITY;
    let mut buffer = vec![0.0f32; 4096];
    let record = value("--record");
    let mut kept: Vec<f32> = Vec::new();
    let (start, mut next_line) = (Instant::now(), 0.5);
    let (mut worst_backlog, mut received) = (0usize, 0usize);
    while start.elapsed().as_secs_f64() < seconds {
        let ready = cap.samples.slots();
        worst_backlog = worst_backlog.max(ready);
        let take = ready.min(buffer.len());
        received += take;
        if take == 0 {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        let Ok(chunk) = cap.samples.read_chunk(take) else { continue };
        let (a, b) = chunk.as_slices();
        buffer[..a.len()].copy_from_slice(a);
        buffer[a.len()..a.len() + b.len()].copy_from_slice(b);
        chunk.commit_all();
        if record.is_some() {
            kept.extend_from_slice(&buffer[..take]);
        }
        analyzer.push(&buffer[..take], |hop| {
            loudest = loudest.max(hop.level_db);
            for (band, onset) in hop.onsets.iter().enumerate() {
                if onset.is_some() {
                    counts[band] += 1;
                }
            }
            if hop.onsets[0].is_some() {
                kicks.push(hop.time);
            }
            if hop.time >= next_line {
                next_line += 0.5;
                let hits: String = hop.onsets.iter().zip(["K", "S", "H"]).map(|(o, c)| if o.is_some() { c } else { "·" }).collect();
                println!("{:6.1} s  level {:6.1} dB  low {:6.1}  mid {:6.1}  high {:6.1}  {hits}  kicks so far {}", hop.time, hop.level_db, hop.bands_db[0], hop.bands_db[1], hop.bands_db[2], kicks.len());
            }
        });
    }
    println!();
    if let Some(path) = &record {
        match write_wav(path, &kept, cap.rate as u32) {
            Ok(()) => println!("kept {:.1} s in {path}", kept.len() as f32 / cap.rate),
            Err(e) => eprintln!("{path}: {e}"),
        }
    }
    if received == 0 {
        println!("no audio arrived at all: the input never started delivering");
        println!("macOS: after granting a permission, quit and reopen the app running this (Cmd+Q); a pending prompt also holds it");
        return;
    }
    if loudest < -60.0 {
        println!("silence (loudest hop {loudest:.0} dB): nothing playing, or no permission to record system audio");
        println!("macOS: System Settings → Privacy & Security → Screen & System Audio Recording → allow the app running this");
        return;
    }
    for (band, n) in counts.iter().enumerate() {
        println!("{:>5}s: {n}", HITS[band]);
    }
    tempo(&kicks);
    println!("worst backlog between reads: {:.1} ms of audio", worst_backlog as f32 / cap.rate * 1000.0);
}

/// From the kick intervals: the tempo they imply and how steady they are.
fn tempo(kicks: &[f64]) {
    let mut gaps: Vec<f64> = kicks.windows(2).map(|w| w[1] - w[0]).filter(|g| (0.25..1.5).contains(g)).collect();
    if gaps.len() < 8 {
        println!("too few kicks for a tempo");
        return;
    }
    gaps.sort_by(f64::total_cmp);
    let beat = gaps[gaps.len() / 2];
    let steady = gaps.iter().filter(|g| (*g - beat).abs() < 0.015).count() as f64 / gaps.len() as f64;
    println!("kicks imply {:.1} BPM; {:.0} % of gaps within 15 ms of that beat", 60.0 / beat, steady * 100.0);
}

/// The synthetic loop through the analyzer, scored against its known hits.
fn scored(seconds: f32, settings: Settings) {
    for rate in [44_100.0, 48_000.0] {
        let take = synth::techno(rate, 128.0, seconds, 7);
        let started = Instant::now();
        let found = analysis::onsets(&take.samples, rate, settings);
        let cost = started.elapsed().as_secs_f64() / f64::from(seconds);
        println!("{rate} Hz: analysis costs {:.3} % of realtime on one core", cost * 100.0);
        for (band, truth) in [(0, &take.kicks), (1, &take.snares), (2, &take.hats)] {
            let (p, r, f, delay) = analysis::score(truth, &found[band], 0.03);
            println!("  {:>4} vs {:<6} precision {p:.2}  recall {r:.2}  F {f:.2}  delay {:.1} ms", HITS[band], ["kicks", "snares", "hats"][band], delay * 1000.0);
        }
    }
}

/// A recording's onsets, one line each: seconds, hit, the signal it fired at (the band's rise in
/// dB, or the model's probability).
fn onsets_of(path: &str, settings: Settings) -> std::io::Result<()> {
    let (samples, rate) = read_wav(path)?;
    println!("time,hit,signal");
    let mut analyzer = Analyzer::with(rate as f32, settings);
    analyzer.push(&samples, |hop| {
        for (band, onset) in hop.onsets.iter().enumerate() {
            if let Some(signal) = onset {
                println!("{:.3},{},{signal:.2}", hop.heard - HOP as f64 / f64::from(rate), HITS[band]);
            }
        }
    });
    Ok(())
}

/// A recording's features hop by hop, as mappings read them: seconds, then each 0..1 value.
fn features_of(path: &str, settings: Settings) -> std::io::Result<()> {
    let (samples, rate) = read_wav(path)?;
    println!("time,level,bass,mid,high,kick,snare,hat,melody");
    let (mut analyzer, mut tracker) = (Analyzer::with(rate as f32, settings), velo_audio::Tracker::new());
    let dt = HOP as f32 / rate as f32;
    analyzer.push(&samples, |hop| {
        tracker.hop(hop, dt);
        let f = tracker.features();
        println!("{:.4},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}", hop.heard, f.level, f.bass, f.mid, f.high, f.kick, f.snare, f.hat, f.melody);
    });
    Ok(())
}

/// A recording's beats as the live tracker predicts them: from each estimate, the beats due until
/// the next one (seconds), with the tempo and how sure it was.
fn beats_of(path: &str, settings: Settings) -> std::io::Result<()> {
    let (samples, rate) = read_wav(path)?;
    println!("time,bpm,confidence");
    let (mut analyzer, mut tempo) = (Analyzer::with(rate as f32, settings), velo_audio::Tempo::new(rate as f32));
    let mut last: Option<velo_audio::Estimate> = None;
    analyzer.push(&samples, |hop| {
        if let Some(e) = tempo.push(hop.strength, hop.heard) {
            last = Some(e);
        }
        // a beat due in this hop, by the latest estimate
        if let Some(e) = last {
            let period = 60.0 / f64::from(e.bpm);
            let hop_s = HOP as f64 / f64::from(rate);
            let k = ((hop.time - e.beat_at) / period).floor();
            let due = e.beat_at + k * period;
            if due > hop.time - hop_s && due <= hop.time {
                println!("{due:.3},{:.2},{:.2}", e.bpm, e.confidence);
            }
        }
    });
    Ok(())
}

/// A WAV as written here: 16-bit PCM, mono (several channels are mixed down).
fn read_wav(path: &str) -> std::io::Result<(Vec<f32>, u32)> {
    let bytes = std::fs::read(path)?;
    let bad = || std::io::Error::other("not a 16-bit PCM WAV");
    let u16_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(bad());
    }
    // the chunks after the header: fmt, then data (others skipped)
    let (mut at, mut format, mut data) = (12, None, None);
    while let (Some(id), Some(len)) = (bytes.get(at..at + 4), u32_at(at + 4)) {
        let body = at + 8;
        match id {
            b"fmt " => format = Some((u16_at(body).ok_or_else(bad)?, u16_at(body + 2).ok_or_else(bad)?, u32_at(body + 4).ok_or_else(bad)?, u16_at(body + 14).ok_or_else(bad)?)),
            b"data" => data = bytes.get(body..(body + len as usize).min(bytes.len())),
            _ => {}
        }
        at = body + len as usize + (len as usize & 1);
    }
    let (Some((1, channels, rate, 16)), Some(data)) = (format, data) else { return Err(bad()) };
    let channels = usize::from(channels.max(1));
    let samples = data.chunks_exact(2 * channels).map(|frame| frame.as_chunks::<2>().0.iter().map(|s| f32::from(i16::from_le_bytes(*s)) / 32768.0).sum::<f32>() / channels as f32).collect();
    Ok((samples, rate))
}

/// 16-bit mono WAV, so the test loop can be played through the speakers and captured back.
fn write_wav(path: &str, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (1 << 16), rate, rate * 2, 2 | (16 << 16)] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out)
}

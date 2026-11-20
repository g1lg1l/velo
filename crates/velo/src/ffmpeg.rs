//! ffmpeg for recording when the computer has none: Velo asks first, then fetches a build of it
//! into its own data folder. Never bundled (it's a separate program, under its own licence) and
//! never fetched unasked. Each OS gets one pinned build, checked against the SHA-256 its
//! publisher lists before anything of it is kept; it has to run before it's put in place.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use eframe::egui::{self, RichText};

use crate::shell::Tone;
use crate::{App, files, icons, kit};

/// A build of ffmpeg to fetch: the zip, its SHA-256, about how big (said before asking), whose.
pub struct Build {
    url: &'static str,
    sha256: &'static str,
    mb: u32,
    from: &'static str,
}

/// This computer's build, if there's one to offer (else: its package manager's).
pub const BUILD: Option<Build> = match (std::env::consts::OS.as_bytes(), std::env::consts::ARCH.as_bytes()) {
    // Martin Riedl's static builds: signed, and zipped as the one program
    (b"macos", b"aarch64") => {
        Some(Build { url: "https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip", sha256: "c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924", mb: 28, from: "ffmpeg.martin-riedl.de" })
    }
    (b"linux", b"x86_64") => {
        Some(Build { url: "https://ffmpeg.martin-riedl.de/download/linux/amd64/1789931100_9.0.2/ffmpeg.zip", sha256: "fa8ecf4abbd290d98f7d188b8649cc6b391ae209a98452be955a15aab1909d7f", mb: 33, from: "ffmpeg.martin-riedl.de" })
    }
    // gyan.dev's essentials build, the one ffmpeg.org links for Windows, from its GitHub releases
    (b"windows", b"x86_64") => {
        Some(Build { url: "https://github.com/GyanD/codexffmpeg/releases/download/9.1/ffmpeg-9.1-essentials_build.zip", sha256: "b9bbbb3acece000cbeec994f856649354e52c773807841e78908374b24d658fd", mb: 119, from: "github.com/GyanD" })
    }
    _ => None,
};

/// The program, as this OS names it.
const NAME: &str = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

/// Where a fetched ffmpeg is kept: Velo's data folder.
fn own() -> Option<PathBuf> {
    Some(files::data_dir()?.join("ffmpeg").join(NAME))
}

/// The ffmpeg to record with: one installed on the computer, else the one Velo fetched.
pub fn find() -> Option<PathBuf> {
    velo_engine::record::ffmpeg().or_else(|| own().filter(|p| p.is_file()))
}

/// Where Velo is with ffmpeg: nothing asked, asking, or fetching it.
#[derive(Default)]
pub enum Fetch {
    #[default]
    Idle,
    Asking,
    Fetching(Receiver<Result<PathBuf, String>>),
}

/// The zip's SHA-256 is the one listed: a download cut short, or swapped, is never unpacked.
fn check(bytes: &[u8], sha256: &str) -> Result<(), String> {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if hex == sha256 { Ok(()) } else { Err(tr!("the download isn't the expected one (its checksum differs)").to_string()) }
}

/// The checked zip's ffmpeg put at `to`: unpacked beside it, made runnable, run once, then moved
/// in, so a half-written or broken one is never found.
fn unpack(bytes: &[u8], sha256: &str, to: &Path) -> Result<(), String> {
    check(bytes, sha256)?;
    let dir = to.parent().ok_or("no folder for ffmpeg")?;
    let staging = dir.join("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        crate::update::unzip_file(bytes, NAME, &staging)?;
        let new = staging.join(NAME);
        if !new.is_file() {
            return Err(tr!("the download has no ffmpeg in it").to_string());
        }
        #[cfg(unix)]
        std::fs::set_permissions(&new, std::os::unix::fs::PermissionsExt::from_mode(0o755)).map_err(|e| e.to_string())?;
        let mut run = std::process::Command::new(&new);
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut run, 0x0800_0000); // CREATE_NO_WINDOW
        if !run.arg("-version").output().is_ok_and(|o| o.status.success()) {
            return Err(tr!("the ffmpeg downloaded doesn't run on this computer").to_string());
        }
        std::fs::rename(&new, to).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Fetches `build` on a thread of its own; the path to it when it's in, or why not.
fn fetch(build: &'static Build) -> Receiver<Result<PathBuf, String>> {
    let (to, from) = channel();
    std::thread::spawn(move || {
        let got = (|| {
            let path = own().ok_or("no data folder")?;
            std::fs::create_dir_all(path.parent().ok_or("no data folder")?).map_err(|e| e.to_string())?;
            unpack(&crate::update::get(build.url)?, build.sha256, &path)?;
            Ok(path)
        })();
        let _ = to.send(got);
    });
    from
}

impl App {
    /// Recording was asked for and there's no ffmpeg: offer to fetch it (or say how to get it).
    pub fn offer_ffmpeg(&mut self) {
        match (&self.ffmpeg, &BUILD) {
            (Fetch::Fetching(_), _) => self.say(Tone::Action, tr!("Still downloading ffmpeg…")),
            (_, Some(_)) => self.ffmpeg = Fetch::Asking,
            (_, None) => self.say(Tone::Error, tr!("Recording needs ffmpeg: install it with your system's package manager")),
        }
    }

    /// The question, while it's asked; then the download, until it's in: recording starts then.
    pub fn ffmpeg_modal(&mut self, ctx: &egui::Context) {
        let got = match &self.ffmpeg {
            Fetch::Idle => return,
            Fetch::Fetching(from) => match from.try_recv() {
                Ok(got) => Some(got),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                    return;
                }
                Err(TryRecvError::Disconnected) => Some(Err(tr!("the download stopped").to_string())),
            },
            Fetch::Asking => None,
        };
        if let Some(got) = got {
            self.ffmpeg = Fetch::Idle;
            match got {
                Ok(_) if self.recording.is_none() => self.toggle_record(),
                Ok(_) => {}
                Err(e) => self.say(Tone::Error, tr!("ffmpeg didn't download: {}", e)),
            }
            return;
        }
        let Some(build) = &BUILD else { return };
        let t = self.theme.clone();
        let (choice, closed) = kit::modal(ctx, &t, "ffmpeg", tr!("Recording needs ffmpeg"), 400.0, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.label(RichText::new(tr!("Velo records through ffmpeg, a free program that isn't on this computer.")).strong().color(t.fg_text));
            ui.label(RichText::new(tr!("Download it now? About {} MB from {}, kept in Velo's own folder: nothing else is installed.", build.mb, build.from)).color(t.fg_muted));
            ui.add_space(t.gap);
            let shown = kit::actions(ui, &t, &[(kit::Variant::Ghost, Some(icons::X), tr!("Cancel")), (kit::Variant::Primary, Some(icons::DOWNLOAD_SIMPLE), tr!("Download and record"))]);
            shown.iter().position(egui::Response::clicked)
        });
        match if closed { Some(0) } else { choice } {
            Some(1) => {
                self.ffmpeg = Fetch::Fetching(fetch(build));
                self.say(Tone::Action, tr!("Downloading ffmpeg ({} MB)…", build.mb));
            }
            Some(_) => self.ffmpeg = Fetch::Idle,
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_is_sha256_in_hex() {
        assert!(check(b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_ok());
        assert!(check(b"abd", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_err());
        for build in BUILD.iter() {
            assert!(build.sha256.len() == 64 && build.sha256.bytes().all(|b| b.is_ascii_hexdigit()) && build.url.starts_with("https://"));
        }
    }

    /// A zip with "ffmpeg" in a folder (a script that runs): kept only with the right checksum,
    /// runnable; nothing left behind otherwise.
    #[cfg(unix)]
    #[test]
    fn only_a_checked_ffmpeg_that_runs_is_put_in_place() {
        let dir = std::env::temp_dir().join(format!("velo-ffmpeg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip = |script: &[u8]| {
            let mut bytes = std::io::Cursor::new(Vec::new());
            let mut zip = zip::ZipWriter::new(&mut bytes);
            zip.start_file("ffmpeg-9/bin/ffmpeg", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, script).unwrap();
            zip.finish().unwrap();
            bytes.into_inner()
        };
        let sum = |bytes: &[u8]| ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>();
        let to = dir.join("ffmpeg");
        let good = zip(b"#!/bin/sh\nexit 0\n");
        assert!(unpack(&good, &"0".repeat(64), &to).is_err() && !to.exists(), "a wrong checksum: nothing kept");
        let broken = zip(b"#!/bin/sh\nexit 1\n");
        assert!(unpack(&broken, &sum(&broken), &to).is_err() && !to.exists(), "one that doesn't run: nothing kept");
        unpack(&good, &sum(&good), &to).unwrap();
        assert!(std::process::Command::new(&to).arg("-version").status().unwrap().success());
        assert!(!dir.join("staging").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

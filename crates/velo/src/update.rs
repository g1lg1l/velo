//! The update check and self-update.
//! From Ravune (03cb268), trimmed for Velo: no extensions or gallery, one program.
//!
//! Velo asks GitHub's feed of its releases for the newest one, only when asked or (if that is
//! on) once at start; nothing else is sent. A copy built by the release workflow (`VELO_RELEASE`)
//! updates itself from the release's files: it swaps its program (on macOS the whole Velo.app)
//! for the new one in place. A running program can be renamed, so the old one becomes `*.old`,
//! cleared next start. A copy installed for all users on Windows (Program Files, not writable)
//! runs the new installer instead; builds from source, and copies in folders Velo can't write
//! to, get a link to the release page.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use eframe::egui::{self, RichText, Ui};

use crate::shell::Tone;
use crate::theme::Theme;
use crate::{App, icons, kit};

/// Velo's releases, betas included: GitHub's feed of them. Its API would do too, but allows 60
/// calls an hour to everyone behind one address (an office or a venue shares them).
const RELEASES: &str = "https://github.com/g1lg1l/velo/releases.atom";
/// Where a release's files are, by its tag (the release workflow names them).
const DOWNLOADS: &str = "https://github.com/g1lg1l/velo/releases/download";
/// What a download may be.
const MAX_BYTES: u64 = 256 << 20;

fn get(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        // an update is tens of MB: minutes on a slow line, but a dead one fails fast
        .timeout_global(Some(Duration::from_secs(600)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .user_agent(concat!("Velo/", env!("CARGO_PKG_VERSION"), " (+https://github.com/g1lg1l/velo)"))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| e.to_string())
}

/// The program a release ships, as this OS names it.
fn binary() -> String {
    format!("velo{}", std::env::consts::EXE_SUFFIX)
}

/// What an update replaces: (its name in the release's archive, where it is now). macOS replaces
/// the whole Velo.app, the others the program.
fn replaced(exe: &Path) -> Option<(String, PathBuf)> {
    if cfg!(target_os = "macos") {
        // …/Velo.app/Contents/MacOS/velo
        let app = exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|e| e == "app"))?;
        Some(("Velo.app".to_string(), app.to_path_buf()))
    } else {
        Some((binary(), exe.to_path_buf()))
    }
}

/// Whether Velo may write into `dir` (not Program Files, a mounted disk image…).
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".velo-probe");
    std::fs::write(&probe, b"").is_ok() && std::fs::remove_file(&probe).is_ok()
}

/// How this copy updates: the end of the release file's name it takes. None: it doesn't (a build
/// from source, or a folder it can't write to and no installer to fall back on).
fn asset_suffix(exe: &Path) -> Option<&'static str> {
    option_env!("VELO_RELEASE")?;
    let folder = replaced(exe)?.1.parent()?.to_path_buf();
    match std::env::consts::OS {
        _ if !writable(&folder) => (cfg!(windows) && exe.with_file_name("unins000.exe").exists()).then_some("-windows-x64-setup.exe"),
        "windows" => Some("-windows-x64.zip"),
        "linux" => Some("-linux-x64.tar.gz"),
        "macos" => Some("-macos-arm64.tar.gz"),
        _ => None,
    }
}

/// The program in a release zip, wherever it sits in it, written into `to`. Nothing else, and
/// nothing from outside the zip's own folders.
fn unzip_binary(bytes: &[u8], to: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let name = binary();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        if file.enclosed_name().and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned())).is_none_or(|n| n != name) {
            continue;
        }
        let mut out = std::fs::File::create(to.join(&name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Puts what `staging` holds in place of `replaced`. The old one is renamed (that works while it
/// runs) to `<name>.<pid>.old`, and comes back if the new one can't go in.
fn swap(staging: &Path, (name, to): &(String, PathBuf)) -> std::io::Result<()> {
    let new = staging.join(name);
    if !new.exists() {
        return Err(std::io::Error::other(tr!("the download has no Velo in it")));
    }
    let old = to.with_file_name(format!("{}.{}.old", to.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    if to.exists() {
        std::fs::rename(to, &old)?;
    }
    if let Err(e) = std::fs::rename(&new, to) {
        let _ = std::fs::rename(&old, to);
        return Err(e);
    }
    Ok(())
}

/// Clears what earlier updates left: old copies and an unpacked download.
fn tidy((_, path): &(String, PathBuf)) {
    let Some(folder) = path.parent() else { return };
    let prefix = format!("{}.", path.file_name().unwrap_or_default().to_string_lossy());
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".old") && name.starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path()).or_else(|_| std::fs::remove_dir_all(entry.path()));
        }
    }
    let _ = std::fs::remove_dir_all(folder.join(".velo-update"));
}

/// An update, downloaded and ready.
pub struct Staged {
    /// Where Velo is (found before the swap: on Linux the running one reads "(deleted)" after).
    exe: PathBuf,
    /// A copy installed for all users (Program Files): the new installer, run when Velo
    /// restarts. None: the update is already in place.
    setup: Option<PathBuf>,
}

/// Downloads `url` (a file of the release) and puts it in place of the Velo at `exe`.
fn stage(url: &str, exe: PathBuf) -> Result<Staged, String> {
    let bytes = get(url)?;
    if url.ends_with(".exe") {
        let setup = std::env::temp_dir().join("velo-setup.exe");
        std::fs::write(&setup, bytes).map_err(|e| e.to_string())?;
        return Ok(Staged { exe, setup: Some(setup) });
    }
    let replaced = replaced(&exe).ok_or("Velo isn't where an update can find it")?;
    // unpacked beside what it replaces, not in the temp folder: a rename can't cross drives
    let staging = replaced.1.with_file_name(".velo-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    if url.ends_with(".zip") {
        unzip_binary(&bytes, &staging)?;
    } else {
        // tar keeps the Unix permissions and the Mac bundle; every Linux and macOS has it
        let tarball = staging.join("update.tar.gz");
        std::fs::write(&tarball, bytes).map_err(|e| e.to_string())?;
        let ok = std::process::Command::new("tar").arg("-xzf").arg(&tarball).arg("-C").arg(&staging).arg("--strip-components=1").status().is_ok_and(|s| s.success());
        if !ok {
            return Err(tr!("couldn't unpack the download").to_string());
        }
    }
    let swapped = swap(&staging, &replaced).map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&staging);
    swapped.map(|()| Staged { exe, setup: None })
}

/// Whether release tag `tag` ("v0.2.0") is newer than `current` ("0.1.0").
/// A pre-release ("0.2.0-beta", "0.1.0-dev") comes before its release ("0.2.0").
fn newer(tag: &str, current: &str) -> bool {
    let parse = |v: &str| {
        let v = v.trim_start_matches('v');
        let (numbers, pre) = v.split_once('-').map_or((v, false), |(n, _)| (n, true));
        (numbers.split('.').map(|n| n.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>(), !pre)
    };
    parse(tag) > parse(current)
}

pub struct Release {
    tag: String,
    page: String,
    /// The file this copy updates from (see [`asset_suffix`]).
    download: Option<String>,
}

/// The newest release in the feed (by its version, whatever the feed's order). `suffix`: the
/// file this copy updates from, `velo-<version><suffix>` in that release.
fn latest(body: &[u8], suffix: Option<&str>) -> Result<Release, String> {
    let text = std::str::from_utf8(body).map_err(|e| e.to_string())?;
    let feed = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let pages = feed.descendants().filter(|n| n.has_tag_name("entry")).filter_map(|e| e.children().find(|c| c.has_tag_name("link"))?.attribute("href"));
    let (tag, page) = pages.filter_map(|page| Some((page.rsplit_once("/releases/tag/")?.1.to_string(), page.to_string()))).reduce(|a, b| if newer(&b.0, &a.0) { b } else { a }).ok_or("no releases yet")?;
    let download = suffix.map(|s| format!("{DOWNLOADS}/{tag}/velo-{}{s}", tag.trim_start_matches('v')));
    Ok(Release { tag, page, download })
}

enum Reply {
    Release(Result<Release, String>),
    Update(Result<Staged, String>),
}

/// What the update check is waiting for and has heard.
#[derive(Default)]
pub struct Updates {
    waiting: Vec<Receiver<Reply>>,
    /// The latest release, or why it couldn't be read.
    release: Option<Result<Release, String>>,
    checking: bool,
    /// The new version on its way (None), ready, or why it isn't.
    update: Option<Option<Result<Staged, String>>>,
    /// Restart into the update: asked for, Velo closes and [`App::finish_update`] opens it.
    pub restart: bool,
    started: bool,
}

impl Updates {
    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Reply + Send + 'static) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
        self.waiting.push(rx);
    }
}

impl App {
    /// Hands over what came back from GitHub; at the first frame, the check if it's on.
    pub fn poll_updates(&mut self, ctx: &egui::Context) {
        if !std::mem::replace(&mut self.updates.started, true) {
            if let Some(r) = std::env::current_exe().ok().as_deref().and_then(replaced) {
                tidy(&r);
            }
            // capture runs (docs screenshots) stay offline
            if !self.settings.no_update_check && std::env::var_os("VELO_CAPTURE_TO").is_none() {
                self.check_updates(ctx);
            }
        }
        if self.updates.restart {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let mut replies = Vec::new();
        self.updates.waiting.retain(|rx| match rx.try_recv() {
            Ok(reply) => {
                replies.push(reply);
                false
            }
            Err(e) => e == std::sync::mpsc::TryRecvError::Empty,
        });
        for reply in replies {
            match reply {
                Reply::Release(release) => {
                    self.updates.checking = false;
                    if let Ok(r) = &release
                        && newer(&r.tag, env!("CARGO_PKG_VERSION"))
                        && self.updates.update.is_none()
                    {
                        self.say(Tone::Action, tr!("Velo {} is available: Preferences → Updates", r.tag));
                    }
                    self.updates.release = Some(release);
                }
                Reply::Update(staged) => {
                    match &staged {
                        Ok(_) => self.say(Tone::Done, tr!("The update is ready: restart Velo (Preferences → Updates)")),
                        Err(e) => self.say(Tone::Error, tr!("Couldn't update: {}", e)),
                    }
                    self.updates.update = Some(Some(staged));
                }
            }
        }
    }

    /// Asks GitHub for the latest release.
    pub fn check_updates(&mut self, ctx: &egui::Context) {
        self.updates.checking = true;
        self.updates.spawn(ctx, || {
            let suffix = std::env::current_exe().ok().and_then(|exe| asset_suffix(&exe));
            Reply::Release(get(RELEASES).and_then(|bytes| latest(&bytes, suffix)))
        });
    }

    /// At exit, after a restart was asked for: opens the new Velo, or for a copy installed for
    /// all users runs the installer first (cmd waits for it while it asks for admin rights).
    pub fn finish_update(&self) {
        let Some(Some(Ok(staged))) = &self.updates.update else { return };
        if !self.updates.restart {
            return;
        }
        let Some(setup) = &staged.setup else {
            let _ = std::process::Command::new(&staged.exe).args(&self.file).spawn();
            return;
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let open = self.file.as_ref().map_or(String::new(), |f| format!(" \"{}\"", f.display()));
            let line = format!("/c start \"\" /wait \"{}\" /SILENT /SUPPRESSMSGBOXES /NORESTART /ALLUSERS && start \"\" \"{}\"{open}", setup.display(), staged.exe.display());
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let _ = std::process::Command::new("cmd").raw_arg(line).creation_flags(CREATE_NO_WINDOW).spawn();
        }
        #[cfg(not(windows))]
        let _ = setup;
    }

    /// Preferences → Updates.
    pub fn prefs_updates(&mut self, ui: &mut Ui, t: &Theme) {
        let version = env!("CARGO_PKG_VERSION");
        kit::card(ui, t, tr!("UPDATES"), |ui| {
            ui.spacing_mut().item_spacing.y = t.gap + 2.0;
            let mut on = !self.settings.no_update_check;
            let detail = tr!("Velo {} asks GitHub for the latest release. Nothing else is sent.", version);
            if kit::setting(ui, t, tr!("Check when Velo starts"), &detail, |ui| kit::switch(ui, t, &mut on, tr!("Check when Velo starts"))).changed() {
                self.settings.no_update_check = !on;
            }
            kit::divider(ui, t);
            ui.horizontal(|ui| {
                if kit::button(ui, t, Some(icons::ARROW_CLOCKWISE), tr!("Check now"), false).clicked() && !self.updates.checking {
                    self.check_updates(ui.ctx());
                }
                if self.updates.checking {
                    ui.spinner();
                }
            });
            let (mut restart, mut download) = (false, None);
            let muted = |text: String| RichText::new(text).small().color(t.fg_muted);
            match (&self.updates.release, &self.updates.update) {
                (Some(Ok(r)), Some(None)) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(muted(tr!("Downloading Velo {}…", r.tag)));
                    });
                }
                (Some(Ok(r)), Some(Some(Ok(staged)))) => {
                    let what = if staged.setup.is_some() { tr!("Velo {} is downloaded: it installs when Velo restarts", r.tag) } else { tr!("Velo {} is installed: it opens next time", r.tag) };
                    ui.label(RichText::new(what).small().color(t.accent));
                    restart = kit::button(ui, t, Some(icons::ARROW_CLOCKWISE), tr!("Restart now"), false).clicked();
                }
                (Some(Ok(r)), Some(Some(Err(e)))) => {
                    ui.label(RichText::new(tr!("Couldn't update: {}", e)).small().color(t.error));
                    ui.hyperlink_to(tr!("Velo {} is available: get it on GitHub", r.tag), &r.page);
                }
                (Some(Ok(r)), None) if newer(&r.tag, version) => match &r.download {
                    Some(url) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tr!("Velo {} is available", r.tag)).small().color(t.accent));
                            if kit::button(ui, t, Some(icons::DOWNLOAD_SIMPLE), tr!("Update"), false).clicked() {
                                download = Some(url.clone());
                            }
                            ui.hyperlink_to(RichText::new(tr!("What's new")).small(), &r.page);
                        });
                    }
                    None => {
                        ui.hyperlink_to(tr!("Velo {} is available: get it on GitHub", r.tag), &r.page);
                    }
                },
                (Some(Ok(_)), _) => {
                    ui.label(muted(tr!("Velo {} is the latest", version)));
                }
                (Some(Err(e)), _) => {
                    ui.label(RichText::new(tr!("Couldn't check: {}", e)).small().color(t.error));
                }
                (None, _) => {}
            }
            ui.add_space(t.pad);
            if restart {
                self.request(crate::files::Pending::Restart);
            }
            // one download per run: a second swap would trip over the first's *.old
            if let Some(url) = download {
                self.updates.update = Some(None);
                self.updates.spawn(ui.ctx(), move || Reply::Update(std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| stage(&url, exe))));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(newer("v0.2.0", "0.1.0") && newer("v1.0.0", "0.12.3") && newer("0.1.1", "0.1.0"));
        assert!(!newer("v0.1.0", "0.1.0") && !newer("v0.0.9", "0.1.0") && !newer("", "0.1.0"));
        // this build is 0.1.0-dev: the 0.1.0 release is newer, a 0.1.0 beta isn't
        assert!(newer("v0.1.0", "0.1.0-dev") && newer("v0.1.1-beta", "0.1.0") && !newer("v0.1.0-beta", "0.1.0-dev"));
    }

    #[test]
    fn the_feed_gives_the_newest_release_and_this_platforms_file() {
        // GitHub's feed, trimmed: an older release listed first, as an edit can put it there
        let entry = |tag: &str| format!(r#"<entry><id>tag:github.com,2008:Repository/1/{tag}</id><link rel="alternate" type="text/html" href="https://github.com/g1lg1l/velo/releases/tag/{tag}"/><title>{tag}</title></entry>"#);
        let feed = |entries: &str| format!(r#"<?xml version="1.0" encoding="UTF-8"?><feed xmlns="http://www.w3.org/2005/Atom"><title>Release notes from velo</title>{entries}</feed>"#);
        let body = feed(&(entry("v0.1.0") + &entry("v0.2.0-beta") + &entry("v0.1.1")));
        let r = latest(body.as_bytes(), Some("-macos-arm64.tar.gz")).unwrap();
        assert_eq!((r.tag.as_str(), r.page.as_str()), ("v0.2.0-beta", "https://github.com/g1lg1l/velo/releases/tag/v0.2.0-beta"));
        assert_eq!(r.download.as_deref(), Some("https://github.com/g1lg1l/velo/releases/download/v0.2.0-beta/velo-0.2.0-beta-macos-arm64.tar.gz"));
        assert!(latest(body.as_bytes(), None).unwrap().download.is_none());
        assert_eq!(latest(feed("").as_bytes(), None).err().as_deref(), Some("no releases yet"));
    }

    #[test]
    fn an_update_swaps_the_program_and_tidies_after() {
        let root = std::env::temp_dir().join(format!("velo-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let staging = root.join(".velo-update");
        std::fs::create_dir_all(&staging).unwrap();
        let name = binary();
        let target = (name.clone(), root.join(&name));
        std::fs::write(&target.1, "old").unwrap();
        assert!(swap(&staging, &target).is_err(), "nothing unpacked: nothing swapped");
        // a zip with the program in a folder, a README, and a path trying to escape
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut bytes);
            for (path, data) in [(format!("velo-9.0.0-windows-x64/{name}"), b"new".as_slice()), ("velo-9.0.0-windows-x64/README.md".into(), b"#"), (format!("../evil-{name}"), b"evil")] {
                zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
                std::io::Write::write_all(&mut zip, data).unwrap();
            }
            zip.finish().unwrap();
        }
        unzip_binary(bytes.get_ref(), &staging).unwrap();
        assert!(!staging.join("README.md").exists() && !root.join(format!("evil-{name}")).exists(), "only the program, only from inside the zip");
        swap(&staging, &target).unwrap();
        let read = |path: &Path| std::fs::read_to_string(path).unwrap();
        let old = root.join(format!("{name}.{}.old", std::process::id()));
        assert_eq!((read(&target.1).as_str(), read(&old).as_str()), ("new", "old"));
        tidy(&target);
        assert!(!old.exists() && !staging.exists() && target.1.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

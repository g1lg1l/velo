//! Project files, autosave and crash recovery. From Ravune (bd10814), trimmed for Velo.
//! A project is one file (docs/project-format.md §2): `Show.velo`, a zip of `project.ron` and each
//! Shader node's code in `shaders/`; exported to share, the files its nodes show too, in `media/`.
//! A bundle, the same as a folder `Show.velo/`, still opens and saves as one, and so does a plain
//! RON file with the code inline (older projects, the autosave).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, RichText};
use serde_json::json;
use velo_core::{FORMAT, Node, Project, Session, is_shader};

use crate::shell::Tone;
use crate::{App, kit};

pub const EXTENSION: &str = "velo";
/// In a bundle: the document, and the folder of shader files.
const PROJECT: &str = "project.ron";
const SHADERS: &str = "shaders";
/// In a project exported to share: the files its nodes show (pictures, movies, fonts).
const MEDIA: &str = "media";
const AUTOSAVE_SECONDS: f64 = 60.0;
const AUTOSAVE: &str = "autosave.velo";
/// There while Velo runs: found at start, the last session didn't close cleanly.
const LOCK: &str = "session.lock";

/// Shader files being watched for an outside editor's saves.
#[derive(Default)]
pub struct Watch {
    /// Each file's modification time when last looked at; cleared on open and save.
    pub seen: HashMap<String, SystemTime>,
    last: f64,
}

/// Opens `path` in the outside editor: `editor` (an app's name on macOS, a command elsewhere)
/// or the system's app for it. On macOS a type with no app falls back to the text editor.
fn launch(editor: &str, path: &Path) -> Result<(), String> {
    use std::process::Command;
    let editor = editor.trim();
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        if !editor.is_empty() {
            c.args(["-a", editor]);
        }
        c
    } else if !editor.is_empty() {
        let mut words = editor.split_whitespace();
        let mut c = Command::new(words.next().unwrap_or(editor));
        c.args(words);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        Command::new("xdg-open")
    };
    let mut child = command.arg(path).spawn().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") && editor.is_empty() {
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            if !child.wait().is_ok_and(|s| s.success()) {
                let _ = Command::new("open").arg("-t").arg(path).status();
            }
        });
    }
    Ok(())
}

/// An action waiting for "save changes?" confirmation.
pub enum Pending {
    New,
    Open(PathBuf),
    /// Close and open the updated Velo (Preferences → Updates).
    Restart,
    /// The demo: the Velo template, live (docs/product.md's first five minutes).
    Demo,
    /// The window's close button, with work unsaved; the project menu's Quit.
    Quit,
    /// Out of the project, back to the start screen's list.
    Start,
}

/// Where Velo keeps its settings, autosave and crash lock: `VELO_DATA` when set (a
/// separate profile, e.g. a second copy for testing), else the OS app-data folder.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("VELO_DATA").map(PathBuf::from).or_else(|| eframe::storage_dir("Velo"))
}

/// `VELO_CAPTURE_TO=shot.png`: once the window has settled (5 s, past the "Opened…"
/// notice), saves what it shows as a PNG and closes. Docs screenshots, with no
/// screen-recording permission needed.
pub fn capture_to_file(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ASKED: AtomicBool = AtomicBool::new(false);
    let Some(path) = std::env::var_os("VELO_CAPTURE_TO") else { return };
    // VELO_CAPTURE_AT=seconds: later than 5 s (an AI reply to wait for)
    let at = std::env::var("VELO_CAPTURE_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    if ctx.input(|i| i.time) > at && !ASKED.swap(true, Ordering::Relaxed) {
        let done = ctx.clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
            if let Err(e) = save_png(Path::new(&path), &image) {
                eprintln!("velo: couldn't save the capture: {e}");
            }
            done.send_viewport_cmd(egui::ViewportCommand::Close);
            done.request_repaint();
        })));
    }
    ctx.request_repaint();
}

fn save_png(path: &Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let mut png = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), image.size[0] as u32, image.size[1] as u32);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header().map_err(std::io::Error::other)?.write_image_data(image.as_raw()).map_err(std::io::Error::other)
}

fn data_file(name: &str) -> Option<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

/// The bundle `path` names: the folder itself, or its `project.ron` (what an open dialog that
/// can't pick folders returns).
pub fn bundle(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    let dir = path.parent().filter(|_| path.file_name().is_some_and(|f| f == PROJECT))?;
    dir.extension().is_some_and(|e| e == EXTENSION).then(|| dir.to_path_buf())
}

/// A shader file name a project may use: a plain `.wgsl` name inside `shaders/`. A project
/// can come from anyone: "../../somewhere" must never be read or written.
fn plain(file: &str) -> bool {
    Path::new(file).file_name().is_some_and(|f| f == file) && (file.ends_with(".wgsl") || file.ends_with(".fs")) && !file.starts_with('.')
}

/// Where a Shader node's code goes in `shaders/`: its file, or one named after its id (nodes
/// from single-file projects, names a project shouldn't have).
fn file_of(node: &Node) -> String {
    node.file.clone().filter(|f| plain(f)).unwrap_or_else(|| format!("{}.wgsl", node.id))
}

/// Writes a node's code to `shaders/` if the file doesn't hold it already; returns the file.
fn store(dir: &Path, node: &Node, code: &str) -> Result<PathBuf, String> {
    let path = dir.join(SHADERS).join(file_of(node));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(code) {
        write_file(&path, code.as_bytes())?;
    }
    Ok(path)
}

/// Whether `path` is a one-file project: a zip.
pub fn is_package(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    path.is_file() && std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *b"PK\x03\x04"
}

/// A project: one file, a bundle (their shader files read into the nodes; a missing one is
/// listed, the node keeps no code) or a RON file. Returns it and the shader files that weren't there.
pub fn read(path: &Path) -> Result<(Project, Vec<String>), String> {
    if is_package(path) {
        return read_package(path, package_dir(MEDIA, path).as_deref());
    }
    let Some(dir) = bundle(path) else { return Ok((read_ron(path)?, Vec::new())) };
    let mut project = read_ron(&dir.join(PROJECT))?;
    let mut missing = Vec::new();
    for node in project.scenes.iter_mut().flat_map(|s| &mut s.nodes) {
        let Some(file) = node.file.clone() else { continue };
        match plain(&file).then(|| std::fs::read_to_string(dir.join(SHADERS).join(&file)).ok()).flatten() {
            Some(code) => node.code = Some(code),
            None => missing.push(file),
        }
    }
    Ok((project, missing))
}

fn read_ron(path: &Path) -> Result<Project, String> {
    parse(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?, path)
}

fn parse(text: &str, path: &Path) -> Result<Project, String> {
    let mut project: Project = ron::from_str(text).map_err(|e| tr!("{} is not a valid project: {}", path.display(), e))?;
    // saving it here would drop whatever the newer version added
    if project.format > FORMAT {
        return Err(tr!("{} was saved by a newer Velo: update to open it", path.display()));
    }
    project.migrate();
    Ok(project)
}

/// Where a one-file project's own folders go in Velo's data folder (its media, its shaders being
/// edited outside): named after it and where it is.
fn package_dir(kind: &str, path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).hash(&mut hash);
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    Some(data_dir()?.join(kind).join(format!("{stem}-{:08x}", hash.finish() as u32)))
}

/// A one-file project: `project.ron`, the shaders' code from `shaders/`, and what `media/` holds
/// written out under `media` (its own folder in Velo's data folder), its nodes pointed there.
fn read_package(path: &Path, media: Option<&Path>) -> Result<(Project, Vec<String>), String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| err(&e))?).map_err(|e| err(&e))?;
    let text = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str| {
        let mut text = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut text).ok().map(|_| text)
    };
    let mut project = parse(&text(&mut zip, PROJECT).ok_or_else(|| tr!("{} is not a valid project: {}", path.display(), PROJECT))?, path)?;
    let mut missing = Vec::new();
    for node in project.scenes.iter_mut().flat_map(|s| &mut s.nodes) {
        let Some(file) = node.file.clone() else { continue };
        match plain(&file).then(|| text(&mut zip, &format!("{SHADERS}/{file}"))).flatten() {
            Some(code) => node.code = Some(code),
            None => missing.push(file),
        }
    }
    let Some(media) = media.filter(|_| zip.file_names().any(|n| n.is_ok_and(|n| n.starts_with(MEDIA)))) else { return Ok((project, missing)) };
    // nothing from outside the zip's own media folder, and nowhere outside ours
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| err(&e))?;
        let Some(name) = entry.enclosed_name().filter(|n| n.starts_with(MEDIA) && !entry.is_dir()) else { continue };
        let to = media.join(&name);
        let written = to.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::File::create(&to)).and_then(|mut out| std::io::copy(&mut entry, &mut out));
        written.map_err(|e| format!("{}: {e}", to.display()))?;
    }
    for node in project.scenes.iter_mut().flat_map(|s| &mut s.nodes) {
        if let Some(own) = node.media.as_deref().map(Path::new).filter(|m| m.starts_with(MEDIA) && m.components().all(|c| matches!(c, std::path::Component::Normal(_)))) {
            node.media = Some(media.join(own).to_string_lossy().into_owned());
        }
    }
    Ok((project, missing))
}

/// Writes a one-file project: a zip of `project.ron` and each Shader node's code in `shaders/`.
/// With `media`, the files its nodes show go in `media/` too, their paths made its own: it opens
/// on any computer.
pub fn write_package(path: &Path, project: &Project, media: bool) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp).map_err(|e| err(&e))?);
    let text = SimpleFileOptions::default();
    let mut stored = project.clone();
    let mut written = std::collections::HashSet::new();
    for node in stored.scenes.iter_mut().flat_map(|s| &mut s.nodes) {
        let Some(code) = node.code.take() else { continue };
        let file = file_of(node);
        if written.insert(file.clone()) {
            zip.start_file(format!("{SHADERS}/{file}"), text).map_err(|e| err(&e))?;
            zip.write_all(code.as_bytes()).map_err(|e| err(&e))?;
        }
        node.file = Some(file);
    }
    if media {
        // each file (or folder of frames) once, under a name of its own; pictures and movies are
        // packed already, so they go in as they are
        let mut names: HashMap<PathBuf, String> = HashMap::new();
        for node in stored.scenes.iter_mut().flat_map(|s| &mut s.nodes) {
            let Some(from) = node.media.as_deref().map(PathBuf::from).filter(|m| m.exists()) else { continue };
            if let Some(name) = names.get(&from) {
                node.media = Some(name.clone());
                continue;
            }
            let file = from.file_name().map_or_else(|| "media".to_string(), |f| f.to_string_lossy().into_owned());
            let (stem, ext) = file.rsplit_once('.').filter(|_| from.is_file()).map_or((file.as_str(), String::new()), |(s, e)| (s, format!(".{e}")));
            let taken = |n: &String| names.values().any(|v| v == n);
            let name = std::iter::once(format!("{MEDIA}/{stem}{ext}")).chain((2..).map(|k| format!("{MEDIA}/{stem}-{k}{ext}"))).find(|n| !taken(n)).expect("a free name");
            for file in if from.is_dir() { walk(&from) } else { vec![from.clone()] } {
                // a folder's files under its name, as they sit in it
                let at = match file.strip_prefix(&from) {
                    Ok(rest) if !rest.as_os_str().is_empty() => format!("{name}/{}", rest.to_string_lossy().replace('\\', "/")),
                    _ => name.clone(),
                };
                let size = std::fs::metadata(&file).map_or(0, |m| m.len());
                let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(size >= u64::from(u32::MAX));
                zip.start_file(at, options).map_err(|e| err(&e))?;
                std::io::copy(&mut std::fs::File::open(&file).map_err(|e| format!("{}: {e}", file.display()))?, &mut zip).map_err(|e| err(&e))?;
            }
            names.insert(from, name.clone());
            node.media = Some(name);
        }
    }
    let ron = ron::ser::to_string_pretty(&stored, ron::ser::PrettyConfig::default().compact_arrays(true)).map_err(|e| e.to_string())?;
    zip.start_file(PROJECT, text).map_err(|e| err(&e))?;
    zip.write_all(ron.as_bytes()).map_err(|e| err(&e))?;
    let file = zip.finish().map_err(|e| err(&e))?;
    file.sync_all().map_err(|e| err(&e))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|e| err(&e))
}

/// Every file under `dir`, in its folders too.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).flat_map(|p| if p.is_dir() { walk(&p) } else { vec![p] }).collect();
    files.sort();
    files
}

/// Writes a bundle: each Shader node's code to `shaders/` (only files that changed, so an
/// outside editor isn't told about the others), then `project.ron` without the code.
pub fn write_bundle(dir: &Path, project: &Project) -> Result<(), String> {
    let shaders = dir.join(SHADERS);
    std::fs::create_dir_all(&shaders).map_err(|e| format!("{}: {e}", shaders.display()))?;
    let mut stored = project.clone();
    for node in stored.scenes.iter_mut().flat_map(|s| &mut s.nodes) {
        let Some(code) = node.code.take() else { continue };
        store(dir, node, &code)?;
        node.file = Some(file_of(node));
    }
    write_ron(&dir.join(PROJECT), &stored)
}

/// Writes a single-file project (the autosave; a project opened from one).
pub fn write_ron(path: &Path, project: &Project) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(project, ron::ser::PrettyConfig::default().compact_arrays(true)).map_err(|e| e.to_string())?;
    write_file(path, text.as_bytes())
}

/// Writes to a temp file first and syncs it, so a crash mid-save never corrupts a file.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    written.and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Shader files an outside editor changed since they were last seen, as (scene, node, code).
/// `seen` holds each file's modification time; a file not seen before counts as unchanged.
pub fn changed_shaders(dir: &Path, project: &Project, seen: &mut HashMap<String, SystemTime>) -> Vec<(String, String, String)> {
    let mut changed = Vec::new();
    for scene in &project.scenes {
        for node in &scene.nodes {
            if !is_shader(&node.kind) {
                continue;
            }
            let file = file_of(node);
            let path = dir.join(SHADERS).join(&file);
            let Ok(time) = std::fs::metadata(&path).and_then(|m| m.modified()) else { continue };
            if seen.insert(file, time).is_some_and(|before| before != time)
                && let Ok(code) = std::fs::read_to_string(&path)
                && node.code.as_deref() != Some(code.as_str())
            {
                changed.push((scene.id.clone(), node.id.clone(), code));
            }
        }
    }
    changed
}

/// Unsaved work from a session that crashed or closed with changes, if any.
/// Also marks this session as running.
pub fn take_recovery() -> Option<Project> {
    let lock = data_file(LOCK)?;
    let recovered = lock.exists().then(|| data_file(AUTOSAVE).and_then(|p| read_ron(&p).ok())).flatten();
    let _ = std::fs::write(&lock, b"running");
    recovered
}

impl App {
    pub fn dirty(&self) -> bool {
        self.session.project != self.saved
    }

    pub fn file_name(&self) -> String {
        self.file.as_deref().and_then(Path::file_stem).map_or(tr!("Untitled").into(), |s| s.to_string_lossy().into_owned())
    }

    /// The work Velo closed with unsaved last time, open again (the start screen's Restore).
    pub fn restore_recovery(&mut self) {
        if let Some(project) = self.recovery.take() {
            self.load_project(project, None, Project::default());
        }
    }

    fn load_project(&mut self, project: Project, file: Option<PathBuf>, saved: Project) {
        // a project is open: the start screen has done its part
        self.start = false;
        self.session = Session::with_project(project);
        if let Some(live) = &mut self.live {
            live.restart();
        }
        // the loading screen again while this project's scenes are built and pictured (not for
        // a new or a one-scene project, ready at once; not in a screenshot run)
        if self.session.project.scenes.len() > 1 && std::env::var_os("VELO_CAPTURE_TO").is_none() {
            (self.splash, self.splash_from) = (Some(None), None);
        }
        self.saved = saved;
        self.file = file;
        self.watch.seen.clear();
    }

    /// New/Open: asks first if there are unsaved changes.
    pub fn request(&mut self, action: Pending) {
        if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.load_project(Project::default(), None, Project::default()),
            Pending::Open(path) => match read(&path) {
                Ok((project, missing)) => {
                    let path = bundle(&path).unwrap_or(path);
                    self.remember(&path);
                    self.load_project(project.clone(), Some(path), project);
                    match missing.is_empty() {
                        true => self.say(Tone::Done, tr!("Opened {}", self.file_name())),
                        false => self.say(Tone::Error, tr!("Opened {}, but these shader files are missing: {}", self.file_name(), missing.join(", "))),
                    }
                }
                Err(e) => self.say(Tone::Error, e),
            },
            Pending::Restart => self.updates.restart = true,
            Pending::Quit => self.quitting = Some(false),
            Pending::Start => {
                self.load_project(Project::default(), None, Project::default());
                self.start = true;
            }
            Pending::Demo => {
                let demo = crate::templates::demo();
                self.load_project(demo.clone(), None, demo);
                // the first five minutes' one guide: long enough to read (screenshots go without it)
                self.toast = std::env::var_os("VELO_CAPTURE_TO")
                    .is_none()
                    .then(|| crate::shell::Toast::new(Tone::Action, tr!("The demo: play some music. The knobs below are its macros; the scenes' + starts more from a template.").into()).lasting(20.0));
            }
        }
    }

    /// The latest of the recent projects (ten kept).
    fn remember(&mut self, path: &Path) {
        let recent = &mut self.settings.recent;
        recent.retain(|p| p != path);
        recent.insert(0, path.to_path_buf());
        recent.truncate(10);
    }

    /// Where the project dialogs start: the open project's folder, else the last project's (the
    /// system's own memory is the last folder any dialog of the app saw, pictures included).
    fn projects_dir(&self) -> Option<PathBuf> {
        self.file.iter().chain(&self.settings.recent).find_map(|p| p.parent().filter(|d| d.is_dir())).map(Path::to_path_buf)
    }

    fn project_dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match self.projects_dir() {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
    }

    pub fn open_dialog(&mut self) {
        // a bundle is a folder: dialogs that can't pick one take its project.ron
        if let Some(path) = self.project_dialog().add_filter(tr!("Velo project"), &[EXTENSION, "ron"]).pick_file() {
            self.request(Pending::Open(path));
        }
    }

    pub fn save(&mut self) -> bool {
        match self.file.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let dialog = self.project_dialog().add_filter(tr!("Velo project"), &[EXTENSION]).set_file_name(format!("{}.{EXTENSION}", self.file_name()));
        match dialog.save_file() {
            Some(path) => self.save_to(path.with_extension(EXTENSION)),
            None => false,
        }
    }

    /// Saves as one file, unless `path` is a bundle or a RON file already (it stays one until
    /// Save As).
    fn save_to(&mut self, path: PathBuf) -> bool {
        let project = &self.session.project;
        let written = if path.is_dir() {
            write_bundle(&path, project)
        } else if path.is_file() && !is_package(&path) {
            write_ron(&path, project)
        } else {
            write_package(&path, project, false)
        };
        match written {
            Ok(()) => {
                self.saved = self.session.project.clone();
                // nothing unsaved now: a crash from here offers nothing older than this
                if let Some(snapshot) = data_file(AUTOSAVE) {
                    let _ = std::fs::remove_file(snapshot);
                }
                self.autosaved = self.saved.clone();
                self.remember(&path);
                self.file = Some(path);
                self.watch.seen.clear();
                self.say(Tone::Done, tr!("Saved {}", self.file_name()));
                true
            }
            Err(e) => {
                self.say(Tone::Error, e);
                false
            }
        }
    }

    /// Adds the scenes of another project (one file, a bundle or a RON file) to this one, as one
    /// undoable step.
    pub fn import_dialog(&mut self) {
        let Some(path) = self.project_dialog().add_filter(tr!("Velo project"), &[EXTENSION, "ron"]).pick_file() else { return };
        let path = bundle(&path).unwrap_or(path);
        let (project, missing) = match read(&path) {
            Ok(read) => read,
            Err(e) => return self.say(Tone::Error, e),
        };
        let name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        // all at once with that show's shared signals: what they read comes along, once
        if !project.scenes.is_empty()
            && let Err(e) = self.session.run("scene.import", json!({ "scenes": project.scenes, "shared": project.shared }))
        {
            return self.say(Tone::Error, e);
        }
        match (project.scenes.as_slice(), missing.is_empty()) {
            ([], _) => self.say(Tone::Error, tr!("{} has no scenes", name)),
            (_, false) => self.say(Tone::Error, tr!("Imported from {}, but these shader files are missing: {}", name, missing.join(", "))),
            ([one], true) => self.say(Tone::Done, tr!("Imported {} from {}", one.name, name)),
            (all, true) => self.say(Tone::Done, tr!("Imported {} scenes from {}", all.len(), name)),
        }
    }

    /// Writes the project, or one scene of it as a project of its own, to one file with the
    /// files its nodes show: what to hand to someone else.
    pub fn export_dialog(&mut self, scene: Option<&str>) {
        let mut project = self.session.project.clone();
        if let Some(id) = scene {
            project.scenes.retain(|s| s.id == id);
            // what controllers do belongs to the show it came from
            project.bindings.clear();
            project.name = project.scenes.first().map_or_else(String::new, |s| s.name.clone());
        }
        let name = if scene.is_some() { project.name.clone() } else { self.file_name() };
        let Some(path) = self.project_dialog().add_filter(tr!("Velo project"), &[EXTENSION]).set_file_name(format!("{name}.{EXTENSION}")).save_file() else { return };
        let path = path.with_extension(EXTENSION);
        match write_package(&path, &project, true) {
            Ok(()) => self.say(Tone::Done, tr!("Exported {}: one file with its media, ready to share", path.file_name().unwrap_or_default().to_string_lossy())),
            Err(e) => self.say(Tone::Error, e),
        }
    }

    /// Twice a second, in a bundle or a one-file project: what an outside editor saved comes
    /// into its node, each save one undoable step.
    pub fn watch_shaders(&mut self, now: f64) {
        if now - self.watch.last < 0.5 {
            return;
        }
        self.watch.last = now;
        let Some(dir) = self.shader_dir() else { return };
        for (scene, node, code) in changed_shaders(&dir, &self.session.project, &mut self.watch.seen) {
            self.run("node.set_code", json!({ "scene": scene, "node": node, "code": code }));
        }
    }

    /// Where the Shader nodes' files are for an outside editor: the bundle itself, or for a
    /// one-file project a folder of its own in Velo's data folder.
    fn shader_dir(&self) -> Option<PathBuf> {
        let file = self.file.as_deref()?;
        if file.is_dir() {
            return Some(file.to_path_buf());
        }
        package_dir("editing", file).filter(|_| is_package(file))
    }

    /// Opens a Shader node's file in the outside editor (Preferences → Code), saving the
    /// project first if it never was; Velo then picks up every save made there.
    pub fn edit_outside(&mut self, node: &str) {
        if self.shader_dir().is_none() && !self.save_as() {
            return;
        }
        let Some(dir) = self.shader_dir() else { return };
        let Some(n) = self.session.project.graphs().flat_map(|s| &s.nodes).find(|n| n.id == node) else { return };
        let opened = std::fs::create_dir_all(dir.join(SHADERS)).map_err(|e| e.to_string()).and_then(|_| store(&dir, n, n.code.as_deref().unwrap_or_default())).and_then(|path| launch(&self.settings.editor, &path).map(|_| path));
        match opened {
            Ok(path) => self.say(Tone::Action, tr!("Editing {} outside Velo: every save shows here", path.file_name().unwrap_or_default().to_string_lossy())),
            Err(e) => self.say(Tone::Error, tr!("Couldn't open the code editor: {}", e)),
        }
    }

    /// Every 60 s, snapshot unsaved work for crash recovery (nothing unsaved, nothing to snapshot).
    pub fn autosave(&mut self, now: f64) {
        // while "restore unsaved work?" is unanswered, the old snapshot is what we'd overwrite
        if now - self.last_autosave < AUTOSAVE_SECONDS || self.recovery.is_some() || self.recovery_later {
            return;
        }
        self.last_autosave = now;
        if self.dirty()
            && self.session.project != self.autosaved
            && let Some(path) = data_file(AUTOSAVE)
            && write_ron(&path, &self.session.project).is_ok()
        {
            self.autosaved = self.session.project.clone();
        }
    }

    /// On a clean exit with nothing unsaved, drop the lock; otherwise keep the work for next time.
    pub fn exit_cleanup(&mut self) {
        if self.recovery.is_some() || self.recovery_later {
            return; // never answered: offer it again next time
        }
        if self.dirty() {
            if let Some(path) = data_file(AUTOSAVE) {
                let _ = write_ron(&path, &self.session.project);
            }
        } else {
            for name in [LOCK, AUTOSAVE] {
                if let Some(path) = data_file(name) {
                    let _ = std::fs::remove_file(path); // nothing to recover next time
                }
            }
        }
    }

    /// "Save changes?" (before New, Open, the demo, an update's restart, and the window's close
    /// button) and "restore unsaved work?" dialogs.
    pub fn file_modals(&mut self, ctx: &egui::Context) {
        // the close button: unsaved work asks first; saved work, or an answer given, just goes
        if ctx.input(|i| i.viewport().close_requested()) && self.quitting.is_none() && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending.get_or_insert(Pending::Quit);
        }
        if self.quitting == Some(false) {
            self.quitting = Some(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let t = self.theme.clone();
        if let Some(action) = self.pending.take() {
            let quit = matches!(action, Pending::Quit);
            let title = if quit { tr!("Quit Velo") } else { tr!("Unsaved changes") };
            let name = self.file_name();
            let (choice, closed) = kit::modal(ctx, &t, "unsaved", title, 380.0, |ui| {
                ui.label(RichText::new(tr!("Save changes to {}?", name)).strong().color(t.fg_text));
                ui.label(RichText::new(tr!("Your changes will be lost if you don't save them.")).color(t.fg_muted));
                ui.add_space(t.gap);
                // the way out on the left, what's asked for on the right, as the system's own dialogs
                let shown = kit::actions(
                    ui,
                    &t,
                    &[(kit::Variant::Ghost, Some(crate::icons::X), tr!("Cancel")), (kit::Variant::Secondary, Some(crate::icons::TRASH), tr!("Don't Save")), (kit::Variant::Primary, Some(crate::icons::FLOPPY_DISK), tr!("Save"))],
                );
                // (0 saves, 1 doesn't, 2 cancels)
                shown.iter().position(egui::Response::clicked).map(|i| 2 - i)
            });
            match if closed { Some(2) } else { choice } {
                Some(0) if self.save() => self.perform(action),
                Some(1) => {
                    // quitting without saving: nothing to offer back next time
                    if quit {
                        self.saved = self.session.project.clone();
                    }
                    self.perform(action);
                }
                Some(2) => {}
                _ => self.pending = Some(action), // still open (or save was cancelled)
            }
        }
        // (the start screen offers it itself, as one of the things to open)
        if !self.start
            && let Some(project) = self.recovery.take()
        {
            let (choice, closed) = kit::modal(ctx, &t, "recover", tr!("Restore unsaved work?"), 380.0, |ui| {
                ui.label(RichText::new(tr!("Velo closed with unsaved changes last time. You can pick up where you left off.")).color(t.fg_muted));
                ui.add_space(t.gap);
                // throwing the work away can't be undone: held, not clicked
                let restore = kit::action(ui, &t, kit::Variant::Primary, Some(crate::icons::CLOCK_COUNTER_CLOCKWISE), tr!("Restore")).clicked();
                let discard = kit::hold_button(ui, &t, Some(crate::icons::TRASH), tr!("Hold to discard"), tr!("Discarded"), 1.0);
                if restore { Some(0) } else { discard.then_some(1) }
            });
            match choice {
                Some(0) => self.load_project(project, None, Project::default()),
                Some(_) => {}
                // closed: asked again next time
                None if closed => self.recovery_later = true,
                None => self.recovery = Some(project),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_round_trip_through_ron_files() {
        let dir = std::env::temp_dir().join("velo-files-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("show.velo");
        let mut session = Session::default();
        session.run("scene.add", serde_json::json!({ "name": "Drop" })).unwrap();
        session.run("project.set_canvas", serde_json::json!({ "width": 1280, "height": 720 })).unwrap();
        write_ron(&path, &session.project).unwrap();
        assert_eq!(read(&path).unwrap(), (session.project, Vec::new()));
        assert!(!dir.join("show.velo.tmp").exists(), "temp file renamed away");
        std::fs::write(&path, "not ron").unwrap();
        assert!(read(&path).is_err());
        std::fs::write(&path, format!("(format: {})", FORMAT + 1)).unwrap();
        assert!(read(&path).unwrap_err().contains("newer"), "a newer file isn't opened (saving it would lose data)");
    }

    #[test]
    fn one_file_projects_carry_their_shaders_and_when_exported_their_media() {
        let dir = std::env::temp_dir().join(format!("velo-package-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("frames")).unwrap();
        // a picture, and a folder of frames, somewhere on this computer
        let (picture, frames) = (dir.join("logo.png"), dir.join("frames"));
        std::fs::write(&picture, b"png bytes").unwrap();
        std::fs::write(frames.join("0001.png"), b"frame one").unwrap();
        let mut session = Session::default();
        session.run("scene.add", json!({})).unwrap();
        session.run("node.add", json!({ "kind": velo_core::SHADER, "name": "Tunnel", "code": velo_engine::TEMPLATE })).unwrap();
        for media in [&picture, &picture, &frames] {
            session.run("node.add", json!({ "kind": velo_core::IMAGE })).unwrap();
            session.project.scenes[0].nodes.last_mut().unwrap().media = Some(media.to_string_lossy().into_owned());
        }
        let project = session.project.clone();
        // saved: one file, the code in it, the media where they are
        let path = dir.join("Show.velo");
        write_package(&path, &project, false).unwrap();
        assert!(is_package(&path) && !dir.join("Show.velo.tmp").exists());
        assert_eq!(read_package(&path, None).unwrap(), (project.clone(), Vec::new()));
        // exported: the media go in too, each once, and come out somewhere else
        let shared = dir.join("Shared.velo");
        write_package(&shared, &project, true).unwrap();
        std::fs::remove_file(&picture).unwrap();
        let (opened, missing) = read_package(&shared, Some(&dir.join("out"))).unwrap();
        assert!(missing.is_empty());
        let shown: Vec<PathBuf> = opened.scenes[0].nodes.iter().filter_map(|n| n.media.as_deref().map(PathBuf::from)).collect();
        assert_eq!(shown.len(), 3);
        assert!(shown.iter().all(|m| m.starts_with(dir.join("out"))), "{shown:?}");
        assert_eq!(shown[0], shown[1], "one file for the two nodes that show it");
        assert_eq!(std::fs::read(&shown[0]).unwrap(), b"png bytes");
        assert_eq!(std::fs::read(shown[2].join("0001.png")).unwrap(), b"frame one", "a folder of frames whole");
        assert_eq!(opened.scenes[0].nodes[0].code, project.scenes[0].nodes[0].code);
        // and a RON file is no zip
        write_ron(&dir.join("Old.velo"), &project).unwrap();
        assert!(!is_package(&dir.join("Old.velo")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundles_keep_shaders_as_files_and_see_outside_saves() {
        let dir = std::env::temp_dir().join(format!("velo-bundle-test-{}", std::process::id())).join("Show.velo");
        let _ = std::fs::remove_dir_all(&dir);
        let mut session = Session::default();
        session.run("scene.add", json!({})).unwrap();
        session.run("node.add", json!({ "kind": velo_core::SHADER, "name": "Tunnel", "code": velo_engine::TEMPLATE })).unwrap();
        session.run("node.add", json!({ "kind": "velo.noise" })).unwrap();
        // a node from a single-file project: code, no file
        let scene = session.project.scenes[0].id.clone();
        let mut project = session.project.clone();
        project.scenes[0].nodes.push(Node {
            id: "n_old".into(),
            kind: velo_core::SHADER.into(),
            name: "Old".into(),
            off: false,
            pos: [0.0; 2],
            params: Default::default(),
            mappings: Vec::new(),
            code: Some("old".into()),
            file: None,
            media: None,
            text: None,
            icon: String::new(),
        });
        write_bundle(&dir, &project).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("shaders/tunnel.wgsl")).unwrap(), velo_engine::TEMPLATE);
        assert_eq!(std::fs::read_to_string(dir.join("shaders/n_old.wgsl")).unwrap(), "old");
        let ron = std::fs::read_to_string(dir.join(PROJECT)).unwrap();
        assert!(!ron.contains("main_image"), "the code lives in shaders/, not project.ron");
        // read back, through the folder or its project.ron
        let (back, missing) = read(&dir).unwrap();
        assert!(missing.is_empty());
        assert_eq!(back.scenes[0].nodes[0].code.as_deref(), Some(velo_engine::TEMPLATE));
        assert_eq!(back.scenes[0].nodes[2].file.as_deref(), Some("n_old.wgsl"));
        assert_eq!(bundle(&dir.join(PROJECT)), Some(dir.clone()));
        // an unchanged file isn't rewritten (an outside editor would think it changed)
        let time = |f: &str| std::fs::metadata(dir.join(SHADERS).join(f)).unwrap().modified().unwrap();
        let before = time("tunnel.wgsl");
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_bundle(&dir, &back).unwrap();
        assert_eq!(time("tunnel.wgsl"), before);
        // an outside save: seen once, then changed
        let mut seen = HashMap::new();
        assert!(changed_shaders(&dir, &back, &mut seen).is_empty(), "the first look is the baseline");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("shaders/tunnel.wgsl"), "edited").unwrap();
        let changed = changed_shaders(&dir, &back, &mut seen);
        assert_eq!(changed, [(scene, back.scenes[0].nodes[0].id.clone(), "edited".to_string())]);
        assert!(changed_shaders(&dir, &back, &mut seen).is_empty(), "once");
        // a missing file is reported; a name leaving shaders/ is never read
        std::fs::remove_file(dir.join("shaders/n_old.wgsl")).unwrap();
        let mut evil = back.clone();
        evil.scenes[0].nodes[0].file = Some("../project.ron".into());
        evil.scenes[0].nodes[0].code = None;
        write_ron(&dir.join(PROJECT), &evil).unwrap();
        let (opened, missing) = read(&dir).unwrap();
        assert_eq!(missing, ["../project.ron", "n_old.wgsl"]);
        assert_eq!(opened.scenes[0].nodes[0].code, None);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}

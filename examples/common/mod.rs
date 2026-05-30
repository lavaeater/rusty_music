//! Shared helpers for the sample-browsing examples (`sample_browser`, `music_machine`).
//!
//! Pure data/IO logic with no Bevy dependency, so it can be unit-reasoned about
//! and reused by any front-end:
//!   * [`FsBrowser`] — roam the filesystem anywhere, dirs-first, audio-only,
//!     with an optional recursive "grouped by folder" view.
//!   * [`SamplePool`] — a favourites list of absolute sample paths, saved to /
//!     loaded from a human-readable TOML file.
//!   * [`disk_asset_path`] — turn an absolute path into a `disk://…` asset path
//!     for the `disk` asset source (see each example's `main`).
//!
//! Not every example uses every item, hence the blanket allow.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
//  Audio file detection + asset paths
// ─────────────────────────────────────────────────────────────────────────────

/// Extensions we treat as playable samples. Mirrors the `bevy_seedling` decoder
/// features enabled in `Cargo.toml`.
pub const AUDIO_EXTS: &[&str] = &["wav", "ogg", "mp3", "flac"];

/// True if `path` looks like a sample we can decode.
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

/// Build a `disk://…` asset path for an absolute on-disk file, suitable for
/// `AssetServer::load` once a `disk` source rooted at `/` is registered.
///
/// The `disk` source root is `/`, so the asset path is the absolute path with
/// its leading separator stripped: `/home/x/kick.wav` → `disk://home/x/kick.wav`.
pub fn disk_asset_path(abs: &Path) -> String {
    let s = abs.to_string_lossy();
    let trimmed = s.trim_start_matches('/');
    format!("disk://{trimmed}")
}

// ─────────────────────────────────────────────────────────────────────────────
//  Filesystem browser
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Non-selectable section heading (recursive/grouped view only).
    Header,
    Parent,
    Dir,
    File,
}

pub struct Entry {
    pub kind: EntryKind,
    /// Absolute path. Empty for headers.
    pub path: PathBuf,
    /// Display label.
    pub label: String,
    /// Indent depth (grouped view) for nicer rendering.
    pub depth: usize,
}

impl Entry {
    pub fn is_selectable(&self) -> bool {
        self.kind != EntryKind::Header
    }
    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }
    pub fn is_dir_like(&self) -> bool {
        matches!(self.kind, EntryKind::Dir | EntryKind::Parent)
    }
}

/// A directory browser that can roam the whole filesystem.
pub struct FsBrowser {
    /// Directory currently being shown.
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    /// When true, show every audio file beneath `dir`, grouped under per-folder
    /// headers, instead of a single flat directory listing.
    pub recursive: bool,
    /// Safety cap on files gathered in recursive mode.
    pub max_recursive_files: usize,
}

impl FsBrowser {
    pub fn new(start: PathBuf) -> Self {
        let mut b = Self {
            dir: start,
            entries: Vec::new(),
            selected: 0,
            recursive: false,
            max_recursive_files: 1500,
        };
        b.refresh();
        b
    }

    /// A sensible place to start browsing: `$HOME`, else current dir, else `/`.
    pub fn default_start() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    pub fn toggle_recursive(&mut self) {
        self.recursive = !self.recursive;
        self.selected = 0;
        self.refresh();
        self.ensure_selectable_forward();
    }

    pub fn move_up(&mut self) {
        if self.selected == 0 {
            return;
        }
        let mut i = self.selected;
        while i > 0 {
            i -= 1;
            if self.entries[i].is_selectable() {
                self.selected = i;
                return;
            }
        }
        // No selectable entry above; stay put.
    }

    pub fn move_down(&mut self) {
        let n = self.entries.len();
        let mut i = self.selected;
        while i + 1 < n {
            i += 1;
            if self.entries[i].is_selectable() {
                self.selected = i;
                return;
            }
        }
    }

    /// Enter the highlighted directory (or `..`). Returns true if we navigated.
    pub fn enter_selected(&mut self) -> bool {
        let Some(entry) = self.entries.get(self.selected) else {
            return false;
        };
        if entry.is_dir_like() {
            let target = entry.path.clone();
            self.dir = target;
            self.recursive = false;
            self.selected = 0;
            self.refresh();
            self.ensure_selectable_forward();
            true
        } else {
            false
        }
    }

    /// Go to the parent directory.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.dir.parent().map(|p| p.to_path_buf()) {
            self.dir = parent;
            self.recursive = false;
            self.selected = 0;
            self.refresh();
            self.ensure_selectable_forward();
        }
    }

    fn ensure_selectable_forward(&mut self) {
        if self
            .entries
            .get(self.selected)
            .map(|e| e.is_selectable())
            .unwrap_or(true)
        {
            return;
        }
        self.move_down();
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        if self.recursive {
            self.refresh_recursive();
        } else {
            self.refresh_flat();
        }
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    fn refresh_flat(&mut self) {
        if let Some(parent) = self.dir.parent() {
            self.entries.push(Entry {
                kind: EntryKind::Parent,
                path: parent.to_path_buf(),
                label: "..".into(),
                depth: 0,
            });
        }
        let (mut dirs, mut files) = (Vec::new(), Vec::new());
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    dirs.push((name, path));
                } else if is_audio_file(&path) {
                    files.push((name, path));
                }
            }
        }
        dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        for (name, path) in dirs {
            self.entries.push(Entry { kind: EntryKind::Dir, path, label: name, depth: 0 });
        }
        for (name, path) in files {
            self.entries.push(Entry { kind: EntryKind::File, path, label: name, depth: 0 });
        }
    }

    /// Walk the subtree under `dir`, collecting audio files grouped by their
    /// containing folder. Each folder becomes a header, files listed beneath.
    fn refresh_recursive(&mut self) {
        // folder (relative to dir) -> sorted file paths
        let mut groups: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
        let mut budget = self.max_recursive_files;

        // Iterative DFS, directories visited in sorted order.
        let mut stack = vec![self.dir.clone()];
        let mut visited_dirs: Vec<PathBuf> = Vec::new();
        while let Some(dir) = stack.pop() {
            if budget == 0 {
                break;
            }
            let mut subdirs = Vec::new();
            let mut files = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let path = e.path();
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        subdirs.push(path);
                    } else if is_audio_file(&path) {
                        files.push(path);
                    }
                }
            }
            files.sort_by(|a, b| a.to_string_lossy().to_lowercase().cmp(&b.to_string_lossy().to_lowercase()));
            if files.len() > budget {
                files.truncate(budget);
            }
            budget -= files.len();
            if !files.is_empty() {
                groups.push((dir.clone(), files));
            }
            visited_dirs.push(dir);
            // Push subdirs reversed so they pop in sorted order.
            subdirs.sort_by(|a, b| b.to_string_lossy().to_lowercase().cmp(&a.to_string_lossy().to_lowercase()));
            stack.extend(subdirs);
        }

        groups.sort_by(|a, b| a.0.to_string_lossy().to_lowercase().cmp(&b.0.to_string_lossy().to_lowercase()));

        if groups.is_empty() {
            self.entries.push(Entry {
                kind: EntryKind::Header,
                path: PathBuf::new(),
                label: "(no audio files found below this folder)".into(),
                depth: 0,
            });
            return;
        }

        let root = self.dir.clone();
        for (folder, files) in groups {
            let rel = folder.strip_prefix(&root).unwrap_or(&folder);
            let header = if rel.as_os_str().is_empty() {
                "./".to_string()
            } else {
                format!("{}/", rel.to_string_lossy())
            };
            let depth = rel.components().count();
            self.entries.push(Entry {
                kind: EntryKind::Header,
                path: PathBuf::new(),
                label: header,
                depth,
            });
            for f in files {
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.entries.push(Entry { kind: EntryKind::File, path: f, label: name, depth: depth + 1 });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Sample pool (favourites)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct PoolFile {
    samples: Vec<String>,
}

/// An ordered set of absolute sample paths the user has marked as favourites.
#[derive(Default)]
pub struct SamplePool {
    paths: Vec<PathBuf>,
}

impl SamplePool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.iter().any(|p| p == path)
    }

    /// Add if absent, returns true if newly added.
    pub fn add(&mut self, path: PathBuf) -> bool {
        if self.contains(&path) {
            false
        } else {
            self.paths.push(path);
            true
        }
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.paths.len();
        self.paths.retain(|p| p != path);
        before != self.paths.len()
    }

    /// Add if absent, remove if present. Returns true if the path is now in the pool.
    pub fn toggle(&mut self, path: PathBuf) -> bool {
        if self.contains(&path) {
            self.remove(&path);
            false
        } else {
            self.paths.push(path);
            true
        }
    }

    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        let pf = PoolFile {
            samples: self.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        };
        let toml = toml::to_string_pretty(&pf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(file, toml)
    }

    pub fn load(file: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(file)?;
        let pf: PoolFile = toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Self { paths: pf.samples.into_iter().map(PathBuf::from).collect() })
    }
}

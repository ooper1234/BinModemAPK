//! Stand-in for the `rfd` file dialog crate, which has no Android backend.
//!
//! A desktop dialog blocks until a file is chosen; a phone app cannot block
//! its only thread on a dialog it would have to draw itself. So:
//!
//! * `pick_file` and `pick_folder` answer "cancelled" and leave a [`Request`]
//!   saying what was asked for. The phone layout takes it with
//!   [`take_request`], shows a file browser, and hands the chosen file to
//!   whatever asked (it knows the callers by their filters).
//! * `save_file` needs no question at all: it answers with the suggested name
//!   in the folder set by [`set_save_dir`], numbered so nothing is
//!   overwritten.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a dialog was opened for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub folder: bool,
    /// The filters as given: a name and its extensions, lower case.
    pub filters: Vec<(String, Vec<String>)>,
}

impl Request {
    /// Every extension any filter allows; empty means any file.
    pub fn extensions(&self) -> Vec<String> {
        self.filters.iter().flat_map(|(_, e)| e.iter().cloned()).collect()
    }

    pub fn first_filter(&self) -> Option<&str> {
        self.filters.first().map(|(n, _)| n.as_str())
    }
}

static REQUEST: Mutex<Option<Request>> = Mutex::new(None);
static SAVE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The dialog last asked for and not yet shown.
pub fn take_request() -> Option<Request> {
    REQUEST.lock().ok()?.take()
}

/// Where `save_file` puts things.
pub fn set_save_dir(dir: PathBuf) {
    if let Ok(mut d) = SAVE_DIR.lock() {
        *d = Some(dir);
    }
}

pub fn save_dir() -> PathBuf {
    SAVE_DIR.lock().ok().and_then(|d| d.clone()).unwrap_or_else(|| PathBuf::from("saved"))
}

#[derive(Debug, Default, Clone)]
pub struct FileDialog {
    filters: Vec<(String, Vec<String>)>,
    name: Option<String>,
}

impl FileDialog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        self.filters
            .push((name.into(), extensions.iter().map(|e| e.to_string().to_ascii_lowercase()).collect()));
        self
    }
    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
    pub fn set_directory<P: AsRef<Path>>(self, _path: P) -> Self {
        self
    }
    pub fn set_title(self, _title: impl Into<String>) -> Self {
        self
    }
    fn ask(self, folder: bool) {
        if let Ok(mut r) = REQUEST.lock() {
            *r = Some(Request { folder, filters: self.filters });
        }
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        self.ask(false);
        None
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        self.ask(false);
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        self.ask(true);
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        let dir = save_dir();
        std::fs::create_dir_all(&dir).ok()?;
        let name = self.name.unwrap_or_else(|| "saved".to_owned());
        let path = Path::new(&name);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "saved".to_owned());
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let mut candidate = dir.join(&name);
        let mut n = 2;
        while candidate.exists() {
            candidate = dir.join(format!("{stem}-{n}{ext}"));
            n += 1;
        }
        Some(candidate)
    }
}

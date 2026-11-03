//! Stand-in for the `rfd` file dialog crate, which has no Android backend.
//!
//! Every dialog reports that it was cancelled. The GUI keeps a text box beside
//! each "Browse" button, so a path can still be typed in by hand.

use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct FileDialog;

impl FileDialog {
    pub fn new() -> Self {
        Self
    }
    pub fn add_filter(self, _name: impl Into<String>, _extensions: &[impl ToString]) -> Self {
        self
    }
    pub fn set_file_name(self, _name: impl Into<String>) -> Self {
        self
    }
    pub fn set_directory<P: AsRef<Path>>(self, _path: P) -> Self {
        self
    }
    pub fn set_title(self, _title: impl Into<String>) -> Self {
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        None
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        None
    }
}

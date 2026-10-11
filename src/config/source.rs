//! Every `.zlint` file that takes part in a configuration, plus the spans
//! pointing into them. Spans survive macro expansion and imports, so errors
//! always point at real source text.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::line_index::LineIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: usize, end: usize) -> Self {
        Span { file, start: start as u32, end: end as u32 }
    }

    pub fn to(self, other: Span) -> Span {
        if self.file != other.file {
            return self;
        }
        Span { file: self.file, start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

pub struct SourceFile {
    /// Display name: a path for user files, `std/rust` for library ones.
    pub name: String,
    pub path: Option<PathBuf>,
    pub text: Arc<str>,
    pub lines: LineIndex,
}

#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, name: String, path: Option<PathBuf>, text: Arc<str>) -> FileId {
        let lines = LineIndex::new(&text);
        self.files.push(SourceFile { name, path, text, lines });
        FileId(self.files.len() as u32 - 1)
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn files(&self) -> impl Iterator<Item = (FileId, &SourceFile)> {
        self.files.iter().enumerate().map(|(i, f)| (FileId(i as u32), f))
    }
}

/// Where configuration text comes from. Disk reads honour registered buffers;
/// the LSP can provide its own document overlay.
pub trait SourceProvider: Sync {
    fn read(&self, path: &Path) -> io::Result<String>;
}

pub struct Disk;

impl SourceProvider for Disk {
    fn read(&self, path: &Path) -> io::Result<String> {
        if let Some(text) = crate::engine::reads::buffer(&normalize_path(path)) {
            return Ok(text.to_string());
        }
        std::fs::read_to_string(path)
    }
}

/// Lexically normalise a path so the same file always produces the same key,
/// whether it came from the CLI, an import, or an LSP URI.
pub fn normalize_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for comp in absolute.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(p) => {
                let s = p.as_os_str().to_string_lossy();
                // `c:` and `C:` are the same drive; LSP clients love lowercase.
                out.push(uppercase_drive(&s));
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn uppercase_drive(prefix: &str) -> String {
    let mut chars: Vec<char> = prefix.chars().collect();
    if chars.len() >= 2 && chars[1] == ':' {
        chars[0] = chars[0].to_ascii_uppercase();
    }
    chars.into_iter().collect()
}

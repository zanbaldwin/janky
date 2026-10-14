//! Finds the media files that note fields refer to, in the `media_dirs` of the project.
//!
//! A [`Library`] searches the media directories in order and records each file that it finds.
//! Anki keeps media in one flat directory, so the package stores each file under its file name
//! only. Two different files with the same file name are a collision. Problems are collected in
//! the library, and the caller turns them into diagnostics.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use crate::project::inside_path;

/// A media file that goes into the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaFile {
    /// The file name that notes refer to, and that Anki stores the file under.
    pub name: String,
    /// The path to show to the author, such as `media/a/pear.png`.
    pub path: String,
    /// `referenced` for a file in a media directory. Generated files use `generated`.
    pub kind: &'static str,
}

/// Why [`Library::lookup`] did not accept a media path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No media directory has the file.
    Missing,
    /// The path is absolute, goes above its media directory, or is a symlink to a file outside it.
    Outside,
    /// A different file with the same file name was found first. `first` is its
    /// [`MediaFile::path`].
    Collision { first: String },
}

/// The media files that the notes refer to, and the problems found on the way.
#[derive(Debug)]
pub struct Library {
    dirs: Vec<(String, PathBuf)>,
    files: IndexMap<String, (PathBuf, MediaFile)>,
    problems: Vec<(String, Problem)>,
}

impl Library {
    /// Makes an empty library for the media directories `dirs`, relative to `root`. An empty list
    /// uses `media`.
    pub fn new(root: &Path, dirs: &[String]) -> Self {
        let dirs = if dirs.is_empty() { vec!["media".to_string()] } else { dirs.to_vec() };
        let dirs = dirs.into_iter().map(|dir| (dir.trim_end_matches('/').to_string(), root.join(&dir))).collect();
        Self {
            dirs,
            files: IndexMap::new(),
            problems: Vec::new(),
        }
    }

    /// The names of the media directories, without a trailing `/`.
    pub fn dirs(&self) -> Vec<&str> {
        self.dirs.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// Finds a media path in the first media directory that has it, and records the file.
    ///
    /// It returns `false` and records a [`Problem`] if the file is missing or outside its
    /// directory. It returns `true` if it finds the file, also for a [`Problem::Collision`], which
    /// it records too. A second lookup of the same file adds nothing.
    pub fn lookup(&mut self, path: &str) -> bool {
        let Some(normal) = inside_path(path) else {
            self.problems.push((path.to_string(), Problem::Outside));
            return false;
        };
        for (dir_name, dir) in &self.dirs {
            let full = dir.join(&normal);
            if !full.is_file() {
                continue;
            }
            let (Ok(canonical), Ok(dir)) = (full.canonicalize(), dir.canonicalize()) else {
                continue;
            };
            if !canonical.starts_with(&dir) {
                self.problems.push((path.to_string(), Problem::Outside));
                return false;
            }
            let name = normal.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let relative = normal.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>();
            let shown = format!("{dir_name}/{}", relative.join("/"));
            match self.files.get(&name) {
                Some((existing, file)) if *existing != canonical => {
                    let first = file.path.clone();
                    self.problems.push((path.to_string(), Problem::Collision { first }));
                },
                Some(_) => {},
                None => {
                    let file = MediaFile {
                        name: name.clone(),
                        path: shown,
                        kind: "referenced",
                    };
                    self.files.insert(name, (canonical, file));
                },
            }
            return true;
        }
        self.problems.push((path.to_string(), Problem::Missing));
        false
    }

    /// The problems recorded since the last call, in the order of the lookups, with the path as
    /// given. The library keeps none of them.
    pub fn take_problems(&mut self) -> Vec<(String, Problem)> {
        std::mem::take(&mut self.problems)
    }

    /// The recorded files, in the order of their first lookup.
    pub fn files(&self) -> Vec<MediaFile> {
        self.files.values().map(|(_, file)| file.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str) {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, path).unwrap();
    }

    #[test]
    fn lookup_searches_the_dirs_in_order() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "media/a/⚛️.png");
        write(root.path(), "more/b/⚛️.png");
        write(root.path(), "more/c.ogg");
        let mut library = Library::new(root.path(), &["media".into(), "more/".into()]);
        assert!(library.lookup("a/⚛️.png"));
        assert!(library.lookup("./a/⚛️.png"));
        assert!(library.lookup("c.ogg"));
        assert!(library.take_problems().is_empty());
        assert!(library.lookup("b/⚛️.png"));
        assert!(!library.lookup("missing-🧪.png"));
        assert!(!library.lookup("../deck.toml"));
        assert!(!library.lookup("/etc/passwd"));
        assert_eq!(
            library.take_problems(),
            [
                ("b/⚛️.png".to_string(), Problem::Collision { first: "media/a/⚛️.png".into() }),
                ("missing-🧪.png".to_string(), Problem::Missing),
                ("../deck.toml".to_string(), Problem::Outside),
                ("/etc/passwd".to_string(), Problem::Outside),
            ]
        );
        let names: Vec<(String, String)> = library.files().into_iter().map(|file| (file.name, file.path)).collect();
        assert_eq!(
            names,
            [
                ("⚛️.png".to_string(), "media/a/⚛️.png".to_string()),
                ("c.ogg".to_string(), "more/c.ogg".to_string())
            ]
        );
    }

    #[test]
    fn symlinks_must_stay_inside_the_dir() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "secret.txt");
        std::fs::create_dir_all(root.path().join("media")).unwrap();
        std::os::unix::fs::symlink(root.path().join("secret.txt"), root.path().join("media/link.txt")).unwrap();
        let mut library = Library::new(root.path(), &[]);
        assert!(!library.lookup("link.txt"));
        assert_eq!(library.take_problems(), [("link.txt".to_string(), Problem::Outside)]);
    }
}

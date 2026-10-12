//! Quick Open: every file the sidebar would show under the open folder, so
//! Ctrl+P (⌘P) can jump to one by typing part of its path.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use crate::file_tree::DEFAULT_EXCLUDED;

/// The most files one listing hands to the palette. A writing folder runs to
/// the hundreds; the cap keeps a folder opened by accident from filling the
/// picker with tens of thousands of rows.
pub const MAX_FILES: usize = 4000;

/// Every visible file under `root`, by path. `show_hidden` follows the
/// sidebar's setting for VS Code's default-excluded entries.
pub fn files_under(root: &Path, show_hidden: bool, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut queue = VecDeque::from([root.to_path_buf()]);
    while let Some(folder) = queue.pop_front() {
        if files.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            if !show_hidden && DEFAULT_EXCLUDED.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                queue.push_back(path);
            } else if files.len() < limit {
                files.push(path);
            }
        }
    }
    files.sort_by(|a, b| {
        a.to_string_lossy()
            .to_lowercase()
            .cmp(&b.to_string_lossy().to_lowercase())
    });
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
    }

    fn relative(root: &Path, files: Vec<PathBuf>) -> Vec<String> {
        files
            .into_iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn files_under_walks_deep_and_skips_excluded_entries() {
        let dir = std::env::temp_dir().join(format!("malgel-quick-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        touch(&dir, "top.md");
        touch(&dir, "notes/a.md");
        touch(&dir, "notes/deep/b.markdown");
        touch(&dir, ".git/config");
        touch(&dir, "node_modules/x.js");

        // By path, so "node_modules" sorts before "notes".
        assert_eq!(
            relative(&dir, files_under(&dir, false, 100)),
            [
                "node_modules/x.js",
                "notes/a.md",
                "notes/deep/b.markdown",
                "top.md"
            ]
        );
        // Version-control folders stay out until hidden entries are on.
        let hidden = relative(&dir, files_under(&dir, true, 100));
        assert!(hidden.contains(&".git/config".to_string()));
        // The cap holds however many files are there.
        assert_eq!(files_under(&dir, true, 1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

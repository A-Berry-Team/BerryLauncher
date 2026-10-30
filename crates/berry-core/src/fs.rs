use std::path::{Path, PathBuf};

/// Depth-first search below `root` for a file whose name satisfies `wanted`.
///
/// Directories at most `max_depth` levels below `root` are read (the root itself is level 0),
/// and a directory is only entered when `enter` accepts its path.
pub fn find_file(
    root: &Path,
    max_depth: u8,
    enter: impl Fn(&Path) -> bool,
    wanted: impl Fn(&str) -> bool,
) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `file_type` does not follow symlinks; only pay for a `stat` when it is one.
            let kind = match entry.file_type() {
                Ok(kind) if kind.is_symlink() => path.metadata().map(|meta| meta.file_type()),
                other => other,
            };
            let Ok(kind) = kind else {
                continue;
            };
            if kind.is_file() {
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(&wanted)
                {
                    return Some(path);
                }
            } else if kind.is_dir() && enter(&path) {
                stack.push((path, depth + 1));
            }
        }
    }
    None
}

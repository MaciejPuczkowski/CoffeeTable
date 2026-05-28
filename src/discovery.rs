use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 4;

pub fn scan_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if root.is_dir() {
            walk(root, MAX_DEPTH, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn walk(dir: &Path, depth_left: usize, out: &mut Vec<PathBuf>) {
    if depth_left == 0 {
        return;
    }
    if dir.join(".git").exists() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip(&name) {
            continue;
        }
        walk(&entry.path(), depth_left - 1, out);
    }
}

fn should_skip(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | "bin"
            | "obj"
            | ".idea"
            | ".vs"
            | ".vscode"
            | "dist"
            | "build"
            | ".next"
            | ".gradle"
            | ".cache"
            | "venv"
            | ".venv"
            | "__pycache__"
    )
}

pub fn canon_key(p: &Path) -> String {
    let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
        .to_lowercase()
}

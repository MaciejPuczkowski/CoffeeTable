use std::path::Path;

pub fn folder(expanded: bool) -> &'static str {
    if expanded { "\u{e5fe}" } else { "\u{e5ff}" }
}

pub fn for_file(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if let Some(icon) = match_special(&lower) {
        return icon;
    }
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match_extension(ext)
}

fn match_special(lower: &str) -> Option<&'static str> {
    match lower {
        "cargo.toml" | "cargo.lock" => Some("\u{e7a8}"),
        "package.json" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => {
            Some("\u{e71e}")
        }
        "dockerfile" | "dockerfile.dev" | "docker-compose.yml" | "docker-compose.yaml" => {
            Some("\u{e650}")
        }
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some("\u{e65d}"),
        ".env" | ".env.local" | ".env.production" | ".env.development" => Some("\u{e615}"),
        "readme.md" | "readme.txt" | "readme" => Some("\u{e66d}"),
        "license" | "license.md" | "license.txt" => Some("\u{e60a}"),
        "makefile" => Some("\u{e673}"),
        _ => None,
    }
}

fn match_extension(ext: &str) -> &'static str {
    match ext {
        "rs" => "\u{e68b}",
        "py" | "pyi" | "pyw" => "\u{e606}",
        "js" | "mjs" | "cjs" => "\u{e60c}",
        "ts" => "\u{e628}",
        "tsx" | "jsx" => "\u{e7ba}",
        "json" | "jsonc" => "\u{e60b}",
        "yaml" | "yml" => "\u{e6a8}",
        "toml" => "\u{e615}",
        "md" | "markdown" | "mdx" => "\u{e609}",
        "html" | "htm" | "xhtml" => "\u{e60e}",
        "css" => "\u{e614}",
        "scss" | "sass" => "\u{e603}",
        "less" => "\u{e614}",
        "sh" | "bash" | "zsh" | "fish" => "\u{e691}",
        "ps1" | "bat" | "cmd" => "\u{e691}",
        "go" => "\u{e627}",
        "c" => "\u{e649}",
        "h" => "\u{e649}",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "\u{e646}",
        "cs" => "\u{e648}",
        "java" | "class" | "jar" => "\u{e66d}",
        "kt" | "kts" => "\u{e634}",
        "swift" => "\u{e755}",
        "rb" | "erb" => "\u{e739}",
        "php" => "\u{e73d}",
        "vue" => "\u{e6a0}",
        "svelte" => "\u{e697}",
        "lua" => "\u{e620}",
        "vim" => "\u{e62b}",
        "el" | "elc" => "\u{e632}",
        "clj" | "cljs" => "\u{e76a}",
        "ex" | "exs" => "\u{e62d}",
        "hs" => "\u{e777}",
        "ml" | "mli" => "\u{e67a}",
        "scala" | "sbt" => "\u{e737}",
        "dart" => "\u{e64c}",
        "r" => "\u{e68a}",
        "jl" => "\u{e624}",
        "zig" => "\u{e6a9}",
        "txt" | "text" => "\u{e64e}",
        "log" => "\u{e602}",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" => "\u{e60d}",
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" => "\u{f03d}",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "\u{f001}",
        "zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "bz2" => "\u{f1c6}",
        "pdf" => "\u{e67d}",
        "doc" | "docx" => "\u{f1c2}",
        "xls" | "xlsx" => "\u{f1c3}",
        "ppt" | "pptx" => "\u{f1c4}",
        "sql" | "db" | "sqlite" | "sqlite3" => "\u{e706}",
        "xml" => "\u{e619}",
        "ini" | "conf" | "cfg" | "config" => "\u{e615}",
        "lock" => "\u{e65e}",
        "diff" | "patch" => "\u{e728}",
        "" => "\u{e64e}",
        _ => "\u{e64e}",
    }
}

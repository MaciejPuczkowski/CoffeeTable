use ratatui::style::Color;
use std::path::Path;

pub const RED: Color = Color::Rgb(0xe6, 0x61, 0x4e);
pub const ORANGE: Color = Color::Rgb(0xe0, 0xa4, 0x58);
pub const YELLOW: Color = Color::Rgb(0xdc, 0xdc, 0xaa);
pub const GREEN: Color = Color::Rgb(0x9b, 0xc8, 0x8f);
pub const BLUE: Color = Color::Rgb(0x7d, 0xae, 0xa3);
pub const AZURE: Color = Color::Rgb(0xd4, 0x81, 0x5f);
pub const CYAN: Color = Color::Rgb(0x8e, 0xc0, 0x7c);
pub const PURPLE: Color = Color::Rgb(0xc8, 0x84, 0xa8);
pub const GREY: Color = Color::Rgb(0xa8, 0x99, 0x84);

pub fn folder(expanded: bool) -> &'static str {
    if expanded { "\u{f0770}" } else { "\u{f024b}" }
}

pub fn folder_color() -> Color {
    AZURE
}

pub fn color_for_file(name: &str) -> Color {
    let lower = name.to_lowercase();
    if let Some(c) = match_special_color(&lower) {
        return c;
    }
    let ext = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match_extension_color(ext)
}

fn match_special_color(lower: &str) -> Option<Color> {
    match lower {
        "cargo.toml" | "cargo.lock" => Some(ORANGE),
        "package.json" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => Some(YELLOW),
        "dockerfile" | "dockerfile.dev" | "docker-compose.yml" | "docker-compose.yaml" => {
            Some(BLUE)
        }
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some(PURPLE),
        ".env" | ".env.local" | ".env.production" | ".env.development" => Some(GREY),
        "readme.md" | "readme.txt" | "readme" => Some(GREY),
        "license" | "license.md" | "license.txt" => Some(BLUE),
        "makefile" => Some(GREY),
        _ => None,
    }
}

fn match_extension_color(ext: &str) -> Color {
    match ext {
        "rs" => ORANGE,
        "py" | "pyi" | "pyw" => YELLOW,
        "js" | "mjs" | "cjs" => YELLOW,
        "ts" => AZURE,
        "tsx" => BLUE,
        "jsx" => AZURE,
        "json" | "jsonc" => YELLOW,
        "yaml" | "yml" => PURPLE,
        "toml" => ORANGE,
        "md" | "markdown" | "mdx" => GREY,
        "html" | "htm" | "xhtml" => ORANGE,
        "css" => AZURE,
        "scss" | "sass" | "less" => PURPLE,
        "sh" | "bash" | "zsh" | "fish" => GREEN,
        "ps1" | "bat" | "cmd" => GREY,
        "go" => AZURE,
        "c" | "h" => BLUE,
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => AZURE,
        "cs" => BLUE,
        "java" | "class" | "jar" => ORANGE,
        "kt" | "kts" => BLUE,
        "swift" => ORANGE,
        "rb" | "erb" => RED,
        "php" => PURPLE,
        "vue" => GREEN,
        "svelte" => ORANGE,
        "lua" => AZURE,
        "vim" => GREEN,
        "el" | "elc" => PURPLE,
        "clj" | "cljs" => GREEN,
        "ex" | "exs" => PURPLE,
        "hs" => BLUE,
        "ml" | "mli" => ORANGE,
        "scala" | "sbt" => RED,
        "dart" => AZURE,
        "r" => BLUE,
        "jl" => PURPLE,
        "zig" => ORANGE,
        "txt" | "text" => GREY,
        "log" => GREY,
        "png" => PURPLE,
        "jpg" | "jpeg" => ORANGE,
        "gif" => AZURE,
        "webp" => BLUE,
        "svg" => ORANGE,
        "ico" | "bmp" | "tif" | "tiff" => YELLOW,
        "mp4" => AZURE,
        "mov" => CYAN,
        "mkv" => GREEN,
        "avi" => GREY,
        "webm" => GREY,
        "flv" => GREY,
        "mp3" => AZURE,
        "wav" => GREEN,
        "flac" => ORANGE,
        "ogg" => GREY,
        "m4a" => PURPLE,
        "aac" => YELLOW,
        "zip" => AZURE,
        "tar" => CYAN,
        "gz" | "tgz" => GREY,
        "xz" => GREEN,
        "7z" => BLUE,
        "rar" => GREEN,
        "bz2" => ORANGE,
        "pdf" => RED,
        "doc" | "docx" => AZURE,
        "xls" | "xlsx" => GREEN,
        "ppt" | "pptx" => RED,
        "sql" | "db" | "sqlite" | "sqlite3" => GREY,
        "xml" => ORANGE,
        "ini" | "conf" | "cfg" | "config" => BLUE,
        "lock" => GREY,
        "diff" | "patch" => YELLOW,
        _ => GREY,
    }
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
        "cargo.toml" | "cargo.lock" => Some("\u{f1617}"),
        "package.json" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => Some("\u{f0626}"),
        "dockerfile" | "dockerfile.dev" | "docker-compose.yml" | "docker-compose.yaml" => {
            Some("\u{f0868}")
        }
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some("\u{f02a2}"),
        ".env" | ".env.local" | ".env.production" | ".env.development" => Some("\u{f0493}"),
        "readme.md" | "readme.txt" | "readme" => Some("\u{f0354}"),
        "license" | "license.md" | "license.txt" => Some("\u{f0fc6}"),
        "makefile" => Some("\u{f1064}"),
        _ => None,
    }
}

fn match_extension(ext: &str) -> &'static str {
    match ext {
        "rs" => "\u{f1617}",
        "py" | "pyi" | "pyw" => "\u{f0320}",
        "js" | "mjs" | "cjs" => "\u{f031e}",
        "ts" => "\u{f06e6}",
        "tsx" | "jsx" => "\u{e7ba}",
        "json" | "jsonc" => "\u{f0626}",
        "yaml" | "yml" => "\u{e6a8}",
        "toml" => "\u{e6b2}",
        "md" | "markdown" | "mdx" => "\u{f0354}",
        "html" | "htm" | "xhtml" => "\u{f031d}",
        "css" => "\u{f031c}",
        "scss" | "sass" | "less" => "\u{f031c}",
        "sh" | "bash" | "zsh" | "fish" => "\u{e795}",
        "ps1" | "bat" | "cmd" => "\u{f069f}",
        "go" => "\u{f07d3}",
        "c" | "h" => "\u{f0671}",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "\u{f0672}",
        "cs" => "\u{f031b}",
        "java" | "class" | "jar" => "\u{f0b37}",
        "kt" | "kts" => "\u{f1219}",
        "swift" => "\u{f06e5}",
        "rb" | "erb" => "\u{f0d2d}",
        "php" => "\u{f031f}",
        "vue" => "\u{f0844}",
        "svelte" => "\u{e697}",
        "lua" => "\u{f08b1}",
        "vim" => "\u{e62b}",
        "el" | "elc" => "\u{e632}",
        "clj" | "cljs" => "\u{e768}",
        "ex" | "exs" => "\u{e62d}",
        "hs" => "\u{f0c93}",
        "ml" | "mli" => "\u{e67a}",
        "scala" | "sbt" => "\u{e737}",
        "dart" => "\u{e64c}",
        "r" => "\u{f25d}",
        "jl" => "\u{e624}",
        "zig" => "\u{e6a9}",
        "txt" | "text" => "\u{f0219}",
        "log" => "\u{f0224}",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" => "\u{f0225}",
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" => "\u{f022b}",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "\u{f0223}",
        "zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "bz2" => "\u{f01c4}",
        "pdf" => "\u{f0226}",
        "doc" | "docx" => "\u{f1392}",
        "xls" | "xlsx" => "\u{f138f}",
        "ppt" | "pptx" => "\u{f1390}",
        "sql" | "db" | "sqlite" | "sqlite3" => "\u{f01bc}",
        "xml" => "\u{f05c0}",
        "ini" | "conf" | "cfg" | "config" => "\u{f0493}",
        "lock" => "\u{f033e}",
        "diff" | "patch" => "\u{f0aa1}",
        "" => "\u{f0214}",
        _ => "\u{f0214}",
    }
}

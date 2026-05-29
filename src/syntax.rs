use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    FontStyle, Style as SynStyle, Theme, ThemeSet,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};

const THEME_XML: &str = include_str!("../assets/gruvbox-custom.tmTheme");

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let mut reader = std::io::Cursor::new(THEME_XML.as_bytes());
        match ThemeSet::load_from_reader(&mut reader) {
            Ok(t) => t,
            Err(_) => {
                let ts = ThemeSet::load_defaults();
                ts.themes["base16-mocha.dark"].clone()
            }
        }
    })
}

pub struct Highlighter {
    syntax_name: String,
}

impl Highlighter {
    pub fn for_extension(ext: &str) -> Self {
        let ps = syntax_set();
        let syntax = ps
            .find_syntax_by_extension(ext)
            .unwrap_or_else(|| ps.find_syntax_plain_text());
        Self {
            syntax_name: syntax.name.clone(),
        }
    }

    pub fn highlight_lines(&self, lines: &[String]) -> Vec<Vec<(Style, String)>> {
        let ps = syntax_set();
        let t = theme();
        let syntax: &SyntaxReference = ps
            .find_syntax_by_name(&self.syntax_name)
            .unwrap_or_else(|| ps.find_syntax_plain_text());
        let mut h = HighlightLines::new(syntax, t);
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            let buf = format!("{}\n", line);
            let regions = h.highlight_line(&buf, ps).unwrap_or_default();
            let mut spans: Vec<(Style, String)> = regions
                .into_iter()
                .map(|(s, t)| (convert_style(s), t.trim_end_matches('\n').to_string()))
                .filter(|(_, text)| !text.is_empty())
                .collect();
            if spans.is_empty() {
                spans.push((Style::default(), String::new()));
            }
            out.push(spans);
        }
        out
    }

}

fn convert_style(s: SynStyle) -> Style {
    let mut style = Style::default().fg(syn_color_to_ratatui(s.foreground));
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn syn_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitView {
    Working,
    Head,
    Diff,
}

#[derive(Default, Clone)]
pub struct YankRegister {
    pub text: String,
    pub linewise: bool,
}

#[derive(Clone)]
pub struct Snapshot {
    pub lines: Vec<Vec<char>>,
    pub cursor: (usize, usize),
}

pub struct CommandDef {
    pub key: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
}

pub const COMMANDS: &[CommandDef] = &[
    CommandDef { key: "w", aliases: &["write"], description: "Save the current file" },
    CommandDef { key: "q", aliases: &["close"], description: "Close editor (q! to force)" },
    CommandDef { key: "x", aliases: &["wq"], description: "Save and close" },
    CommandDef { key: "e", aliases: &["edit", "reload"], description: "Reload from disk (e! to discard)" },
    CommandDef { key: "Q", aliases: &["qa", "quit"], description: "Quit application (Q! to force)" },
    CommandDef { key: "f", aliases: &["find"], description: "Find file in project" },
    CommandDef { key: "g", aliases: &["grep"], description: "Grep across project" },
    CommandDef { key: "p", aliases: &["projects"], description: "Open project picker" },
    CommandDef { key: "t", aliases: &["tree", "explorer"], description: "Focus file tree" },
    CommandDef { key: "b", aliases: &["buffer"], description: "Focus editor" },
    CommandDef { key: "h", aliases: &["help"], description: "Show help overlay" },
    CommandDef { key: "S", aliases: &["settings", "config"], description: "Open settings.yaml in editor" },
    CommandDef { key: "H", aliases: &["head", "old"], description: "Show HEAD version of file (read-only)" },
    CommandDef { key: "W", aliases: &["working", "work", "new"], description: "Back to working copy (editable)" },
    CommandDef { key: "D", aliases: &["diff"], description: "Show unified diff against HEAD (read-only)" },
];

pub fn filter_commands(query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..COMMANDS.len()).collect();
    }
    let q = query.trim_end_matches('!').to_lowercase();
    if q.is_empty() {
        return (0..COMMANDS.len()).collect();
    }
    let mut scored: Vec<(i32, usize)> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(i, c)| score_command(c, &q).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, i)| i).collect()
}

fn score_command(c: &CommandDef, q: &str) -> Option<i32> {
    let key_l = c.key.to_lowercase();
    if key_l == q { return Some(100); }
    if key_l.starts_with(q) { return Some(80); }
    for a in c.aliases {
        let al = a.to_lowercase();
        if al == q { return Some(90); }
        if al.starts_with(q) { return Some(70); }
    }
    if c.description.to_lowercase().contains(q) { return Some(30); }
    None
}

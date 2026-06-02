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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    Off,
    Hard(u16),
}

impl WrapMode {
    pub fn cycle(self) -> Self {
        match self {
            WrapMode::Off => WrapMode::Hard(120),
            WrapMode::Hard(120) => WrapMode::Hard(80),
            _ => WrapMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WrapMode::Off => "no wrap",
            WrapMode::Hard(120) => "wrap 120",
            WrapMode::Hard(80) => "wrap 80",
            WrapMode::Hard(_) => "wrap",
        }
    }

    pub fn width(self) -> Option<usize> {
        match self {
            WrapMode::Hard(n) => Some(n as usize),
            WrapMode::Off => None,
        }
    }
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
    CommandDef { key: "S", aliases: &["settings", "config"], description: "Open settings split (global ↔ project)" },
    CommandDef { key: "import-settings", aliases: &["psi"], description: "Import CoffeeTable.Settings.yaml from project root into DB" },
    CommandDef { key: "export-settings", aliases: &["pse"], description: "Export project settings from DB to CoffeeTable.Settings.yaml" },
    CommandDef { key: "import-runtime", aliases: &["rti"], description: "Import CoffeeTable.Runtime.yaml from project root into DB" },
    CommandDef { key: "export-runtime", aliases: &["rte"], description: "Export runtime config from DB to CoffeeTable.Runtime.yaml" },
    CommandDef { key: "H", aliases: &["head", "old"], description: "Show HEAD version of file (read-only)" },
    CommandDef { key: "W", aliases: &["working", "work", "new"], description: "Back to working copy (editable)" },
    CommandDef { key: "D", aliases: &["diff"], description: "Show unified diff against HEAD (read-only)" },
    CommandDef { key: "L", aliases: &["lane"], description: "Toggle the Agents lane (right side)" },
    CommandDef { key: "R", aliases: &["rename"], description: "Rename the active agent (Agents view)" },
    CommandDef { key: "runtime", aliases: &[], description: "Open Runtime view" },
    CommandDef { key: "run", aliases: &[], description: "Run service(s) — :run <name> or :run for all" },
    CommandDef { key: "stop", aliases: &[], description: "Stop service(s) — :stop <name> or :stop for all" },
    CommandDef { key: "build", aliases: &[], description: "Build service(s) — :build <name> or :build for all" },
    CommandDef { key: "restart", aliases: &[], description: "Restart service(s) — :restart <name> or all" },
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

use crate::config::Paths;
use crate::db::Db;
use crate::project::{CommentKind, CommentStatus, FeatureStatus, ProjectMeta};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!(usage());
    }
    let (op, rest) = (&args[0], &args[1..]);
    let opts = parse_options(rest)?;
    let paths = Paths::resolve()?;
    let db = Db::open(&paths.db_file)?;
    match op.as_str() {
        "comment" => cmd_comment(&db, &opts),
        "log-turn" => cmd_log_turn(&db, &opts),
        "append-notes" => cmd_append_meta(&db, &opts, MetaField::AiNotes, true),
        "set-notes" => cmd_append_meta(&db, &opts, MetaField::AiNotes, false),
        "append-hints" => cmd_append_meta(&db, &opts, MetaField::AiHints, true),
        "set-hints" => cmd_append_meta(&db, &opts, MetaField::AiHints, false),
        "list-features" => cmd_list_features(&db, &opts),
        "set-feature-status" => cmd_set_feature_status(&db, &opts),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            Ok(())
        }
        other => bail!("unknown agent subcommand: {}\n\n{}", other, usage()),
    }
}

fn usage() -> &'static str {
    "Usage: coffeetable agent <subcommand> [--key value ...]\n\n\
Subcommands:\n  \
  comment       --feature-id <id> --kind <note|request|response> --message <text>\n  \
  log-turn      --feature-id <id> --request <text> --response <text>\n  \
  append-notes  --project-id <id> --message <text>\n  \
  set-notes     --project-id <id> --message <text>\n  \
  append-hints  --project-id <id> --message <text>\n  \
  set-hints     --project-id <id> --message <text>\n  \
  list-features      --project-id <id>\n  \
  set-feature-status --feature-id <id> --status <idea|todo|in_progress|in_review|done|cancelled>\n"
}

fn cmd_set_feature_status(db: &Db, opts: &HashMap<String, String>) -> Result<()> {
    let feature_id = require_i64(opts, "feature-id")?;
    let raw = require(opts, "status")?;
    let status = FeatureStatus::from_str(raw);
    if status == FeatureStatus::Idea && raw != "idea" {
        bail!("unknown status: {} (use idea|todo|in_progress|in_review|done|cancelled)", raw);
    }
    db.update_feature_status(feature_id, status)?;
    println!("{}", status.as_str());
    Ok(())
}

fn cmd_comment(db: &Db, opts: &HashMap<String, String>) -> Result<()> {
    let feature_id = require_i64(opts, "feature-id")?;
    let message = require(opts, "message")?;
    let kind = match opts.get("kind").map(|s| s.as_str()) {
        Some("request") => CommentKind::Request,
        Some("response") => CommentKind::Response,
        Some("note") | None => CommentKind::Note,
        Some(other) => bail!("unknown kind: {} (use note|request|response)", other),
    };
    let id = db.insert_comment_with_kind(feature_id, message, kind)?;
    println!("{}", id);
    Ok(())
}

fn cmd_log_turn(db: &Db, opts: &HashMap<String, String>) -> Result<()> {
    let feature_id = require_i64(opts, "feature-id")?;
    let request = require(opts, "request")?;
    let response = require(opts, "response")?;
    let req_id = db.insert_comment_with_kind(feature_id, request, CommentKind::Request)?;
    db.update_comment(req_id, request, CommentStatus::Done)?;
    let resp_id = db.insert_comment_with_kind(feature_id, response, CommentKind::Response)?;
    db.update_comment(resp_id, response, CommentStatus::Done)?;
    println!("{},{}", req_id, resp_id);
    Ok(())
}

#[derive(Clone, Copy)]
enum MetaField {
    AiHints,
    AiNotes,
}

fn cmd_append_meta(
    db: &Db,
    opts: &HashMap<String, String>,
    field: MetaField,
    append: bool,
) -> Result<()> {
    let project_id = require_i64(opts, "project-id")?;
    let message = require(opts, "message")?;
    let mut meta = db.load_project_meta(project_id)?;
    apply_meta_change(&mut meta, field, message, append);
    db.save_project_meta(project_id, &meta)?;
    Ok(())
}

fn apply_meta_change(meta: &mut ProjectMeta, field: MetaField, msg: &str, append: bool) {
    let target: &mut String = match field {
        MetaField::AiHints => &mut meta.ai_hints,
        MetaField::AiNotes => &mut meta.ai_notes,
    };
    if append {
        if !target.is_empty() && !target.ends_with('\n') {
            target.push('\n');
        }
        target.push_str(msg);
        target.push('\n');
    } else {
        *target = msg.to_string();
    }
}

fn cmd_list_features(db: &Db, opts: &HashMap<String, String>) -> Result<()> {
    let project_id = require_i64(opts, "project-id")?;
    let features = db.list_features(project_id)?;
    let mut out = String::from("[\n");
    for (i, f) in features.iter().enumerate() {
        let comma = if i + 1 == features.len() { "" } else { "," };
        out.push_str(&format!(
            "  {{\"id\": {}, \"title\": {:?}, \"status\": {:?}}}{}\n",
            f.id,
            f.title,
            f.status.label(),
            comma
        ));
    }
    out.push_str("]\n");
    print!("{}", out);
    Ok(())
}

fn parse_options(args: &[String]) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if !arg.starts_with("--") {
            bail!("unexpected positional argument: {}", arg);
        }
        let key = arg.trim_start_matches("--").to_string();
        let value = args
            .get(i + 1)
            .cloned()
            .ok_or_else(|| anyhow!("missing value for --{}", key))?;
        out.insert(key, value);
        i += 2;
    }
    Ok(out)
}

fn require<'a>(opts: &'a HashMap<String, String>, key: &str) -> Result<&'a str> {
    opts.get(key)
        .map(|s| s.as_str())
        .ok_or_else(|| anyhow!("missing --{}", key))
}

fn require_i64(opts: &HashMap<String, String>, key: &str) -> Result<i64> {
    require(opts, key)?
        .parse()
        .map_err(|_| anyhow!("--{} must be a number", key))
}

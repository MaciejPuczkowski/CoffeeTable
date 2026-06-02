use directories::BaseDirs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SESSION_WINDOW_SECS: u64 = 5 * 60 * 60;
const WEEKLY_WINDOW_SECS: u64 = 7 * 24 * 60 * 60;
const THINKING_GAP_CAP_SECS: u64 = 600;
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenSnapshot {
    pub session_tokens: u64,
    pub weekly_tokens: u64,
    pub session_thinking_secs: u64,
    pub weekly_thinking_secs: u64,
    pub last_scan_at: Option<SystemTime>,
    pub scanning: bool,
    pub last_error: Option<&'static str>,
}

pub fn start_background_scanner(state: Arc<Mutex<TokenSnapshot>>) {
    thread::Builder::new()
        .name("token-usage-scanner".into())
        .spawn(move || loop {
            {
                if let Ok(mut s) = state.lock() {
                    s.scanning = true;
                }
            }
            let result = scan_once();
            if let Ok(mut s) = state.lock() {
                match result {
                    Ok(totals) => {
                        s.session_tokens = totals.session_tokens;
                        s.weekly_tokens = totals.weekly_tokens;
                        s.session_thinking_secs = totals.session_thinking_secs;
                        s.weekly_thinking_secs = totals.weekly_thinking_secs;
                        s.last_scan_at = Some(SystemTime::now());
                        s.last_error = None;
                    }
                    Err(e) => {
                        s.last_error = Some(e);
                    }
                }
                s.scanning = false;
            }
            thread::sleep(REFRESH_INTERVAL);
        })
        .expect("spawn token scanner thread");
}

#[derive(Default)]
struct ScanTotals {
    session_tokens: u64,
    weekly_tokens: u64,
    session_thinking_secs: u64,
    weekly_thinking_secs: u64,
}

fn scan_once() -> Result<ScanTotals, &'static str> {
    let Some(base) = BaseDirs::new() else {
        return Err("no home dir");
    };
    let root = base.home_dir().join(".claude").join("projects");
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(ScanTotals::default()),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let session_cutoff = now.saturating_sub(SESSION_WINDOW_SECS);
    let weekly_cutoff = now.saturating_sub(WEEKLY_WINDOW_SECS);
    let mut totals = ScanTotals::default();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(jsonl_iter) = std::fs::read_dir(&path) else { continue };
        for j in jsonl_iter.flatten() {
            let p = j.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let mtime_secs = j
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if mtime_secs < weekly_cutoff {
                continue;
            }
            scan_file(&p, session_cutoff, weekly_cutoff, &mut totals);
        }
    }
    Ok(totals)
}

fn scan_file(
    path: &PathBuf,
    session_cutoff: u64,
    weekly_cutoff: u64,
    totals: &mut ScanTotals,
) {
    let Ok(content) = std::fs::read_to_string(path) else { return };
    let mut prev_ts: Option<u64> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else { continue };
        let Some(msg_type) = v.get("type").and_then(|t| t.as_str()) else { continue };
        if msg_type != "assistant" && msg_type != "user" {
            continue;
        }
        let Some(timestamp) = v.get("timestamp").and_then(|t| t.as_str()) else { continue };
        let Some(ts) = parse_rfc3339_secs(timestamp) else { continue };

        if msg_type == "assistant" && ts >= weekly_cutoff {
            if let Some(usage) = v.get("message").and_then(|m| m.get("usage")) {
                let tokens = sum_usage(usage);
                totals.weekly_tokens += tokens;
                if ts >= session_cutoff {
                    totals.session_tokens += tokens;
                }
            }
            if let Some(prev) = prev_ts {
                if ts > prev {
                    let delta = (ts - prev).min(THINKING_GAP_CAP_SECS);
                    totals.weekly_thinking_secs += delta;
                    if ts >= session_cutoff {
                        totals.session_thinking_secs += delta;
                    }
                }
            }
        }
        prev_ts = Some(ts);
    }
}

fn sum_usage(usage: &serde_json::Value) -> u64 {
    let pick = |key: &str| -> u64 {
        usage
            .get(key)
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    pick("input_tokens")
        + pick("output_tokens")
        + pick("cache_creation_input_tokens")
}

fn parse_rfc3339_secs(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i32 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let month: u32 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    let hour: u32 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
    let minute: u32 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
    let second: u32 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
    let days = days_from_civil(year, month as i32, day as i32);
    let secs = days as i64 * 86_400
        + hour as i64 * 3600
        + minute as i64 * 60
        + second as i64;
    if secs < 0 { None } else { Some(secs as u64) }
}

fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let m = m as i64;
    let d = d as i64;
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{}h{:02}m", h, m)
    }
}

pub fn format_compact(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

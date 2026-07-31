//! Claude Code token usage, computed from local transcripts.
//!
//! Every assistant message in `~/.claude/projects/<project>/<session>.jsonl`
//! carries a `usage` object alongside a top-level ISO-8601 `timestamp`. Summing
//! those gives session and rolling-week totals without any network call.
//!
//! Note: rate-limit *percentages* are not recoverable this way. Claude Code
//! does not persist quota or reset-time information anywhere on disk, so this
//! reports raw token counts only.

use chrono::{DateTime, Duration, Utc};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Default, Clone, Copy, Debug)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    /// What the model actually had to process this turn. Cache reads are
    /// excluded — they are the cheap path and would dwarf everything else.
    pub fn billable(&self) -> u64 {
        self.input + self.output + self.cache_write
    }

    fn add(&mut self, o: &Tokens) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
    }
}

#[derive(Default, Debug)]
pub struct Totals {
    pub session: Tokens,
    pub week: Tokens,
    pub session_name: String,
    pub files_scanned: usize,
}

pub fn projects_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    Path::new(&home).join(".claude/projects")
}

/// Collect totals. `session` is the most recently modified transcript.
pub fn collect() -> io::Result<Totals> {
    let root = projects_dir();
    let cutoff = Utc::now() - Duration::days(7);

    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for project in fs::read_dir(&root)? {
        let project = project?;
        if !project.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(project.path())? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let mtime = entry.metadata()?.modified()?;
            files.push((path, mtime));
        }
    }

    files.sort_by_key(|(_, m)| std::cmp::Reverse(*m));

    let mut totals = Totals::default();
    if let Some((newest, _)) = files.first() {
        totals.session_name = newest
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session")
            .chars()
            .take(8)
            .collect();
    }

    // A file untouched for over a week cannot contain entries inside the
    // window, so it can be skipped without reading it.
    let week_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7 * 86400);

    for (i, (path, mtime)) in files.iter().enumerate() {
        let is_session = i == 0;
        if !is_session && *mtime < week_ago {
            continue;
        }

        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        totals.files_scanned += 1;

        for line in BufReader::new(file).lines().map_while(Result::ok) {
            // Cheap reject before paying for a JSON parse; most lines are
            // user turns or tool results with no usage block.
            if !line.contains("\"usage\"") {
                continue;
            }
            let Some((tok, ts)) = parse_line(&line) else {
                continue;
            };

            if is_session {
                totals.session.add(&tok);
            }
            if ts.map(|t| t >= cutoff).unwrap_or(false) {
                totals.week.add(&tok);
            }
        }
    }

    Ok(totals)
}

/// Pull the usage counters and timestamp out of one transcript line.
///
/// Hand-rolled rather than pulling in a JSON parser: the fields are flat
/// integers in a known shape, and this runs over ~176 MB of transcripts.
fn parse_line(line: &str) -> Option<(Tokens, Option<DateTime<Utc>>)> {
    let u = line.find("\"usage\"")?;
    let tail = &line[u..];

    let tok = Tokens {
        input: num_after(tail, "\"input_tokens\":").unwrap_or(0),
        output: num_after(tail, "\"output_tokens\":").unwrap_or(0),
        cache_read: num_after(tail, "\"cache_read_input_tokens\":").unwrap_or(0),
        cache_write: num_after(tail, "\"cache_creation_input_tokens\":").unwrap_or(0),
    };

    let ts = str_after(line, "\"timestamp\":\"")
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc));

    Some((tok, ts))
}

fn num_after(hay: &str, key: &str) -> Option<u64> {
    let i = hay.find(key)? + key.len();
    let rest = &hay[i..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn str_after(hay: &str, key: &str) -> Option<String> {
    let i = hay.find(key)? + key.len();
    let rest = &hay[i..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Compact human form: 12345 -> "12.3K", 4500000 -> "4.5M".
pub fn short(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

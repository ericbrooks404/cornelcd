//! What Claude Code is doing right now, read from the live transcript.
//!
//! The newest `~/.claude/projects/*/*.jsonl` is the active session. Assistant
//! messages carry `tool_use` blocks naming the tool, and every line carries an
//! ISO-8601 timestamp, so the current activity is just "the most recent tool,
//! if it was recent enough."
//!
//! Reads incrementally — only the bytes appended since the last scan — because
//! a long session's transcript runs to megabytes and this polls every second.

use chrono::{DateTime, Utc};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Must match `clawd_activity_t` in the firmware's display.h.
pub const ACT_NONE: u8 = 0;
pub const ACT_IDLE: u8 = 1;
pub const ACT_RUN: u8 = 2;
pub const ACT_PAINT: u8 = 3;
pub const ACT_LOOK: u8 = 4;

/// Nothing seen for this long means Claude is between turns.
const QUIET_SECS: i64 = 25;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tally {
    pub bash: u32,
    pub edit: u32,
    pub write: u32,
    pub web: u32,
}

pub struct Watcher {
    path: Option<PathBuf>,
    offset: u64,
    tally: Tally,
    last_tool: Option<String>,
    last_seen: Option<DateTime<Utc>>,
    partial: String,
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Watcher {
    pub fn new() -> Watcher {
        Watcher {
            path: None,
            offset: 0,
            tally: Tally::default(),
            last_tool: None,
            last_seen: None,
            partial: String::new(),
        }
    }

    pub fn tally(&self) -> Tally {
        self.tally
    }

    /// The tool Claude used most recently, for display or debugging.
    pub fn last_tool(&self) -> Option<&str> {
        self.last_tool.as_deref()
    }

    /// Re-scan and return the activity state to report to the keyboard.
    pub fn poll(&mut self) -> io::Result<u8> {
        let newest = newest_transcript()?;

        // A new session means a fresh file: reset counters rather than
        // carrying the previous session's tally forward.
        if self.path.as_ref() != newest.as_ref() {
            self.path = newest.clone();
            self.offset = 0;
            self.tally = Tally::default();
            self.last_tool = None;
            self.last_seen = None;
            self.partial.clear();
        }

        let Some(path) = self.path.clone() else {
            return Ok(ACT_NONE);
        };

        let mut f = fs::File::open(&path)?;
        let len = f.metadata()?.len();

        // Truncated or rotated underneath us: start over.
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }

        if len > self.offset {
            f.seek(SeekFrom::Start(self.offset))?;
            let mut buf = Vec::with_capacity((len - self.offset) as usize);
            f.read_to_end(&mut buf)?;
            self.offset = len;

            let text = String::from_utf8_lossy(&buf);
            self.partial.push_str(&text);

            // Keep any trailing fragment for the next poll — a line may be
            // half-written when we read it.
            let mut consumed = 0;
            let chunk = std::mem::take(&mut self.partial);
            for line in chunk.split_inclusive('\n') {
                if !line.ends_with('\n') {
                    self.partial.push_str(line);
                    break;
                }
                consumed += 1;
                self.ingest(line);
            }
            let _ = consumed;
        }

        Ok(self.state())
    }

    fn ingest(&mut self, line: &str) {
        if let Some(ts) = str_after(line, "\"timestamp\":\"")
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        {
            let ts = ts.with_timezone(&Utc);
            if self.last_seen.map(|p| ts > p).unwrap_or(true) {
                self.last_seen = Some(ts);
            }
        }

        // Count every tool_use block on the line, and remember the last one.
        let mut rest = line;
        while let Some(i) = rest.find("\"type\":\"tool_use\"") {
            rest = &rest[i + 17..];
            let Some(name) = str_after(rest, "\"name\":\"") else {
                break;
            };
            match name.as_str() {
                "Bash" => self.tally.bash += 1,
                "Edit" | "NotebookEdit" => self.tally.edit += 1,
                "Write" => self.tally.write += 1,
                "WebSearch" | "WebFetch" => self.tally.web += 1,
                _ => {}
            }
            self.last_tool = Some(name);
        }
    }

    fn state(&self) -> u8 {
        let quiet = match self.last_seen {
            Some(t) => (Utc::now() - t).num_seconds() > QUIET_SECS,
            None => true,
        };
        if quiet {
            return ACT_IDLE;
        }
        match self.last_tool.as_deref() {
            Some("Bash") => ACT_RUN,
            Some("Edit") | Some("Write") | Some("NotebookEdit") => ACT_PAINT,
            Some("Read") | Some("Grep") | Some("Glob") | Some("WebSearch") | Some("WebFetch")
            | Some("ToolSearch") | Some("Task") => ACT_LOOK,
            // Mid-turn with no tool yet: Claude is composing a reply.
            Some(_) | None => ACT_PAINT,
        }
    }
}

fn newest_transcript() -> io::Result<Option<PathBuf>> {
    let Some(root) = crate::paths::claude_projects() else {
        return Ok(None);
    };

    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    let Ok(projects) = fs::read_dir(&root) else {
        return Ok(None);
    };

    for project in projects.flatten() {
        let Ok(entries) = fs::read_dir(project.path()) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(m) = e.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            if best.as_ref().map(|(_, bm)| m > *bm).unwrap_or(true) {
                best = Some((p, m));
            }
        }
    }

    Ok(best.map(|(p, _)| p))
}

fn str_after(hay: &str, key: &str) -> Option<String> {
    let i = hay.find(key)? + key.len();
    let rest = &hay[i..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub fn state_name(s: u8) -> &'static str {
    match s {
        ACT_IDLE => "idle/sleeping",
        ACT_RUN => "running (bash)",
        ACT_PAINT => "painting (edit/write)",
        ACT_LOOK => "looking (read/search)",
        _ => "none",
    }
}

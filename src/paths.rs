//! Where Claude Code keeps its data, per platform.

use std::path::PathBuf;

/// The user's home directory. `HOME` on Unix, `USERPROFILE` on Windows.
pub fn home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| {
                // Fall back to the split form some environments still set.
                match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
                    (Some(d), Some(p)) => {
                        let mut s = d;
                        s.push(p);
                        Some(s)
                    }
                    _ => None,
                }
            })
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// `~/.claude/projects`, where session transcripts live.
pub fn claude_projects() -> Option<PathBuf> {
    home().map(|h| h.join(".claude").join("projects"))
}

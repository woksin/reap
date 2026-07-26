use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Format a byte count in SI units, matching what macOS and `docker system df`
/// report. Figures shown here can be compared against those directly, which
/// would not hold for the 1024-based units `du -h` uses.
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1000.0 && unit < UNITS.len() - 1 {
        val /= 1000.0;
        unit += 1;
    }
    if val >= 100.0 {
        format!("{val:.0} {}", UNITS[unit])
    } else if val >= 10.0 {
        format!("{val:.1} {}", UNITS[unit])
    } else {
        format!("{val:.2} {}", UNITS[unit])
    }
}

/// Recursive directory size, parallelised at every level.
///
/// `read_dir` file types do not follow symlinks, so link farms and cycles
/// contribute nothing and cannot trap the walk.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(path) else {
        return 0;
    };
    let entries: Vec<_> = rd.flatten().collect();
    entries
        .par_iter()
        .map(|e| match e.file_type() {
            Ok(ft) if ft.is_dir() => dir_size(&e.path()),
            Ok(ft) if ft.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Size of a path whether it is a file or a directory.
pub fn path_size(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => dir_size(path),
        Ok(m) if m.is_file() => m.len(),
        _ => 0,
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Days since `path` was last modified.
pub fn age_days(path: &Path) -> Option<u64> {
    let mtime = fs::metadata(path).ok()?.modified().ok()?;
    let secs = mtime.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(now_secs().saturating_sub(secs) / 86_400)
}

/// Days since a unix timestamp.
pub fn days_since(unix_secs: u64) -> u64 {
    now_secs().saturating_sub(unix_secs) / 86_400
}

/// Compact human age, e.g. `3d`, `5w`, `14mo`.
pub fn human_age(days: u64) -> String {
    if days < 1 {
        "today".into()
    } else if days < 14 {
        format!("{days}d")
    } else if days < 60 {
        format!("{}w", days / 7)
    } else if days < 730 {
        format!("{}mo", days / 30)
    } else {
        format!("{}y", days / 365)
    }
}

/// Free and total bytes on the volume holding `path`, via `df`.
///
/// `statvfs` is not in std and this is queried once per run, so shelling out
/// costs nothing and keeps the dependency list short.
pub fn disk_free(path: &Path) -> Option<(u64, u64)> {
    let out = std::process::Command::new("df")
        .arg("-Pk")
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Filesystem  1024-blocks  Used  Available  Capacity  Mounted-on
    let fields: Vec<&str> = text.lines().nth(1)?.split_whitespace().collect();
    let total: u64 = fields.get(1)?.parse().ok()?;
    let available: u64 = fields.get(3)?.parse().ok()?;
    Some((available * 1024, total * 1024))
}

/// Shorten a path for display, replacing `$HOME` with `~`.
pub fn tilde(path: &Path) -> String {
    let s = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => {
            format!("~{}", &s[home.len()..])
        }
        _ => s,
    }
}

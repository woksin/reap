//! Persisted directory sizes, so a rescan does not re-walk millions of inodes.
//!
//! An entry is trusted only while the directory's own mtime is unchanged *and*
//! the measurement is recent. That mtime moves when direct children are added
//! or removed but not when a file deep inside is rewritten, so the check alone
//! would eventually go stale — hence the time limit as well. `--no-cache`
//! forces a fresh measurement.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How long a measurement stays trustworthy.
const MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Entry {
    size: u64,
    mtime: u64,
    measured_at: u64,
}

#[derive(Default)]
pub struct SizeCache {
    entries: Mutex<HashMap<PathBuf, Entry>>,
    enabled: bool,
    dirty: Mutex<bool>,
}

/// Where reap keeps things it can afford to lose.
pub fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::scan::home_dir().map(|h| h.join(".cache")))?;
    Some(base.join("reap"))
}

fn cache_path() -> Option<PathBuf> {
    cache_dir().map(|d| d.join("sizes.json"))
}

/// Directory mtime in nanoseconds.
///
/// Whole seconds would miss any change landing in the same second as the
/// measurement, which is exactly what happens during a build.
fn mtime_of(path: &Path) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok()?;
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        // Nanoseconds overflow a u64 in 2554. Saturating there keeps the value
        // ordered rather than wrapping it back to a plausible-looking time.
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

impl SizeCache {
    pub fn load(enabled: bool) -> Self {
        let entries = if enabled {
            cache_path()
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice::<HashMap<PathBuf, Entry>>(&b).ok())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        Self {
            entries: Mutex::new(entries),
            enabled,
            dirty: Mutex::new(false),
        }
    }

    /// Measured size of `path`, reusing a previous measurement when it still holds.
    pub fn size_of(&self, path: &Path) -> u64 {
        if !self.enabled {
            return crate::util::dir_size(path);
        }
        let now = crate::util::now_secs();
        let mtime = mtime_of(path);

        if let Some(mtime) = mtime
            && let Ok(entries) = self.entries.lock()
            && let Some(e) = entries.get(path)
            && e.mtime == mtime
            && now.saturating_sub(e.measured_at) < MAX_AGE_SECS
        {
            return e.size;
        }

        let size = crate::util::dir_size(path);
        if let Some(mtime) = mtime
            && let Ok(mut entries) = self.entries.lock()
        {
            entries.insert(
                path.to_path_buf(),
                Entry {
                    size,
                    mtime,
                    measured_at: now,
                },
            );
            if let Ok(mut d) = self.dirty.lock() {
                *d = true;
            }
        }
        size
    }

    /// Drop an entry whose directory we just deleted.
    pub fn forget(&self, path: &Path) {
        if let Ok(mut entries) = self.entries.lock()
            && entries.remove(path).is_some()
            && let Ok(mut d) = self.dirty.lock()
        {
            *d = true;
        }
    }

    pub fn save(&self) {
        if !self.enabled || !self.dirty.lock().is_ok_and(|d| *d) {
            return;
        }
        let Some(path) = cache_path() else { return };
        let Ok(entries) = self.entries.lock() else {
            return;
        };
        // Entries for directories that no longer exist would grow without bound.
        let live: HashMap<&PathBuf, &Entry> = entries.iter().filter(|(p, _)| p.exists()).collect();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec(&live) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("reap-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reuses_a_measurement_while_the_directory_is_untouched() {
        let root = scratch("hit");
        let target = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.bin"), vec![0u8; 4096]).unwrap();

        let cache = SizeCache::load(true);
        let first = cache.size_of(&target);
        assert_eq!(first, 4096);

        // Rewrite the file's contents without touching the directory entry:
        // the cached figure is deliberately kept.
        std::fs::write(target.join("a.bin"), vec![0u8; 8192]).unwrap();
        assert_eq!(
            cache.size_of(&target),
            first,
            "entry should still be reused"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remeasures_when_the_directory_changes() {
        let root = scratch("miss");
        let target = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.bin"), vec![0u8; 4096]).unwrap();

        let cache = SizeCache::load(true);
        assert_eq!(cache.size_of(&target), 4096);

        // Adding a child moves the directory's own mtime.
        std::fs::write(target.join("b.bin"), vec![0u8; 4096]).unwrap();
        assert_eq!(cache.size_of(&target), 8192, "must re-measure");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn disabled_cache_always_measures() {
        let root = scratch("off");
        let target = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.bin"), vec![0u8; 1024]).unwrap();

        let cache = SizeCache::load(false);
        assert_eq!(cache.size_of(&target), 1024);
        std::fs::write(target.join("a.bin"), vec![0u8; 2048]).unwrap();
        assert_eq!(cache.size_of(&target), 2048, "no cache means no reuse");

        std::fs::remove_dir_all(&root).ok();
    }
}

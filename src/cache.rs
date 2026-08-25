//! Persisted directory sizes, so a rescan does not re-walk millions of inodes.
//!
//! An entry is trusted only while the directory's own mtime is unchanged, its
//! direct entries are the same ones, *and* the measurement is recent. None of
//! the three catches everything on its own: the mtime does not move when a file
//! deep inside is rewritten, the entry list does not change when a child is
//! rewritten in place, and neither notices a slow drift — hence the time limit
//! as well. `--no-cache` forces a fresh measurement.
//!
//! The entry list is what makes this correct on Windows. NTFS updates a
//! directory's own timestamp lazily, so a child added moments after a
//! measurement can leave the mtime looking untouched, and a size cache that
//! trusted the mtime alone would report the old figure — in a tool whose whole
//! job is reporting sizes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

/// How long a measurement stays trustworthy.
// Deep rewrites need not move a candidate root's mtime or direct entry list.
// A one-day ceiling keeps a catalogue from carrying a week-old estimate while
// still avoiding repeated walks during an interactive session.
const MAX_AGE_SECS: u64 = 24 * 60 * 60;
/// Bumped whenever measurement semantics change. Version 1 deduplicates
/// hard-linked files and directory metadata when reporting allocated bytes.
const CACHE_ENTRY_VERSION: u8 = 2;

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Entry {
    #[serde(default)]
    version: u8,
    size: u64,
    mtime: u64,
    measured_at: u64,
    /// Absent in caches written before this was recorded, which is why it is an
    /// option rather than a bare number: an entry that never had one cannot be
    /// vouched for, so it is measured again once and written back complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entries: Option<u64>,
    /// Newest write found during the same traversal as `size`. Old cache files
    /// lack this and are deliberately measured once before scanners trust an
    /// age derived from them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    newest_mtime: Option<u64>,
    /// Host blocks occupied by the tree. Needed by inventory to handle sparse
    /// virtual disks without confusing apparent and physical size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocated_size: Option<u64>,
}

type Measurement = (u64, u64, Option<u64>);

struct PendingMeasurement {
    result: Mutex<Option<Measurement>>,
    ready: Condvar,
}

#[derive(Default)]
pub struct SizeCache {
    entries: Mutex<HashMap<PathBuf, Entry>>,
    /// Scanner categories overlap. Coalesce an exact-path miss so two workers
    /// never launch the same recursive traversal at the same time.
    in_flight: Mutex<HashMap<PathBuf, Arc<PendingMeasurement>>>,
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

/// A fingerprint of the directory's direct entries, by name.
///
/// One shallow `read_dir` — a rounding error against the recursive walk it
/// exists to avoid — and it changes exactly when a child is added, removed or
/// renamed. Names only: rewriting a file in place leaves the fingerprint alone,
/// which is the reuse this cache is for.
///
/// Combined with xor so the filesystem's iteration order cannot change the
/// answer. Names within one directory are unique, so nothing cancels out.
fn entries_of(path: &Path) -> Option<u64> {
    const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut combined: u64 = 0;
    for entry in std::fs::read_dir(path).ok()? {
        let name = entry.ok()?.file_name();
        let mut h = BASIS;
        for b in name.as_encoded_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(PRIME);
        }
        combined ^= h;
    }
    Some(combined)
}

impl SizeCache {
    pub fn load(enabled: bool) -> Self {
        let mut entries = if enabled {
            cache_path()
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice::<HashMap<PathBuf, Entry>>(&b).ok())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        let now = crate::util::now_secs();
        // Expired or old-format entries cannot produce a hit. Drop them once
        // while loading instead of carrying them through every lookup and
        // serialising them again at shutdown.
        entries.retain(|_, entry| {
            entry.version == CACHE_ENTRY_VERSION
                && entry.entries.is_some()
                && entry.newest_mtime.is_some()
                && entry.allocated_size.is_some()
                && now.saturating_sub(entry.measured_at) < MAX_AGE_SECS
        });
        Self {
            entries: Mutex::new(entries),
            in_flight: Mutex::new(HashMap::new()),
            enabled,
            dirty: Mutex::new(false),
        }
    }

    /// Measured size of `path`, retained as the narrow test-facing convenience.
    #[cfg(test)]
    pub fn size_of(&self, path: &Path) -> u64 {
        self.measure_full(path).0
    }

    /// Logical size and newest nested write from one cached traversal.
    pub fn measure(&self, path: &Path) -> (u64, Option<u64>) {
        let (logical, _, newest) = self.measure_full(path);
        (logical, newest)
    }

    pub fn allocated_size(&self, path: &Path) -> u64 {
        self.measure_full(path).1
    }

    fn cached_measurement(
        &self,
        path: &Path,
        now: u64,
        mtime: Option<u64>,
        listing: Option<u64>,
    ) -> Option<Measurement> {
        let mtime = mtime?;
        let entries = self.entries.lock().ok()?;
        let entry = *entries.get(path)?;
        drop(entries);
        if entry.version != CACHE_ENTRY_VERSION
            || entry.mtime != mtime
            || entry.entries.is_none()
            || entry.entries != listing
            || entry.newest_mtime.is_none()
            || entry.allocated_size.is_none()
            || now.saturating_sub(entry.measured_at) >= MAX_AGE_SECS
        {
            return None;
        }
        Some((entry.size, entry.allocated_size?, entry.newest_mtime))
    }

    fn complete_pending(
        &self,
        path: &Path,
        pending: &PendingMeasurement,
        measurement: Measurement,
    ) {
        if let Ok(mut result) = pending.result.lock() {
            *result = Some(measurement);
        }
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.remove(path);
        }
        pending.ready.notify_all();
    }

    fn measure_full(&self, path: &Path) -> Measurement {
        if !self.enabled {
            return crate::util::dir_measure_full(path);
        }
        let now = crate::util::now_secs();
        let mtime = mtime_of(path);
        let listing = entries_of(path);

        if let Some(measurement) = self.cached_measurement(path, now, mtime, listing) {
            return measurement;
        }

        let (pending, leader) = if let Ok(mut in_flight) = self.in_flight.lock() {
            if let Some(pending) = in_flight.get(path) {
                (Arc::clone(pending), false)
            } else {
                let pending = Arc::new(PendingMeasurement {
                    result: Mutex::new(None),
                    ready: Condvar::new(),
                });
                in_flight.insert(path.to_path_buf(), Arc::clone(&pending));
                (pending, true)
            }
        } else {
            // Poisoning must degrade to duplicate work, never make a scanner
            // silently report zero bytes.
            return crate::util::dir_measure_full(path);
        };

        if !leader {
            let Ok(mut result) = pending.result.lock() else {
                return crate::util::dir_measure_full(path);
            };
            loop {
                if let Some(measurement) = *result {
                    return measurement;
                }
                let Ok(next) = pending.ready.wait(result) else {
                    return crate::util::dir_measure_full(path);
                };
                result = next;
            }
        }

        // Another caller may have completed between our first cache check and
        // registration. Recheck after becoming leader so that narrow race does
        // not launch a second identical traversal.
        if let Some(measurement) = self.cached_measurement(path, now, mtime, listing) {
            self.complete_pending(path, &pending, measurement);
            return measurement;
        }

        let measurement @ (size, allocated_size, newest_mtime) =
            crate::util::dir_measure_full(path);
        if let Some(mtime) = mtime
            && let Ok(mut entries) = self.entries.lock()
        {
            entries.insert(
                path.to_path_buf(),
                Entry {
                    version: CACHE_ENTRY_VERSION,
                    size,
                    mtime,
                    measured_at: now,
                    entries: listing,
                    newest_mtime,
                    allocated_size: Some(allocated_size),
                },
            );
            if let Ok(mut dirty) = self.dirty.lock() {
                *dirty = true;
            }
        }
        self.complete_pending(path, &pending, measurement);
        measurement
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
        // Loading drops every entry too old to be reused, so the persisted map
        // is already bounded. Avoid thousands of serial `exists` syscalls here:
        // deleted paths are harmless and disappear on the next load.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec(&*entries) {
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

        std::fs::write(target.join("b.bin"), vec![0u8; 4096]).unwrap();
        assert_eq!(cache.size_of(&target), 8192, "must re-measure");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remeasures_when_a_child_appears_without_the_mtime_moving() {
        // The Windows case, and the reason the entry list is recorded at all.
        // NTFS updates a directory's own timestamp lazily, so a child added
        // just after a measurement can leave the mtime looking untouched. This
        // reproduces that on any filesystem by making the cache's record agree
        // with the directory's current timestamp, which is what a deferred
        // update looks like from in here.
        let root = scratch("lazy-mtime");
        let target = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.bin"), vec![0u8; 4096]).unwrap();

        let cache = SizeCache::load(true);
        assert_eq!(cache.size_of(&target), 4096);

        std::fs::write(target.join("b.bin"), vec![0u8; 4096]).unwrap();

        // Pretend the directory's timestamp never moved.
        let unmoved = mtime_of(&target).unwrap();
        {
            let mut entries = cache.entries.lock().unwrap();
            entries.get_mut(&target).unwrap().mtime = unmoved;
        }

        assert_eq!(
            cache.size_of(&target),
            8192,
            "an unchanged mtime must not vouch for a changed directory"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_entry_from_an_older_cache_is_measured_again() {
        // Written before the entry list was recorded, so there is nothing to
        // check it against and it cannot be trusted on its word alone.
        let root = scratch("legacy");
        let target = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.bin"), vec![0u8; 4096]).unwrap();

        let cache = SizeCache::load(true);
        cache.entries.lock().unwrap().insert(
            target.clone(),
            Entry {
                version: 0,
                size: 1,
                mtime: mtime_of(&target).unwrap(),
                measured_at: crate::util::now_secs(),
                entries: None,
                newest_mtime: None,
                allocated_size: None,
            },
        );

        assert_eq!(cache.size_of(&target), 4096, "must not trust the old shape");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn concurrent_misses_share_one_completed_measurement() {
        let root = scratch("concurrent");
        let target = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        for index in 0..100 {
            std::fs::write(target.join(format!("{index}.bin")), vec![0u8; 1024]).unwrap();
        }

        let cache = Arc::new(SizeCache::load(true));
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let barrier = Arc::clone(&barrier);
                let target = target.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    cache.size_of(&target)
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), 100 * 1024);
        }
        assert!(cache.in_flight.lock().unwrap().is_empty());

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

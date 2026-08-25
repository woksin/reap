use rayon::prelude::*;
#[cfg(unix)]
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Format a byte count in SI units, matching what macOS and `docker system df`
/// report. Figures shown here can be compared against those directly, which
/// would not hold for the 1024-based units `du -h` uses.
#[expect(
    clippy::cast_precision_loss,
    reason = "the mantissa runs out at 4 PB, well past the point where this \
              prints three significant figures and drops the rest anyway"
)]
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut unit = 0;
    // Step up while the figure would *print* as 1000 or more, not merely while
    // it is. Values from 100 up are shown without decimals, so 999_999 rounds
    // to "1000 kB" — four digits and a unit nobody would have chosen. Promoting
    // at 999.5 renders it "1.00 MB" instead.
    while val >= 999.5 && unit < UNITS.len() - 1 {
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
    dir_measure(path).0
}

/// Logical bytes and newest write in one traversal. Scanners need both size
/// and staleness evidence; walking a million-file package tree twice makes the
/// safer timestamp prohibitively expensive.
pub fn dir_measure(path: &Path) -> (u64, Option<u64>) {
    let (logical, _, newest) = dir_measure_full(path);
    (logical, newest)
}

#[derive(Default)]
struct PhysicalTracker {
    #[cfg(unix)]
    hard_links: Mutex<HashSet<(u64, u64)>>,
}

impl PhysicalTracker {
    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "the receiver carries the hard-link set on Unix; other platforms use file length"
        )
    )]
    fn allocated(&self, metadata: &fs::Metadata) -> u64 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1
                && self
                    .hard_links
                    .lock()
                    .is_ok_and(|mut seen| !seen.insert((metadata.dev(), metadata.ino())))
            {
                return 0;
            }
            metadata.blocks().saturating_mul(512)
        }
        #[cfg(not(unix))]
        {
            metadata.len()
        }
    }
}

/// Logical bytes, host-allocated bytes, and newest write in one traversal.
pub fn dir_measure_full(path: &Path) -> (u64, u64, Option<u64>) {
    dir_measure_tracked(path, &PhysicalTracker::default())
}

fn dir_measure_tracked(path: &Path, physical: &PhysicalTracker) -> (u64, u64, Option<u64>) {
    let modified = |metadata: &fs::Metadata| {
        metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs())
    };
    let own = fs::symlink_metadata(path).ok();
    let own_newest = own.as_ref().and_then(modified);
    let own_physical = own
        .as_ref()
        .map_or(0, |metadata| physical.allocated(metadata));
    let Ok(rd) = fs::read_dir(path) else {
        return (0, own_physical, own_newest);
    };
    let entries: Vec<_> = rd.flatten().collect();
    let (logical, physical, child_newest) = entries
        .par_iter()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_measure_tracked(&entry.path(), physical),
            Ok(kind) if kind.is_file() => entry.metadata().map_or((0, 0, None), |metadata| {
                (
                    metadata.len(),
                    physical.allocated(&metadata),
                    modified(&metadata),
                )
            }),
            _ => (0, 0, None),
        })
        .reduce(
            || (0, 0, None),
            |(left_logical, left_physical, left_newest),
             (right_logical, right_physical, right_newest)| {
                (
                    left_logical.saturating_add(right_logical),
                    left_physical.saturating_add(right_physical),
                    left_newest
                        .max(right_newest)
                        .or(left_newest)
                        .or(right_newest),
                )
            },
        );
    (
        logical,
        physical.saturating_add(own_physical),
        own_newest.max(child_newest).or(own_newest).or(child_newest),
    )
}

/// The newest modification time anywhere under `path`, including `path` itself.
///
/// A directory's own mtime answers a narrower question than it looks like it
/// does: it moves when an entry is added, removed or renamed, and not when one
/// is written to. A process appending to a file it already created leaves every
/// directory above it untouched — so a tree can be under active use and still
/// carry a timestamp from the day it was made.
///
/// That distinction does not matter for build output, which is rewritten
/// wholesale. It matters a great deal for anything appended to a line at a
/// time, which is how every agent writes a transcript and how every long
/// running job writes a log.
///
/// Returned in whole seconds since the epoch. `None` means nothing here could
/// be read at all, which is never taken as evidence that nothing was written.
pub fn newest_mtime(path: &Path) -> Option<u64> {
    let secs = |m: &fs::Metadata| {
        m.modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    };

    let own = fs::symlink_metadata(path).ok()?;
    if !own.is_dir() {
        return secs(&own);
    }

    let entries: Option<Vec<_>> = fs::read_dir(path).ok()?.map(Result::ok).collect();
    let nested: Option<Vec<Option<u64>>> = entries?
        .par_iter()
        .map(|entry| {
            let kind = entry.file_type().ok()?;
            // As with `dir_size`: `read_dir` file types do not follow symlinks,
            // so links contribute nothing and cycles cannot trap this walk.
            if kind.is_dir() {
                Some(Some(newest_mtime(&entry.path())?))
            } else if kind.is_file() {
                let metadata = entry.metadata().ok()?;
                Some(Some(secs(&metadata)?))
            } else {
                Some(None)
            }
        })
        .collect();
    let deepest = nested?.into_iter().flatten().max();

    Some(secs(&own)?.max(deepest.unwrap_or(0)))
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
        .map_or(0, |d| d.as_secs())
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

/// Move `idx` by `delta`, clamped to `0..=max`.
///
/// Kept in `usize` with saturating steps rather than routed through `isize`, so
/// the extremes the key handlers use for Home and End arrive at the ends of the
/// list instead of wrapping through a signed conversion on the way.
pub fn offset(idx: usize, delta: isize, max: usize) -> usize {
    if delta < 0 {
        idx.saturating_sub(delta.unsigned_abs())
    } else {
        idx.saturating_add(delta.unsigned_abs()).min(max)
    }
}

/// Narrow a count to the `u16` terminal geometry is expressed in.
///
/// Saturating rather than wrapping. Nothing here should ever reach 65535 rows,
/// but if a list somehow did, clamping leaves it merely taller than the screen
/// instead of wrapping round to a handful of rows and drawing a corrupt frame.
pub fn rows(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

#[cfg(not(windows))]
pub(crate) struct DfLine {
    pub filesystem: String,
    pub total_kb: u64,
    pub available_kb: u64,
    pub mount: PathBuf,
}

#[cfg(not(windows))]
pub(crate) fn parse_df_line(line: &str) -> Option<DfLine> {
    fn take(input: &str) -> Option<(&str, &str)> {
        let input = input.trim_start();
        let end = input.find(char::is_whitespace).unwrap_or(input.len());
        (!input.is_empty()).then_some((&input[..end], &input[end..]))
    }

    let (filesystem, rest) = take(line)?;
    let (total, rest) = take(rest)?;
    let (_, rest) = take(rest)?; // used
    let (available, rest) = take(rest)?;
    let (_, rest) = take(rest)?; // capacity
    let mount = rest.trim();
    if mount.is_empty() {
        return None;
    }
    Some(DfLine {
        filesystem: filesystem.to_string(),
        total_kb: total.parse().ok()?,
        available_kb: available.parse().ok()?,
        mount: PathBuf::from(mount),
    })
}

#[derive(Clone, Debug)]
pub struct DiskStat {
    pub mount: PathBuf,
    /// Stable storage-pool identity: APFS container on macOS, filesystem
    /// device elsewhere, drive root on Windows.
    pub pool: String,
    pub free: u64,
    pub total: u64,
}

impl DiskStat {
    pub fn shares_pool(&self, other: &Self) -> bool {
        self.pool == other.pool
    }
}

#[cfg(not(windows))]
fn df_line(path: &Path) -> Option<DfLine> {
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
    parse_df_line(text.lines().nth(1)?)
}

/// Filesystem and capacity information for the volume holding `path`.
///
/// `statvfs` is not in std and this is queried a handful of times per run, so
/// shelling out to `df` costs nothing and keeps the dependency list short.
#[cfg(not(windows))]
fn disk_stat_fresh(path: &Path) -> Option<DiskStat> {
    let row = df_line(path)?;
    let mount = row.mount;
    #[cfg(target_os = "macos")]
    let pool = apfs_container(&mount).unwrap_or_else(|| row.filesystem.clone());
    #[cfg(all(unix, not(target_os = "macos")))]
    let pool = {
        use std::os::unix::fs::MetadataExt;
        let device = fs::metadata(&mount).ok()?.dev();
        format!("dev:{device}")
    };
    #[cfg(all(not(unix), not(windows)))]
    let pool = row.filesystem.clone();
    Some(DiskStat {
        mount,
        pool,
        free: row.available_kb.saturating_mul(1024),
        total: row.total_kb.saturating_mul(1024),
    })
}

#[cfg(any(target_os = "macos", test))]
fn parse_apfs_container(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("APFS Container Reference:")
                .or_else(|| line.strip_prefix("APFS Container:"))
        })
        .map(|container| format!("apfs:{}", container.trim()))
}

#[cfg(target_os = "macos")]
fn apfs_container(mount: &Path) -> Option<String> {
    let output = std::process::Command::new("diskutil")
        .arg("info")
        .arg(mount)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_apfs_container(&String::from_utf8_lossy(&output.stdout))
}

/// Mount point, free bytes, and total bytes without resolving storage-pool
/// identity. On macOS this deliberately avoids `diskutil`.
#[cfg(not(windows))]
pub fn disk_capacity(path: &Path) -> Option<(PathBuf, u64, u64)> {
    let row = df_line(path)?;
    Some((
        row.mount,
        row.available_kb.saturating_mul(1024),
        row.total_kb.saturating_mul(1024),
    ))
}

/// Free and total bytes on the volume holding `path`.
#[cfg(not(windows))]
pub fn disk_free(path: &Path) -> Option<(u64, u64)> {
    disk_capacity(path).map(|(_, free, total)| (free, total))
}

/// Free and total bytes on the volume holding `path`.
///
/// There is no `df` on Windows and the shell alternatives are either deprecated
/// (`wmic`) or print numbers in whatever the machine's locale does, so this asks
/// the kernel directly. `GetDiskFreeSpaceExW` takes three out-pointers and no
/// structure, which makes the declaration unambiguous — the reason it is worth
/// doing here and not for anything more elaborate.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows reports free space only through Win32; there is no safe \
              std equivalent, and the call is justified at its SAFETY comment"
)]
pub fn disk_free(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;

    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_available_to_caller: *mut u64,
            total_bytes: *mut u64,
            total_free_bytes: *mut u64,
        ) -> i32;
    }

    // The call needs a directory that exists; a candidate's path may already
    // have been reaped by the time free space is measured again.
    let mut dir = path;
    while !dir.is_dir() {
        dir = dir.parent()?;
    }

    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call, and
    // the three out-parameters are distinct, initialised, correctly sized locals.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &raw mut available,
            &raw mut total,
            &raw mut free,
        )
    };
    (ok != 0).then_some((available, total))
}

#[cfg(windows)]
pub fn disk_capacity(path: &Path) -> Option<(PathBuf, u64, u64)> {
    let (free, total) = disk_free(path)?;
    let mount = fs::canonicalize(existing_ancestor(path)?)
        .ok()?
        .ancestors()
        .last()?
        .to_path_buf();
    Some((mount, free, total))
}

#[cfg(windows)]
fn disk_stat_fresh(path: &Path) -> Option<DiskStat> {
    let (mount, free, total) = disk_capacity(path)?;
    let pool = mount.to_string_lossy().to_lowercase();
    Some(DiskStat {
        mount,
        pool,
        free,
        total,
    })
}

fn existing_ancestor(mut path: &Path) -> Option<&Path> {
    loop {
        if path.exists() {
            return Some(path);
        }
        path = path.parent()?;
    }
}

#[cfg(unix)]
fn volume_key(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let device = fs::metadata(existing_ancestor(path)?).ok()?.dev();
    Some(format!("dev:{device}"))
}

#[cfg(windows)]
fn volume_key(path: &Path) -> Option<String> {
    let path = fs::canonicalize(existing_ancestor(path)?).ok()?;
    Some(path.ancestors().last()?.to_string_lossy().to_lowercase())
}

#[cfg(all(not(unix), not(windows)))]
fn volume_key(path: &Path) -> Option<String> {
    fs::canonicalize(existing_ancestor(path)?)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// Whether two paths are on the same mounted volume. This is a metadata check,
/// unlike storage-pool resolution, which may need a platform subprocess.
pub fn shares_volume(left: &Path, right: &Path) -> bool {
    volume_key(left)
        .zip(volume_key(right))
        .is_some_and(|(left, right)| left == right)
}

fn disk_stat_cache() -> &'static Mutex<std::collections::HashMap<String, DiskStat>> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, DiskStat>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Forget the per-run storage snapshot before an explicit rescan.
pub fn clear_disk_stat_cache() {
    if let Ok(mut cache) = disk_stat_cache().lock() {
        cache.clear();
    }
}

/// Filesystem and capacity information for the volume holding `path`.
///
/// A scan can produce thousands of paths but generally only one or two host
/// volumes. Resolve each volume once: on macOS this also turns one `diskutil`
/// process per finding into one process per mounted volume.
pub fn disk_stat(path: &Path) -> Option<DiskStat> {
    let key = volume_key(path)?;
    let mut cache = disk_stat_cache().lock().ok()?;
    if let Some(stat) = cache.get(&key) {
        return Some(stat.clone());
    }
    let stat = disk_stat_fresh(path)?;
    cache.insert(key, stat.clone());
    drop(cache);
    Some(stat)
}

/// Shorten a path for display, replacing the home directory with `~`.
pub fn tilde(path: &Path) -> String {
    let s = path.display().to_string();
    match crate::scan::home_dir() {
        Some(home) => {
            let home = home.display().to_string();
            match s.strip_prefix(&home) {
                Some(rest) if !home.is_empty() => format!("~{rest}"),
                _ => s,
            }
        }
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Open a file *or a directory* in a way that permits setting its times.
    ///
    /// The two platforms disagree about what that takes and both refuse the
    /// other's answer. `futimens` works through a read-only descriptor, and a
    /// directory cannot be opened any other way on unix; Windows wants the
    /// attribute-write right named explicitly and will not open a directory at
    /// all without backup semantics. This test ages directories, so a plain
    /// `File::open` passes everywhere it is developed and fails in CI.
    #[cfg(unix)]
    fn open_to_set_times(path: &Path) -> std::io::Result<fs::File> {
        fs::File::open(path)
    }

    #[cfg(windows)]
    fn open_to_set_times(path: &Path) -> std::io::Result<fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

        fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }

    #[test]
    fn the_newest_write_is_found_where_no_directory_timestamp_records_it() {
        // The case the whole function exists for. A process appending to a file
        // it already created leaves every directory above it untouched, so a
        // tree in active use keeps the timestamp of the day it was made. Read
        // that timestamp and a conversation happening right now is reported as
        // months idle.
        let root =
            std::env::temp_dir().join(format!("reap-newest-{}-{}", std::process::id(), line!()));
        let deep = root.join("projects/one");
        fs::create_dir_all(&deep).unwrap();
        let transcript = deep.join("session.jsonl");
        fs::write(&transcript, b"{}\n").unwrap();

        // Age everything, deepest first, the way a finished store looks.
        let long_ago = SystemTime::now() - Duration::from_secs(90 * 86_400);
        for path in [transcript.as_path(), deep.as_path(), root.as_path()] {
            open_to_set_times(path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        assert_eq!(
            age_days(&root),
            Some(90),
            "the fixture must start out looking finished"
        );

        // Now append, exactly as a resumed session does.
        let now = SystemTime::now();
        open_to_set_times(&transcript)
            .unwrap()
            .set_modified(now)
            .unwrap();

        assert_eq!(
            age_days(&root),
            Some(90),
            "the directory's own timestamp is expected to miss this"
        );
        let newest = newest_mtime(&root).expect("a readable tree");
        assert!(
            now_secs().saturating_sub(newest) < 60,
            "the append must be found: {} seconds old",
            now_secs().saturating_sub(newest)
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    #[cfg(not(windows))]
    fn df_mount_paths_keep_embedded_whitespace() {
        let row = parse_df_line("/dev/disk9s1 100000 25000 75000 25% /media/me/My Source Disk")
            .expect("a df data row");
        assert_eq!(row.filesystem, "/dev/disk9s1");
        assert_eq!(row.total_kb, 100_000);
        assert_eq!(row.available_kb, 75_000);
        assert_eq!(row.mount, PathBuf::from("/media/me/My Source Disk"));
    }

    #[test]
    fn both_diskutil_apfs_container_labels_are_understood() {
        assert_eq!(
            parse_apfs_container("   APFS Container Reference: disk3\n"),
            Some("apfs:disk3".into())
        );
        assert_eq!(
            parse_apfs_container("   APFS Container:            disk4\n"),
            Some("apfs:disk4".into())
        );
    }

    #[test]
    fn storage_pools_are_identified_by_device_or_container_not_capacity_coincidence() {
        let first = DiskStat {
            mount: PathBuf::from("/first"),
            pool: "device-a".into(),
            free: 50,
            total: 100,
        };
        let same_pool = DiskStat {
            mount: PathBuf::from("/second"),
            pool: "device-a".into(),
            free: 1,
            total: 2,
        };
        let coincidental_capacity = DiskStat {
            mount: PathBuf::from("/third"),
            pool: "device-b".into(),
            free: 50,
            total: 100,
        };
        assert!(first.shares_pool(&same_pool));
        assert!(!first.shares_pool(&coincidental_capacity));
    }

    #[test]
    fn an_unreadable_path_reports_nothing_rather_than_a_time() {
        // Not being able to tell must never come back as "nothing was written",
        // which is the answer that would make a store look finished.
        assert_eq!(newest_mtime(Path::new("/nonexistent/reap/tree")), None);
    }

    #[test]
    #[cfg(unix)]
    fn a_partly_unreadable_tree_reports_no_activity_verdict() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "reap-unreadable-{}-{}",
            std::process::id(),
            line!()
        ));
        let hidden = root.join("hidden");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(hidden.join("active.jsonl"), "history").unwrap();
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o0)).unwrap();

        let result = newest_mtime(&root);
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&root).ok();
        assert_eq!(result, None, "a partial walk must fail closed");
    }

    #[test]
    #[cfg(unix)]
    fn inventory_counts_hard_linked_blocks_once() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!("reap-hardlink-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.bin");
        fs::write(&first, vec![0u8; 4096]).unwrap();
        fs::hard_link(&first, root.join("second.bin")).unwrap();
        let one_file = fs::metadata(&first).unwrap().blocks().saturating_mul(512);
        let directory = fs::metadata(&root).unwrap().blocks().saturating_mul(512);
        let (logical, physical, _) = dir_measure_full(&root);
        assert_eq!(logical, 8192);
        assert_eq!(physical, one_file + directory);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[cfg(unix)]
    fn inventory_counts_allocated_blocks_not_a_sparse_files_apparent_length() {
        let root = std::env::temp_dir().join(format!("reap-sparse-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sparse = fs::File::create(root.join("disk.raw")).unwrap();
        sparse.set_len(1_000_000_000_000).unwrap();
        assert!(dir_size(&root) >= 1_000_000_000_000);
        assert!(dir_measure_full(&root).1 < 10_000_000);
        fs::remove_dir_all(root).ok();
    }
}

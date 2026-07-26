//! Moving things to the trash rather than unlinking them.
//!
//! macOS keeps one trash per volume: `~/.Trash` for the boot volume, and
//! `<mount>/.Trashes/<uid>` for every other. A rename only works within a
//! single filesystem, so the right directory has to be chosen by device id —
//! `/Volumes/sourcecode` and `/` may share an APFS container and still be
//! separate filesystems as far as `rename(2)` is concerned.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The outermost directory reachable from `path` without crossing a filesystem.
fn mount_point(path: &Path) -> io::Result<PathBuf> {
    let dev = fs::symlink_metadata(path)?.dev();
    let mut best = path.to_path_buf();
    let mut cur = path.to_path_buf();
    while let Some(parent) = cur.parent().map(Path::to_path_buf) {
        match fs::symlink_metadata(&parent) {
            Ok(m) if m.dev() == dev => {
                best = parent.clone();
                cur = parent;
            }
            _ => break,
        }
    }
    Ok(best)
}

fn uid() -> u32 {
    crate::scan::home_dir()
        .and_then(|h| fs::metadata(h).ok())
        .map(|m| m.uid())
        .unwrap_or(0)
}

/// The trash directory that `path` can be renamed into.
///
/// macOS: `~/.Trash`, or `<mount>/.Trashes/<uid>` on any other volume.
#[cfg(target_os = "macos")]
pub fn trash_dir_for(path: &Path) -> io::Result<PathBuf> {
    let dev = fs::symlink_metadata(path)?.dev();

    if let Some(home) = crate::scan::home_dir() {
        let home_trash = home.join(".Trash");
        if let Ok(m) = fs::metadata(&home_trash)
            && m.dev() == dev
        {
            return Ok(home_trash);
        }
    }

    let dir = mount_point(path)?.join(".Trashes").join(uid().to_string());
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Freedesktop trash: `$XDG_DATA_HOME/Trash/files`, or `<mount>/.Trash-<uid>/files`
/// on another filesystem.
#[cfg(not(target_os = "macos"))]
pub fn trash_dir_for(path: &Path) -> io::Result<PathBuf> {
    let dev = fs::symlink_metadata(path)?.dev();

    let home_trash = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::scan::home_dir().map(|h| h.join(".local/share")))
        .map(|d| d.join("Trash"));

    if let Some(base) = home_trash {
        let files = base.join("files");
        // Compare against the parent, which exists even when the trash does not.
        let probe = if files.exists() {
            files.clone()
        } else {
            base.clone()
        };
        let same_fs = fs::metadata(&probe)
            .or_else(|_| fs::metadata(probe.parent().unwrap_or(&probe)))
            .map(|m| m.dev() == dev)
            .unwrap_or(false);
        if same_fs {
            fs::create_dir_all(&files)?;
            fs::create_dir_all(base.join("info"))?;
            return Ok(files);
        }
    }

    let base = mount_point(path)?.join(format!(".Trash-{}", uid()));
    let files = base.join("files");
    fs::create_dir_all(&files)?;
    fs::create_dir_all(base.join("info"))?;
    Ok(files)
}

/// Record the original location so a desktop file manager can restore it.
#[cfg(not(target_os = "macos"))]
fn write_trashinfo(files_dir: &Path, name: &str, original: &Path) {
    let Some(base) = files_dir.parent() else {
        return;
    };
    let info = base.join("info").join(format!("{name}.trashinfo"));
    // Percent-encode the few characters the spec requires.
    let mut encoded = String::new();
    for byte in original.to_string_lossy().bytes() {
        match byte {
            b'/' | b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            b if b.is_ascii_alphanumeric() => encoded.push(b as char),
            b => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    let body = format!(
        "[Trash Info]\nPath={encoded}\nDeletionDate={}\n",
        now_iso8601()
    );
    let _ = fs::write(info, body);
}

#[cfg(not(target_os = "macos"))]
fn now_iso8601() -> String {
    // Days-since-epoch to a civil date, so no date crate is needed for one field.
    let secs = crate::util::now_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (mut y, mut d) = (1970i64, days as i64);
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    while m < 12 && d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!(
        "{y:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        m + 1,
        d + 1,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Move `path` into its volume's trash, returning where it landed.
///
/// Note this does not reclaim any space — the bytes come back when the trash is
/// emptied. The caller is responsible for saying so.
pub fn move_to_trash(path: &Path) -> io::Result<PathBuf> {
    let dir = trash_dir_for(path)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();

    let mut dest = dir.join(&name);
    let mut n = 1;
    // Trash entries collide constantly — every project has a `node_modules`.
    while dest.exists() {
        dest = dir.join(format!("{name} {n}"));
        n += 1;
        if n > 9999 {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "no free name in trash",
            ));
        }
    }

    fs::rename(path, &dest)?;

    #[cfg(not(target_os = "macos"))]
    {
        let final_name = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(name);
        write_trashinfo(&dir, &final_name, path);
    }

    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_a_trash_on_the_same_filesystem() {
        let dir = std::env::temp_dir().join(format!("reap-trash-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("victim");
        fs::create_dir_all(&target).unwrap();

        let trash = trash_dir_for(&target).expect("a trash directory");
        let tdev = fs::metadata(&trash).unwrap().dev();
        let vdev = fs::metadata(&target).unwrap().dev();
        assert_eq!(tdev, vdev, "trash must be renameable-into from the target");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn moves_the_directory_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("reap-trash-mv-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // Unique per test: these all share the real trash directory.
        let target = dir.join(format!("reap-move-{}", std::process::id()));
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("artifact.bin"), b"payload").unwrap();

        let dest = move_to_trash(&target).expect("move to trash");
        assert!(!target.exists(), "original must be gone");
        assert!(dest.join("artifact.bin").exists(), "contents must survive");

        fs::remove_dir_all(&dest).ok();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collisions_get_a_distinct_name() {
        let dir = std::env::temp_dir().join(format!("reap-trash-dup-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let mut dests = Vec::new();
        let name = format!("reap-dup-{}", std::process::id());
        for _ in 0..2 {
            let target = dir.join(&name);
            fs::create_dir_all(&target).unwrap();
            dests.push(move_to_trash(&target).expect("move to trash"));
        }
        assert_ne!(
            dests[0], dests[1],
            "second entry must not overwrite the first"
        );

        for d in dests {
            fs::remove_dir_all(&d).ok();
        }
        fs::remove_dir_all(&dir).ok();
    }
}

//! The disk a person fills up, as opposed to the one a build system does.
//!
//! Everything the other scanners find has an owner that will remake it — a
//! compiler, a package manager, a daemon. Nothing here does. A file in
//! Downloads is either an installer for something already installed, or it is
//! the only copy of something, and reap has no way to tell those apart by
//! looking at the bytes.
//!
//! So it does not try. Installer-shaped names are grouped to make review easier,
//! but a suffix cannot prove a file is downloadable: a custom ISO or a locally
//! built executable may be the only copy. Every personal file therefore stays
//! irreversible unless the user explicitly re-grades it.

use super::ScanOpts;
use crate::model::{Action, Candidate, Category, Risk, ScanEvent};
use crate::util::{age_days, path_size, tilde};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Extensions that make an entry useful to review as an installer.
///
/// This changes only its group and explanation, never its risk. An extension is
/// not evidence that another copy still exists.
const INSTALLER_EXTENSIONS: &[&str] = &[
    "dmg",
    "pkg",
    "iso",
    "msi",
    "msix",
    "msixbundle",
    "exe",
    "deb",
    "rpm",
    "appimage",
];

pub fn scan(opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let _ = tx.send(ScanEvent::Status("scanning personal files".into()));

    if let Some(dir) = downloads_dir() {
        downloads(&dir, opts, tx);
    }
    device_backups(&backup_roots(), opts, tx);
}

/// The user's download directory.
///
/// Linux resolves the XDG user directory, Windows asks the Known Folder API, and
/// other platforms fall back to `~/Downloads`. A direct `XDG_DOWNLOAD_DIR`
/// environment value wins everywhere for scripts and unusual sessions.
pub fn downloads_dir() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("XDG_DOWNLOAD_DIR") {
        let path = PathBuf::from(configured);
        if usable_download_dir(&path) {
            return Some(path);
        }
    }

    let home = super::home_dir()?;
    platform_downloads_dir(&home)
        .filter(|path| usable_download_dir(path))
        .or_else(|| {
            let fallback = home.join("Downloads");
            usable_download_dir(&fallback).then_some(fallback)
        })
}

/// Treating the home directory itself as Downloads would put every top-level
/// personal file in front of a delete key. XDG uses `$HOME` to mean that a user
/// directory is disabled, so reject that value explicitly.
fn usable_download_dir(path: &Path) -> bool {
    if !path.is_dir() || !path.is_absolute() {
        return false;
    }
    let Some(home) = super::home_dir() else {
        return true;
    };
    if path == home {
        return false;
    }
    match (std::fs::canonicalize(path), std::fs::canonicalize(home)) {
        (Ok(path), Ok(home)) => path != home,
        _ => true,
    }
}

#[cfg(target_os = "linux")]
fn platform_downloads_dir(home: &Path) -> Option<PathBuf> {
    let from_command = std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let raw = String::from_utf8_lossy(&output.stdout);
            let path = PathBuf::from(raw.trim());
            (!path.as_os_str().is_empty()).then_some(path)
        });
    from_command.or_else(|| {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("user-dirs.dirs");
        let body = std::fs::read_to_string(config).ok()?;
        parse_xdg_download_dir(&body, home)
    })
}

#[cfg(target_os = "linux")]
fn parse_xdg_download_dir(body: &str, home: &Path) -> Option<PathBuf> {
    let value = body
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("XDG_DOWNLOAD_DIR="))?
        .trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    let rest = value
        .strip_prefix("$HOME")
        .or_else(|| value.strip_prefix("${HOME}"));
    if let Some(rest) = rest {
        return Some(home.join(rest.trim_start_matches('/')));
    }
    (!value.contains('$')).then(|| PathBuf::from(value))
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows exposes a redirected Downloads directory through the \
              Known Folder API; the allocation and UTF-16 reads are justified \
              at their SAFETY comments"
)]
fn platform_downloads_dir(_home: &Path) -> Option<PathBuf> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const DOWNLOADS: Guid = Guid {
        data1: 0x374d_e290,
        data2: 0x123f,
        data3: 0x4565,
        data4: [0x91, 0x64, 0x39, 0xc4, 0x92, 0x5e, 0x46, 0x7b],
    };

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            folder: *const Guid,
            flags: u32,
            token: *mut c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(memory: *const c_void);
    }

    let downloads = DOWNLOADS;
    let mut raw = std::ptr::null_mut();
    // SAFETY: `DOWNLOADS` is the documented FOLDERID_Downloads GUID, the token
    // is null for the current user, and `raw` is a valid out-pointer.
    let result = unsafe {
        SHGetKnownFolderPath(&raw const downloads, 0, std::ptr::null_mut(), &raw mut raw)
    };
    if result < 0 || raw.is_null() {
        return None;
    }

    let mut len = 0;
    // SAFETY: a successful call returns a NUL-terminated UTF-16 allocation.
    unsafe {
        while *raw.add(len) != 0 {
            len += 1;
        }
    }
    // SAFETY: the preceding loop found the terminator inside that allocation.
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, len)
    }));
    // SAFETY: the API documents this allocation as owned by CoTaskMemFree.
    unsafe { CoTaskMemFree(raw.cast()) };
    Some(path)
}

#[cfg(not(any(target_os = "linux", windows)))]
const fn platform_downloads_dir(_home: &Path) -> Option<PathBuf> {
    None
}

/// Is this the name of something that was installed rather than authored?
pub fn is_installer(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .is_some_and(|ext| INSTALLER_EXTENSIONS.contains(&ext.as_str()))
}

/// Top-level entries in Downloads that are big enough and old enough to matter.
///
/// Only the top level: a directory someone made inside Downloads is a decision
/// they took about how to keep things, and taking it apart entry by entry would
/// bury the actual finding under its own contents.
pub fn downloads(dir: &Path, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let floor = opts.min_size.max(opts.rules.downloads_floor);

    let entries: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            // Hidden entries here are the browser's bookkeeping, not the
            // user's files.
            !p.file_name()
                .is_none_or(|n| n.to_string_lossy().starts_with('.'))
        })
        .collect();

    entries.par_iter().for_each_with(tx.clone(), |tx, path| {
        let Some(age) = age_days(path) else { return };
        if age < opts.stale_days {
            return;
        }
        let size = if path.is_dir() {
            opts.cache.size_of(path)
        } else {
            path_size(path)
        };
        if size < floor {
            return;
        }

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let (group, detail) = if is_installer(&name) {
            (
                "installers",
                "looks like an installer, but reap cannot prove another copy exists",
            )
        } else {
            (
                "downloads",
                "reap cannot tell whether this exists anywhere else",
            )
        };

        let cand = Candidate::new(
            Category::Personal,
            group,
            name,
            format!("{} · {detail}", tilde(path)),
            size,
            Risk::Danger,
            Action::Remove(path.clone()),
        )
        .with_age(Some(age));
        super::emit(tx, opts, cand);
    });
}

/// Where phone and tablet backups pile up, per platform.
fn backup_roots() -> Vec<PathBuf> {
    let Some(home) = super::home_dir() else {
        return Vec::new();
    };
    [
        // macOS, both the Finder and the old iTunes location.
        "Library/Application Support/MobileSync/Backup",
        // Windows: iTunes from the installer, then from the Microsoft Store.
        "AppData/Roaming/Apple Computer/MobileSync/Backup",
        "Apple/MobileSync/Backup",
    ]
    .iter()
    .map(|p| home.join(p))
    .filter(|p| p.is_dir())
    .collect()
}

/// One candidate per device backup.
///
/// Always irreversible. A backup is the only copy of a phone that may itself no
/// longer exist, and it is routinely the largest single thing on the disk — the
/// two facts that make it worth showing and worth never selecting by default.
pub fn device_backups(roots: &[PathBuf], opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let backups: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flat_map(Iterator::flatten)
        .filter(|e| e.file_type().is_ok_and(|f| f.is_dir()))
        .map(|e| e.path())
        .collect();

    backups.par_iter().for_each_with(tx.clone(), |tx, path| {
        let size = opts.cache.size_of(path);
        if size < opts.min_size {
            return;
        }
        let age = age_days(path);
        let cand = Candidate::new(
            Category::Personal,
            "device backups",
            device_name(path),
            format!(
                "{} · a full backup of a device · nothing else holds this",
                tilde(path)
            ),
            size,
            Risk::Danger,
            Action::Remove(path.clone()),
        )
        .with_age(age);
        super::emit(tx, opts, cand);
    });
}

/// What to call a backup directory.
///
/// The directory is named after the device's identifier, which tells nobody
/// anything. `Info.plist` alongside it holds the name the owner gave the device,
/// and reading one line out of it turns "00008030-001..." into "Sara's iPhone".
fn device_name(path: &Path) -> String {
    let fallback = || {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let Ok(plist) = std::fs::read_to_string(path.join("Info.plist")) else {
        return fallback();
    };
    // <key>Device Name</key>\n<string>Sara's iPhone</string>
    let Some(after) = plist.split("<key>Device Name</key>").nth(1) else {
        return fallback();
    };
    let Some(open) = after.find("<string>") else {
        return fallback();
    };
    let rest = &after[open + "<string>".len()..];
    match rest.find("</string>") {
        Some(close) if close > 0 => rest[..close].to_string(),
        _ => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installers_are_recognised_by_extension_whatever_its_case() {
        for name in [
            "Xcode_15.dmg",
            "Setup.EXE",
            "ubuntu-24.04.iso",
            "node-v22.pkg",
            "app.AppImage",
        ] {
            assert!(is_installer(name), "{name} should read as an installer");
        }
    }

    #[test]
    fn anything_that_might_be_the_only_copy_is_not_an_installer() {
        // These land on the irreversible side, which is the whole safety
        // property of this scanner.
        for name in [
            "wedding.mov",
            "tax-return-2024.pdf",
            "portfolio.sketch",
            "archive.zip",
            "notes",
            "backup.tar.gz",
        ] {
            assert!(!is_installer(name), "{name} must not read as an installer");
        }
    }

    #[test]
    fn a_backup_is_named_after_the_device_not_its_identifier() {
        let dir = std::env::temp_dir().join(format!("reap-backup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Info.plist"),
            "<plist><dict>\n<key>Device Name</key>\n<string>Sara's iPhone</string>\n</dict></plist>",
        )
        .unwrap();

        assert_eq!(device_name(&dir), "Sara's iPhone");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_backup_with_nothing_readable_falls_back_to_its_directory_name() {
        let dir = std::env::temp_dir().join(format!("reap-backup-bare-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(device_name(&dir).starts_with("reap-backup-bare-"));

        // And a plist that does not hold the key must not produce an empty label.
        std::fs::write(dir.join("Info.plist"), "<plist><dict/></plist>").unwrap();
        assert!(!device_name(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_linux_download_directory_is_read_from_user_dirs_configuration() {
        let home = Path::new("/home/me");
        let body = "XDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Incoming\"\n";
        assert_eq!(
            parse_xdg_download_dir(body, home),
            Some(home.join("Incoming"))
        );
    }
}

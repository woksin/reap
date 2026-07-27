//! A download directory, and the device backups beside it.
//!
//! Files are written at a real size and back-dated to a real age, because both
//! are thresholds the scanner decides on — a fixture that faked either would
//! specify nothing.

use super::scratch;
use crate::model::{Candidate, ScanEvent};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub struct a_download_directory {
    dir: scratch,
    /// How long something must sit untouched before the scan counts it.
    stale_days: u64,
    /// The floor under which nothing is reported.
    floor: String,
}

impl a_download_directory {
    pub fn new() -> Self {
        Self {
            dir: scratch::named("downloads"),
            stale_days: 30,
            floor: "0".into(),
        }
    }

    pub fn where_stale_means(mut self, days: u64) -> Self {
        self.stale_days = days;
        self
    }

    /// Nothing below this is worth putting in front of anyone.
    pub fn ignoring_anything_under(mut self, size: &str) -> Self {
        self.floor = size.into();
        self
    }

    /// A file of `bytes`, last touched `age_days` ago.
    pub fn with_a_file(self, name: &str, bytes: usize, age_days: u64) -> Self {
        let path = self.dir.path.join(name);
        std::fs::write(&path, vec![0u8; bytes]).unwrap();
        back_date(&path, age_days);
        self
    }

    /// A directory of one file, last touched `age_days` ago.
    pub fn with_a_directory(self, name: &str, bytes: usize, age_days: u64) -> Self {
        let path = self.dir.path.join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("contents.bin"), vec![0u8; bytes]).unwrap();
        back_date(&path, age_days);
        self
    }

    /// Everything the scanner reports for this download directory.
    pub fn candidates(self) -> Vec<Candidate> {
        let opts = self.opts();
        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::personal::downloads(&self.dir.path.clone(), &opts, &tx);
        drop(tx);
        collect(rx)
    }

    fn opts(&self) -> crate::scan::ScanOpts {
        let mut cfg = crate::config::Config::default();
        cfg.scan.downloads_floor = Some(self.floor.clone());
        crate::scan::ScanOpts {
            rules: std::sync::Arc::new(crate::scan::Rules::from_config(&cfg)),
            stale_days: self.stale_days,
            ..super::scanning_everything(vec![self.dir.path.clone()])
        }
    }
}

/// A directory holding device backups.
pub struct a_backup_directory {
    dir: scratch,
}

impl a_backup_directory {
    pub fn new() -> Self {
        Self {
            dir: scratch::named("mobilesync"),
        }
    }

    /// A backup whose `Info.plist` names the device it came from.
    pub fn with_a_backup_of(self, device: &str, bytes: usize) -> Self {
        // Named after the device identifier, exactly as the real thing is.
        let path = self.dir.path.join(format!("0000{:04x}-00", device.len()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("Manifest.db"), vec![0u8; bytes]).unwrap();
        std::fs::write(
            path.join("Info.plist"),
            format!(
                "<plist><dict>\n<key>Device Name</key>\n<string>{device}</string>\n</dict></plist>"
            ),
        )
        .unwrap();
        self
    }

    pub fn candidates(self) -> Vec<Candidate> {
        let opts = super::scanning_everything(vec![]);
        let (tx, rx) = std::sync::mpsc::channel();
        crate::scan::personal::device_backups(&[self.dir.path.clone()], &opts, &tx);
        drop(tx);
        collect(rx)
    }
}

fn collect(rx: std::sync::mpsc::Receiver<ScanEvent>) -> Vec<Candidate> {
    rx.into_iter()
        .filter_map(|e| match e {
            ScanEvent::Found(c) => Some(*c),
            _ => None,
        })
        .collect()
}

/// Move a path's modification time into the past.
///
/// The scanner's staleness rule reads mtime, so a fixture that wrote everything
/// "now" could only ever specify the recent case.
fn back_date(path: &PathBuf, age_days: u64) {
    if age_days == 0 {
        return;
    }
    let when = SystemTime::now() - Duration::from_secs(age_days * 86_400);
    // A directory cannot be opened for writing, and does not need to be: the
    // timestamp is set through the descriptor either way.
    let file = std::fs::File::open(path).expect("the fixture's own path");
    file.set_modified(when).expect("back-dating the fixture");
}

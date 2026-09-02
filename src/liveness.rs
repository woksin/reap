//! Whether a program that owns a resource is currently running.
//!
//! Most caches are rebuildable: deleting one costs a re-download and nothing
//! else. A few are *live*. Removing a package manager's store while it is
//! resolving does not cost a re-download, it corrupts an operation already in
//! flight — and the failure surfaces inside the other tool, a long way from the
//! thing that caused it. A rule that names an owner says "this is one of
//! those", and reap leaves it alone while that owner is up.
//!
//! The probe is deliberately tri-state. **Could not tell is not idle.** An
//! unreadable process table is a claim about reap, not about the machine, and
//! the only safe reading of it is to keep the cache. This matches how reap
//! already reports a Docker size it cannot parse: as unknown, never as zero.

use std::collections::HashSet;
use std::process::Command;
use std::sync::Mutex;

/// What a liveness probe established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// This named program is running, so the resource is in use.
    Running(String),
    /// The table was read and named no owner.
    Idle,
    /// The table could not be read, so nothing was established either way.
    Unknown,
}

enum Cached {
    NotLoaded,
    /// Process basenames, lowercased, with reap's own tree removed.
    Readable(HashSet<String>),
    Unreadable,
}

static TABLE: Mutex<Cached> = Mutex::new(Cached::NotLoaded);

/// Re-read the process table.
///
/// Called once at the start of a reap run so every candidate in that run is
/// judged against the same snapshot, and so a second reap in a long-lived
/// interface does not decide against a table read minutes ago.
pub fn refresh() {
    let fresh = read_table().map_or(Cached::Unreadable, Cached::Readable);
    if let Ok(mut slot) = TABLE.lock() {
        *slot = fresh;
    }
}

/// Is any of `owners` running?
///
/// An empty list is [`Liveness::Idle`]: a rule that names no owner is making no
/// claim, and must not be held up by one.
pub fn state(owners: &[String]) -> Liveness {
    if owners.is_empty() {
        return Liveness::Idle;
    }
    let Ok(mut slot) = TABLE.lock() else {
        // A poisoned lock means another thread panicked mid-probe. That is not
        // evidence the owner is idle.
        return Liveness::Unknown;
    };
    if matches!(*slot, Cached::NotLoaded) {
        *slot = read_table().map_or(Cached::Unreadable, Cached::Readable);
    }
    match &*slot {
        Cached::Unreadable | Cached::NotLoaded => Liveness::Unknown,
        Cached::Readable(names) => owners
            .iter()
            .find(|owner| names.contains(&normalise(owner)))
            .map_or(Liveness::Idle, |owner| Liveness::Running(owner.clone())),
    }
}

/// Reduce a program name to the token the table is keyed by.
///
/// Basename, lowercased, and without the extension Windows reports. Matching is
/// on whole names rather than substrings on purpose: `node` must not be
/// answered by a running `nodemon`, and a shared helper binary must not
/// attribute one application's cache to another.
fn normalise(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

#[cfg(not(windows))]
fn read_table() -> Option<HashSet<String>> {
    // `pid ppid comm`, so reap's own tree can be identified and dropped. A tool
    // reap itself spawned — `git`, `docker`, the pruner for a neighbouring
    // rule — is not a user's build, and counting it would let reap block on
    // itself.
    let out = Command::new("ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let mut rows = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(own), Some(above), Some(comm)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (Ok(own), Ok(above)) = (own.parse::<u32>(), above.parse::<u32>()) else {
            continue;
        };
        rows.push((own, above, comm.to_string()));
    }
    if rows.is_empty() {
        // A table that parsed to nothing is not a machine with no processes on
        // it; it is a format this did not understand.
        return None;
    }

    let me = std::process::id();
    let above: std::collections::HashMap<u32, u32> =
        rows.iter().map(|(own, up, _)| (*own, *up)).collect();
    let is_ours = |start: u32| {
        let mut walk = start;
        // Bounded, so a cycle in a malformed table cannot spin here.
        for _ in 0..64 {
            if walk == me {
                return true;
            }
            match above.get(&walk) {
                Some(&next) if next != walk && next != 0 => walk = next,
                _ => return false,
            }
        }
        false
    };

    Some(
        rows.iter()
            .filter(|(own, _, _)| !is_ours(*own))
            .map(|(_, _, comm)| normalise(comm))
            .collect(),
    )
}

#[cfg(windows)]
fn read_table() -> Option<HashSet<String>> {
    // `tasklist` reports no parent pid, so only reap's own entry can be
    // dropped rather than its whole tree. Every owner reap names is a build or
    // package-manager command it never spawns itself, so the difference does
    // not arise in practice; it is a narrower guarantee, not a broken one.
    let out = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let me = std::process::id().to_string();
    let mut names = HashSet::new();
    for line in text.lines() {
        // "name","pid","session","#","mem"
        let mut fields = line.split("\",\"").map(|f| f.trim_matches('"'));
        let (Some(name), Some(pid)) = (fields.next(), fields.next()) else {
            continue;
        };
        if pid.trim() == me {
            continue;
        }
        names.insert(normalise(name));
    }
    if names.is_empty() { None } else { Some(names) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_naming_no_owner_is_never_held_up() {
        assert_eq!(state(&[]), Liveness::Idle);
    }

    #[test]
    fn a_name_is_matched_whole_and_not_as_a_substring() {
        // The mistake this avoids: `nodemon` answering for `node`, or a shared
        // helper binary attributing one application's cache to another.
        assert_eq!(normalise("nodemon"), "nodemon");
        assert_ne!(normalise("nodemon"), normalise("node"));
    }

    #[test]
    fn a_name_is_reduced_to_its_basename_however_it_is_spelled() {
        assert_eq!(normalise("/usr/local/bin/cargo"), "cargo");
        assert_eq!(normalise(r"C:\Program Files\nodejs\node.exe"), "node");
        assert_eq!(normalise("Xcode"), "xcode");
    }

    /// The probe must find something that is definitely running: this test
    /// process. Proves the table is actually read and parsed on this platform,
    /// rather than quietly returning an empty set that would read as idle.
    #[test]
    fn the_running_test_process_is_visible_to_the_probe() {
        refresh();
        let own = std::env::current_exe().expect("the test binary has a path");
        let name = own.file_name().unwrap().to_string_lossy().into_owned();
        // reap excludes its *own* tree, so the test binary is deliberately not
        // matched. What is asserted is that the table read succeeded at all.
        assert_ne!(
            state(&[name]),
            Liveness::Unknown,
            "the process table should be readable on this platform"
        );
    }

    #[test]
    fn an_owner_that_cannot_be_running_reads_as_idle() {
        refresh();
        assert_eq!(
            state(&["reap-no-such-program-xyzzy".to_string()]),
            Liveness::Idle
        );
    }
}

use super::ScanOpts;
use crate::config::{CacheRule, expand};
use crate::model::{Action, Candidate, Category, ScanEvent};
use crate::util::{age_days, path_size, tilde};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// Where unnamed application caches live, per platform.
fn unnamed_cache_root(home: &std::path::Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
    }
}

/// Where applications keep their own data, per platform.
///
/// Unlike the cache root above, these hold real state as well — settings,
/// message history, licences — so nothing here is offered wholesale. Only the
/// directories named below, and only where they sit inside one of these trees.
fn app_data_roots(home: &std::path::Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(target_os = "macos") {
        roots.push(home.join("Library/Application Support"));
    } else if cfg!(windows) {
        roots.extend(
            ["APPDATA", "LOCALAPPDATA"]
                .iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from),
        );
    } else {
        roots.push(
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config")),
        );
        roots.push(
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share")),
        );
    }
    roots.retain(|r| r.is_dir());
    roots.sort();
    roots.dedup();
    roots
}

/// Directory names that mean "cache" wherever they turn up inside an
/// application's own data.
///
/// Every desktop application built on Electron — the chat clients, the meeting
/// clients, the note apps, the design tools — carries a Chromium inside it, and
/// Chromium writes these. They are the single largest thing on a lot of
/// non-developer machines and no platform's cache directory contains them,
/// because the application data directory is where Chromium was pointed.
///
/// Matched by exact name only. Anything ambiguous is left alone: this walks
/// directories that also hold real state, so a near-miss here deletes work.
const APP_CACHE_DIRS: &[&str] = &[
    "Cache",
    "Cache_Data",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "ShaderCache",
    "CachedData",
    "CachedExtensions",
    "component_crx_cache",
];

/// Trees not worth walking for the above: enormous, and none of them holds one.
const APP_DATA_SKIP: &[&str] = &[
    "steamapps",
    "Backup",
    "node_modules",
    "MobileSync",
    "Containers",
];

/// How far under an app-data root a cache directory can be and still be found.
///
/// Chromium's own is the deepest that matters — `Google/Chrome/User Data/
/// Default/Cache` is five down — and stopping there keeps the walk off the
/// content directories that live further in.
const APP_CACHE_MAX_DEPTH: usize = 5;

/// Entries under that root holding live state rather than a rebuildable cache.
const LIBRARY_CACHE_DENYLIST: &[&str] = &[
    "com.apple.containermanagerd",
    "com.apple.HomeKit",
    "CloudKit",
    "com.apple.iCloudHelper",
    "FamilyCircle",
];

pub fn scan(opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let _ = tx.send(ScanEvent::Status("scanning caches".into()));

    // Rules naming a path this machine does not have simply do not apply, which
    // is how one rule set covers macOS and Linux without branching.
    let present: Vec<(&CacheRule, PathBuf)> = opts
        .rules
        .caches
        .iter()
        .map(|rule| (rule, expand(&rule.path)))
        .filter(|(_, path)| path.exists())
        .collect();

    present
        .par_iter()
        .for_each_with(tx.clone(), |tx, (rule, path)| {
            let size = if path.is_dir() {
                opts.cache.size_of(path)
            } else {
                path_size(path)
            };
            if size < opts.min_size {
                return;
            }
            let action = match rule.prune.split_first() {
                Some((program, args)) => Action::Run {
                    program: program.clone(),
                    args: args.to_vec(),
                    cwd: None,
                },
                None => Action::Remove(path.clone()),
            };
            let detail = if rule.detail.is_empty() {
                tilde(path)
            } else {
                format!("{} · {}", tilde(path), rule.detail)
            };
            let cand = Candidate::new(
                Category::Caches,
                rule.group.clone(),
                rule.label.clone(),
                detail,
                size,
                rule.risk.into(),
                action,
            )
            .with_age(age_days(path));
            super::emit(tx, opts, cand);
        });

    if let Some(home) = super::home_dir() {
        library_caches(&home, opts, tx);
        let named: Vec<PathBuf> = present.iter().map(|(_, path)| path.clone()).collect();
        app_data_caches(&home, &named, opts, tx);
    }
}

/// Chromium-shaped caches sitting inside applications' own data directories.
///
/// Reported one directory at a time rather than one per application, because a
/// candidate is a thing that can be removed and there is no single path that
/// removes all of an app's caches at once. The label carries the application
/// name so the rows still read as belonging together.
fn app_data_caches(
    home: &std::path::Path,
    named: &[PathBuf],
    opts: &ScanOpts,
    tx: &Sender<ScanEvent>,
) {
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    for root in app_data_roots(home) {
        collect_app_caches(&root, &root, 0, &mut found);
    }

    // A path a rule already named is reported by that rule, with its own
    // wording and its own risk.
    found.retain(|(path, _)| !named.iter().any(|n| path.starts_with(n)));
    found.sort();
    found.dedup();

    found
        .par_iter()
        .for_each_with(tx.clone(), |tx, (path, label): &(PathBuf, String)| {
            let size = opts.cache.size_of(path);
            if size < opts.min_size {
                return;
            }
            let cand = Candidate::new(
                Category::Caches,
                "app caches",
                label.clone(),
                format!("{} · rebuilt by the app as you use it", tilde(path)),
                size,
                crate::model::Risk::Safe,
                Action::Remove(path.clone()),
            )
            .with_age(age_days(path));
            super::emit(tx, opts, cand);
        });
}

/// What to call one of these in a list of a hundred.
///
/// The first segment under the root is the application it belongs to, which is
/// what makes the rows group by eye. On its own that is not always enough —
/// several apps keep more than one, and some file theirs under a vendor name —
/// so anything deeper than `<app>/<cache>` also carries the directory it sits
/// in: `Code · vscode-browser/Cache` rather than a second `Code · Cache`.
fn app_cache_label(root: &std::path::Path, path: &std::path::Path) -> String {
    let name = |p: &std::path::Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let Ok(rel) = path.strip_prefix(root) else {
        return name(path);
    };
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    match parts.as_slice() {
        [] => name(path),
        [only] => only.clone(),
        [app, cache] => format!("{app} · {cache}"),
        [app, .., parent, cache] => format!("{app} · {parent}/{cache}"),
    }
}

/// Walk one app-data root, collecting `(cache directory, label)`.
fn collect_app_caches(
    root: &std::path::Path,
    dir: &std::path::Path,
    depth: usize,
    out: &mut Vec<(PathBuf, String)>,
) {
    if depth > APP_CACHE_MAX_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        // `is_dir` is false for symlinks, which keeps cycles out of the walk.
        if !entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if APP_DATA_SKIP.contains(&name.as_ref()) {
            continue;
        }
        let path = entry.path();
        if APP_CACHE_DIRS.contains(&name.as_ref()) {
            out.push((path.clone(), app_cache_label(root, &path)));
            // A cache directory never contains another one worth finding.
            continue;
        }
        collect_app_caches(root, &path, depth + 1, out);
    }
}

/// How far into the cache root the unnamed sweep will step around a rule.
const UNNAMED_MAX_DEPTH: usize = 3;

/// Anything large under the platform's application-cache root that no rule
/// already names — `~/Library/Caches` on macOS, `~/.cache` elsewhere.
fn library_caches(home: &std::path::Path, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let root = unnamed_cache_root(home);
    // Anything a rule already covers is reported by that rule, with its own
    // wording and its own risk.
    let named: Vec<PathBuf> = opts.rules.caches.iter().map(|r| expand(&r.path)).collect();

    let mut entries = Vec::new();
    collect_unnamed(&root, &named, 0, &mut entries);

    // These are noisy, so only surface the ones actually worth a keystroke.
    let floor = opts.min_size.max(opts.rules.library_cache_floor);

    entries.par_iter().for_each_with(tx.clone(), |tx, path| {
        let size = opts.cache.size_of(path);
        if size < floor {
            return;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let cand = Candidate::new(
            Category::Caches,
            "application caches",
            name,
            format!("{} · rebuilt by the owning app", tilde(path)),
            size,
            crate::model::Risk::Caution,
            Action::Remove(path.clone()),
        )
        .with_age(age_days(path));
        super::emit(tx, opts, cand);
    });
}

/// Collect the entries under `dir` that no rule names.
///
/// A directory holding a named rule is stepped into rather than reported, and
/// the rule's own entry is left out of what comes back. Reporting the parent as
/// well would offer the same bytes twice — once with a precise label and a
/// precise risk, once as an anonymous lump — and the headline "reclaimable"
/// figure would count them both.
fn collect_unnamed(dir: &std::path::Path, named: &[PathBuf], depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        // `is_dir` is false for symlinks, which keeps cycles out of the walk.
        if !entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if LIBRARY_CACHE_DENYLIST.contains(&name.as_ref()) {
            continue;
        }
        let path = entry.path();
        if named.contains(&path) {
            continue;
        }
        // Something named lives further in. Step past it so its siblings are
        // still offered, rather than losing them along with it.
        if depth < UNNAMED_MAX_DEPTH && named.iter().any(|n| n.starts_with(&path)) {
            collect_unnamed(&path, named, depth + 1, out);
            continue;
        }
        out.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn an_app_cache_is_labelled_by_the_app_that_owns_it() {
        let root = Path::new("/home/me/.config");
        assert_eq!(
            app_cache_label(root, &root.join("Slack/Cache")),
            "Slack · Cache"
        );
        // Deeper than <app>/<cache>, so the directory it sits in comes along —
        // otherwise an app with several of these produces identical rows.
        assert_eq!(
            app_cache_label(root, &root.join("Code/Partitions/vscode-browser/Cache")),
            "Code · vscode-browser/Cache"
        );
    }

    /// Set up `dir/<children>` and return the root.
    fn tree(name: &str, children: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("reap-caches-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for child in children {
            std::fs::create_dir_all(root.join(child)).unwrap();
        }
        root
    }

    #[test]
    fn the_unnamed_sweep_steps_around_a_rule_rather_than_over_it() {
        // The bug this exists to prevent: a rule naming `Google/Chrome` while
        // the sweep also reports `Google` wholesale. Both would be offered,
        // both would count their bytes, and the headline figure would promise
        // the same gigabytes twice.
        let root = tree("nested", &["Google/Chrome", "Google/Earth", "Spotify"]);
        let named = vec![root.join("Google/Chrome")];

        let mut found = Vec::new();
        collect_unnamed(&root, &named, 0, &mut found);
        found.sort();

        assert_eq!(
            found,
            vec![root.join("Google/Earth"), root.join("Spotify")],
            "the named entry must be left to its rule, and its siblings kept"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_unnamed_sweep_reports_a_directory_holding_no_rule_whole() {
        // Stepping into everything would turn one useful row into twenty
        // meaningless ones.
        let root = tree("whole", &["JetBrains/IntelliJ", "JetBrains/Rider"]);

        let mut found = Vec::new();
        collect_unnamed(&root, &[], 0, &mut found);

        assert_eq!(found, vec![root.join("JetBrains")]);
        std::fs::remove_dir_all(&root).ok();
    }
}

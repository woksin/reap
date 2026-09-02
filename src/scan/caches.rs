use super::ScanOpts;
use crate::config::{CacheRule, expand};
use crate::model::{Action, Candidate, Category, ScanEvent};
use crate::util::{age_days, days_since, path_size, tilde};
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// Where unnamed application caches live, per platform.
///
/// More than one place on macOS, which used to be written here as a choice
/// between them. The platform has a cache directory and says so, but a great
/// deal of developer tooling is cross-platform first and writes to `~/.cache`
/// wherever it runs — node, gh, the scanners, anything built to XDG. Treating
/// that as a Linux-only path left gigabytes on a Mac that no sweep ever looked
/// at, while rules naming `~/.cache/...` were quietly finding things there all
/// along.
fn unnamed_cache_roots(home: &std::path::Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(target_os = "macos") {
        roots.push(home.join("Library/Caches"));
    }
    roots.push(
        std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from),
    );
    roots.retain(|r| r.is_dir());
    roots.sort();
    roots.dedup();
    roots
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
            std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from),
        );
        roots.push(
            std::env::var_os("XDG_DATA_HOME")
                .map_or_else(|| home.join(".local/share"), PathBuf::from),
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
            let (size, age) = if path.is_dir() {
                let (size, newest) = opts.cache.measure(path);
                (size, newest.map(days_since))
            } else {
                (path_size(path), age_days(path))
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
            .with_age(age)
            .with_owner(rule.owner.clone())
            .with_footprint(path.clone());
            super::emit(tx, opts, cand);
        });

    if let Some(home) = super::home_dir() {
        granular_cache_children(&home, opts, tx);
        library_caches(&home, opts, tx);
        let named: Vec<PathBuf> = present.iter().map(|(_, path)| path.clone()).collect();
        app_data_caches(&home, &named, opts, tx);
    }
}

/// Large shared cache roots are often touched every day even while most of
/// their contents have not been used for months. Keep the whole-root row as an
/// honest inventory total, and add child rows at the smallest level whose
/// deletion has clear rebuild semantics.
fn granular_cache_children(home: &std::path::Path, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let layouts = [
        (
            home.join(".nuget/packages"),
            "package managers",
            "NuGet",
            "package and all locally cached versions · restored on demand",
            crate::model::Risk::Caution,
        ),
        (
            home.join("Library/Caches/JetBrains"),
            "editors",
            "JetBrains",
            "product cache · rebuilt or fetched by JetBrains",
            crate::model::Risk::Caution,
        ),
    ];

    for (root, group, owner, detail, risk) in layouts {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        let children: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        children.par_iter().for_each_with(tx.clone(), |tx, path| {
            let (size, newest) = opts.cache.measure(path);
            if size < opts.min_size {
                return;
            }
            let age = newest.map(days_since);
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let candidate = Candidate::new(
                Category::Caches,
                group,
                format!("{owner} · {name}"),
                format!("{} · {detail}", tilde(path)),
                size,
                risk,
                Action::Remove(path.clone()),
            )
            .with_age(age);
            super::emit(tx, opts, candidate);
        });
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
            let (size, newest) = opts.cache.measure(path);
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
            .with_age(newest.map(days_since));
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
        if !entry.file_type().is_ok_and(|f| f.is_dir()) {
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

/// How far below a cache entry to look for a checkout before offering it.
///
/// One level finds a directory that is itself a clone; two finds the common
/// arrangement where a tool keeps a directory of them, one per branch or per
/// task. Deeper than that costs more than it is worth on a cache root holding
/// hundreds of entries.
const REPO_PROBE_DEPTH: usize = 2;

/// Whether `dir`, or something just inside it, is a git checkout.
fn holds_a_checkout(dir: &std::path::Path, depth: usize) -> bool {
    if dir.join(".git").exists() {
        return true;
    }
    if depth == 0 {
        return false;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.flatten()
        // `is_dir` is false for symlinks, which keeps cycles out of the probe.
        .filter(|e| e.file_type().is_ok_and(|f| f.is_dir()))
        .any(|e| holds_a_checkout(&e.path(), depth - 1))
}

/// Is this entry what an uninstalled application left behind?
///
/// Only ever with proof. `None` — no inventory could be established, or the
/// platform has none to establish — answers `false` for everything, so the
/// sweep keeps the cautious wording it has always used rather than guessing in
/// the direction of a stronger claim.
fn is_leftover(
    path: &std::path::Path,
    name: &str,
    bundle_root: Option<&std::path::Path>,
    installed: Option<&std::collections::HashSet<String>>,
) -> bool {
    // Bundle identifiers are a macOS idea, and they name direct children of
    // `~/Library/Caches` — nowhere else. `~/.cache` holds tool names, and a
    // tool that puts its version in the directory name produces something that
    // parses as reverse DNS without being an identifier at all. Anchoring to
    // the one root where the concept exists is what makes the rest of this
    // sound, rather than relying on the shape of a name alone.
    if bundle_root.is_none_or(|root| path.parent() != Some(root)) {
        return false;
    }
    installed.is_some_and(|ids| {
        super::apps::looks_like_a_bundle_id(name)
            && !super::apps::is_apple(name)
            && !super::apps::accounted_for(ids, name)
    })
}

/// Anything large under the platform's application-cache root that no rule
/// already names — `~/Library/Caches` on macOS, `~/.cache` elsewhere.
fn library_caches(home: &std::path::Path, opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    // Anything a rule already covers is reported by that rule, with its own
    // wording and its own risk.
    let named: Vec<PathBuf> = opts.rules.caches.iter().map(|r| expand(&r.path)).collect();

    let mut entries = Vec::new();
    for root in unnamed_cache_roots(home) {
        collect_unnamed(&root, &named, 0, &mut entries);
    }

    // These are noisy, so only surface the ones actually worth a keystroke.
    let floor = opts.min_size.max(opts.rules.library_cache_floor);

    // Evidence for the sentence below. Resolved once, before the parallel
    // walk, so the `plutil` cost is paid a single time; `None` means the
    // machine could not be inventoried and every row keeps the cautious
    // wording it always had.
    let installed = super::apps::installed();
    // The one root where a directory name is a bundle identifier.
    let bundle_root = if cfg!(target_os = "macos") {
        Some(home.join("Library/Caches"))
    } else {
        None
    };

    entries.par_iter().for_each_with(tx.clone(), |tx, path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        // "Rebuilt by the owning app" is the claim this sweep makes about
        // everything it finds, and for a directory whose owning app was
        // uninstalled it is simply false: nothing will rebuild it, and it is
        // not a cache any more but what an application left behind. Saying so
        // is worth more than the row itself.
        //
        // Only ever with proof. Without an inventory, or for anything that is
        // not plainly a bundle identifier, the original wording stands.
        let orphaned = is_leftover(path, &name, bundle_root.as_deref(), installed);

        // The 200MB floor exists because an unexplained lump is noise. A
        // leftover is not unexplained — it is named, and its owner is gone — so
        // it only has to clear the global "hide anything under" threshold.
        let effective_floor = if orphaned { opts.min_size } else { floor };

        let (size, newest) = opts.cache.measure(path);
        if size < effective_floor {
            return;
        }

        let (detail, risk) = if orphaned {
            (
                format!("{} · no installed app owns this", tilde(path)),
                // Nothing is lost either way. If the inventory is right the
                // owner is gone and this is dead weight; if it somehow missed
                // an installed app, this is a cache and that app rebuilds it.
                crate::model::Risk::Safe,
            )
        } else {
            (
                format!("{} · rebuilt by the owning app", tilde(path)),
                crate::model::Risk::Caution,
            )
        };

        let cand = Candidate::new(
            Category::Caches,
            if orphaned {
                "uninstalled app leftovers"
            } else {
                "application caches"
            },
            name,
            detail,
            size,
            risk,
            Action::Remove(path.clone()),
        )
        .with_age(newest.map(days_since));
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
        if !entry.file_type().is_ok_and(|f| f.is_dir()) {
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
        // A checkout is not a cache, whatever directory it was put in. Tools
        // that build in a scratch directory leave worktrees under one of these
        // roots, and this sweep's whole claim is that the owning application
        // will rebuild what it takes — which is exactly wrong about a
        // repository, and wrong in the direction that loses commits. reap has a
        // category that can prove things about a checkout; this is not it, so
        // it declines rather than guesses.
        if holds_a_checkout(&path, REPO_PROBE_DEPTH) {
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

    /// The sweep's standing claim is that what it finds is "rebuilt by the
    /// owning app". These decide when that sentence is false.
    mod when_deciding_whether_an_owner_still_exists {
        use super::*;
        use std::collections::HashSet;

        fn installed(ids: &[&str]) -> HashSet<String> {
            ids.iter().map(|s| (*s).to_string()).collect()
        }

        const BUNDLE_ROOT: &str = "/Users/someone/Library/Caches";

        /// A direct child of the macOS application-cache root.
        fn under_bundle_root(name: &str) -> PathBuf {
            PathBuf::from(BUNDLE_ROOT).join(name)
        }

        fn verdict(name: &str, ids: Option<&HashSet<String>>) -> bool {
            is_leftover(
                &under_bundle_root(name),
                name,
                Some(Path::new(BUNDLE_ROOT)),
                ids,
            )
        }

        #[test]
        fn a_bundle_no_installed_app_accounts_for_is_a_leftover() {
            let ids = installed(&["com.microsoft.vscode"]);
            assert!(verdict("com.dead.editor", Some(&ids)));
        }

        #[test]
        fn a_bundle_an_installed_app_accounts_for_is_not() {
            let ids = installed(&["com.microsoft.vscode"]);
            assert!(!verdict("com.microsoft.VSCode", Some(&ids)));
        }

        /// Without an inventory every row keeps the wording it always had.
        /// This is the whole fail-closed property: no inventory must never
        /// become "nothing is installed, so everything is a leftover".
        #[test]
        fn nothing_is_a_leftover_when_the_machine_could_not_be_inventoried() {
            assert!(!verdict("com.dead.editor", None));
        }

        #[test]
        fn an_ordinary_directory_name_is_never_a_leftover() {
            let ids = installed(&["com.microsoft.vscode"]);
            // The sweep is full of these, and none of them is a bundle id.
            for name in ["Google", "Homebrew", "node-gyp", "com.example"] {
                assert!(!verdict(name, Some(&ids)), "{name} is not a bundle id");
            }
        }

        #[test]
        fn apples_own_caches_are_left_alone_even_when_unaccounted_for() {
            let ids = installed(&["com.microsoft.vscode"]);
            assert!(!verdict("com.apple.Safari", Some(&ids)));
        }

        /// The concept only exists under the macOS application-cache root. A
        /// real scan of a real machine offered two build-tool caches from
        /// `~/.cache` as uninstalled applications' leftovers, because a version
        /// number parses as reverse DNS. Anchoring to the root is what makes
        /// that impossible rather than merely unlikely.
        #[test]
        fn a_directory_outside_the_bundle_root_is_never_a_leftover() {
            let ids = installed(&["com.microsoft.vscode"]);
            let elsewhere = PathBuf::from("/Users/someone/.cache")
                .join("critterstack-consumer-generation-0.13.2");
            assert!(!is_leftover(
                &elsewhere,
                "critterstack-consumer-generation-0.13.2",
                Some(Path::new(BUNDLE_ROOT)),
                Some(&ids),
            ));
        }

        /// And nested entries are not identifiers either: `Google/Chrome` is a
        /// path, not a bundle id, however the sweep reached it.
        #[test]
        fn a_nested_entry_is_never_a_leftover() {
            let ids = installed(&["com.microsoft.vscode"]);
            let nested = under_bundle_root("Google").join("com.dead.editor");
            assert!(!is_leftover(
                &nested,
                "com.dead.editor",
                Some(Path::new(BUNDLE_ROOT)),
                Some(&ids),
            ));
        }
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
    fn the_unnamed_sweep_refuses_a_checkout_however_it_got_there() {
        // The bug this exists to prevent, and it was a real one: a tool that
        // builds in a scratch directory leaves git worktrees under a cache
        // root, and this sweep offers what it does not recognise as "rebuilt by
        // the owning app" at the rebuildable tier. Nothing rebuilds a commit
        // that was never pushed, and the rebuildable tier is taken by a recipe
        // without a typed confirmation.
        let root = tree(
            "checkouts",
            &[
                // A clone sitting directly in the cache root.
                "some-clone/.git",
                // The commoner shape: a directory of worktrees, one per task.
                "worktrees/task-one/.git",
                "worktrees/task-two/.git",
                // And something that really is a cache, to prove the guard is
                // not simply refusing everything.
                "http-cache/entries",
            ],
        );

        let mut found = Vec::new();
        collect_unnamed(&root, &[], 0, &mut found);
        found.sort();

        assert_eq!(
            found,
            vec![root.join("http-cache")],
            "a checkout must never be offered as a cache"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_checkout_is_recognised_at_the_surface_and_one_level_down() {
        let root = tree(
            "probe",
            &["itself/.git", "holder/inside/.git", "plain/a/b/c"],
        );

        assert!(holds_a_checkout(&root.join("itself"), REPO_PROBE_DEPTH));
        assert!(holds_a_checkout(&root.join("holder"), REPO_PROBE_DEPTH));
        assert!(!holds_a_checkout(&root.join("plain"), REPO_PROBE_DEPTH));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_cache_roots_include_the_cross_platform_one_everywhere() {
        // `~/.cache` was treated as the Linux answer and `~/Library/Caches` as
        // the macOS one, as though a machine had only whichever its vendor
        // chose. Tooling written to be cross-platform writes to `~/.cache` on a
        // Mac too, and none of it was ever swept.
        let home = tree("roots", &["Library/Caches", ".cache"]);
        // Set nothing: this reads XDG_CACHE_HOME from the environment, and the
        // specs must not depend on how the machine running them is configured.
        let roots = unnamed_cache_roots(&home);

        assert!(
            roots.contains(&home.join(".cache")) || std::env::var_os("XDG_CACHE_HOME").is_some(),
            "~/.cache must be swept on every platform: {roots:?}"
        );
        if cfg!(target_os = "macos") {
            assert!(
                roots.contains(&home.join("Library/Caches")),
                "the platform's own cache root must still be swept: {roots:?}"
            );
        }
        std::fs::remove_dir_all(&home).ok();
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

//! Which applications this machine actually has installed.
//!
//! The cache sweep offers a reverse-DNS directory under `~/Library/Caches`
//! saying it is "rebuilt by the owning app". When no such app exists that
//! sentence is simply untrue: nothing will rebuild it, and it is not a cache at
//! all any more, it is what an uninstalled application left behind.
//!
//! Telling the two apart needs evidence, and the evidence is an inventory of
//! installed bundle identifiers. This is the same shape as the artifact rules,
//! where a `target` is only offered when a sibling `Cargo.toml` proves what
//! produced it: a leftover is only named when the inventory proves nothing owns
//! it.
//!
//! The inventory **fails closed**. Every finding built on it is an assertion
//! that nothing owns a directory, so an inventory that could not be completed
//! must be reported as unknown rather than as empty — an empty one would make
//! that assertion about everything at once. This is the mistake that made
//! mole's equivalent delete live applications' data: a Spotlight probe timed
//! out, the empty result was cached as "not installed", and a transient stall
//! became a deletion.

use std::collections::HashSet;

/// Is this the name of a bundle identifier rather than an ordinary directory?
///
/// Reverse DNS, at least three labels. Two would admit `com.example`, but it
/// would also admit any directory with a dot in the middle of its name.
///
/// No label may be entirely digits, which is what separates an identifier from
/// a **version number**. `critterstack-consumer-generation-0.13.2` splits into
/// three perfectly well-formed labels and is not an identifier at all — found
/// by running this against a real machine, where it was the only thing standing
/// between a build tool's cache and being called an uninstalled app's leftover.
/// `com.1password.safari` is why the test is "entirely digits" rather than
/// "starts with a digit": that one is a real identifier, installed here.
pub fn looks_like_a_bundle_id(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() >= 3
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.chars().all(|c| c.is_ascii_digit())
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// Apple's own identifiers, which are never leftovers to offer.
///
/// A `com.apple.*` cache belongs to the operating system whether or not any
/// application claims it, and the inventory has no way to prove otherwise.
pub fn is_apple(bundle: &str) -> bool {
    bundle.len() >= 10 && bundle[..10].eq_ignore_ascii_case("com.apple.")
}

#[cfg(target_os = "macos")]
mod inventory {
    use super::HashSet;
    use rayon::prelude::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::OnceLock;

    /// Where applications live. Homebrew casks and Setapp nest one level
    /// further, which is why those get a deeper walk below.
    fn roots(home: &Path) -> Vec<(PathBuf, usize)> {
        vec![
            (PathBuf::from("/Applications"), 2),
            (PathBuf::from("/Applications/Utilities"), 1),
            (PathBuf::from("/System/Applications"), 2),
            (PathBuf::from("/System/Applications/Utilities"), 1),
            (home.join("Applications"), 2),
            (PathBuf::from("/opt/homebrew/Caskroom"), 3),
            (PathBuf::from("/usr/local/Caskroom"), 3),
            (PathBuf::from("/Library/Input Methods"), 1),
            (home.join("Library/Input Methods"), 1),
            (
                home.join("Library/Application Support/Setapp/Applications"),
                2,
            ),
        ]
    }

    fn collect_bundles(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth == 0 || out.len() > 4000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            // `is_dir` is false for a symlink, which keeps an aliased
            // Applications folder from being walked twice.
            if !entry.file_type().is_ok_and(|f| f.is_dir()) {
                continue;
            }
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "app") {
                out.push(path);
            } else {
                collect_bundles(&path, depth - 1, out);
            }
        }
    }

    /// The identifier an app bundle declares, plus any privileged helper it
    /// registers.
    ///
    /// Helpers are named by the file under `Contents/Library/LaunchServices`,
    /// which is where an embedded helper's own identifier appears. Without them
    /// a helper's cache looks unowned while its parent application is installed
    /// and running.
    fn ids_for(app: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let info = app.join("Contents/Info.plist");
        if let Ok(out) = Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&info)
            .output()
            && out.status.success()
        {
            let id = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
            if super::looks_like_a_bundle_id(&id) {
                found.push(id);
            }
        }
        if let Ok(rd) = std::fs::read_dir(app.join("Contents/Library/LaunchServices")) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if super::looks_like_a_bundle_id(&name) {
                    found.push(name);
                }
            }
        }
        found
    }

    /// Every installed bundle identifier, lowercased.
    ///
    /// `None` when the inventory could not be established. Built once: the
    /// answer cannot change usefully within a single scan, and the `plutil`
    /// call per bundle is the expensive part.
    pub fn installed() -> Option<&'static HashSet<String>> {
        static CACHE: OnceLock<Option<HashSet<String>>> = OnceLock::new();
        CACHE.get_or_init(build).as_ref()
    }

    fn build() -> Option<HashSet<String>> {
        let home = crate::scan::home_dir()?;
        let mut bundles = Vec::new();
        for (root, depth) in roots(&home) {
            collect_bundles(&root, depth, &mut bundles);
        }
        // A Mac with no application bundles anywhere is not a Mac, it is a
        // machine reap could not read. Saying "nothing is installed" here would
        // declare every cache on it a leftover.
        if bundles.is_empty() {
            return None;
        }

        let ids: HashSet<String> = bundles
            .par_iter()
            .flat_map_iter(|app| ids_for(app))
            .collect();
        // Likewise: bundles were found but not one identifier could be read, so
        // the tool that reads them is missing or refusing.
        if ids.is_empty() { None } else { Some(ids) }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn scratch(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("reap-apps-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// Write an app bundle declaring `id`, shaped the way a real one is.
        fn an_app(root: &Path, rel: &str, id: &str) -> PathBuf {
            let app = root.join(rel);
            std::fs::create_dir_all(app.join("Contents")).unwrap();
            std::fs::write(
                app.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <plist version=\"1.0\"><dict>\n\
                     <key>CFBundleIdentifier</key><string>{id}</string>\n\
                     </dict></plist>"
                ),
            )
            .unwrap();
            app
        }

        /// Setapp and the Homebrew Caskroom nest a bundle several levels down,
        /// which is why those roots get a deeper walk. Neither is on every Mac,
        /// so this builds the shape rather than waiting to meet one.
        #[test]
        fn a_bundle_is_found_at_the_top_level_and_nested_in_a_cask() {
            let root = scratch("nested");
            an_app(&root, "Top.app", "com.example.top");
            an_app(&root, "some-cask/1.2.3/Deep.app", "com.example.deep");

            let mut found = Vec::new();
            collect_bundles(&root, 3, &mut found);
            // Sort the names, not the paths: `Top.app` sorts before
            // `some-cask/…` by path, which says nothing about what was found.
            let mut names: Vec<_> = found
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            assert_eq!(names, vec!["Deep.app", "Top.app"]);

            std::fs::remove_dir_all(&root).ok();
        }

        /// A bundle is a leaf. Helper applications live inside real ones, and
        /// walking into them would inventory a helper as separately installed.
        #[test]
        fn the_walk_does_not_descend_into_a_bundle() {
            let root = scratch("leaf");
            an_app(&root, "Outer.app", "com.example.outer");
            an_app(
                &root,
                "Outer.app/Contents/Helpers/Inner.app",
                "com.example.inner",
            );

            let mut found = Vec::new();
            collect_bundles(&root, 5, &mut found);
            assert_eq!(found.len(), 1, "found: {found:?}");
            assert!(found[0].ends_with("Outer.app"));

            std::fs::remove_dir_all(&root).ok();
        }

        /// An aliased Applications folder would otherwise be walked twice, and
        /// a link pointing back up the tree would not terminate at all.
        #[test]
        fn a_linked_directory_is_not_followed() {
            let root = scratch("linked");
            let real = root.join("real");
            an_app(&real, "Linked.app", "com.example.linked");
            std::os::unix::fs::symlink(&real, root.join("alias")).unwrap();

            let mut found = Vec::new();
            collect_bundles(&root, 3, &mut found);
            assert_eq!(found.len(), 1, "the alias must not yield a second copy");

            std::fs::remove_dir_all(&root).ok();
        }

        #[test]
        fn the_declared_identifier_is_read_from_a_real_bundle() {
            let root = scratch("ids");
            let app = an_app(&root, "Thing.app", "com.example.Thing");
            assert_eq!(ids_for(&app), vec!["com.example.thing".to_string()]);
            std::fs::remove_dir_all(&root).ok();
        }

        /// An embedded privileged helper registers its own identifier as a file
        /// name. Missing those is what made mole report the Adobe helpers as
        /// unowned while Acrobat was installed.
        #[test]
        fn an_embedded_helper_contributes_its_own_identifier() {
            let root = scratch("helpers");
            let app = an_app(&root, "Parent.app", "com.example.parent");
            let services = app.join("Contents/Library/LaunchServices");
            std::fs::create_dir_all(&services).unwrap();
            std::fs::write(services.join("com.example.parent.helper"), b"").unwrap();
            // Not an identifier, so it must not enter the inventory.
            std::fs::write(services.join("README"), b"").unwrap();

            let mut ids = ids_for(&app);
            ids.sort();
            assert_eq!(
                ids,
                vec![
                    "com.example.parent".to_string(),
                    "com.example.parent.helper".to_string()
                ]
            );
            std::fs::remove_dir_all(&root).ok();
        }

        /// A bundle whose plist cannot be read contributes nothing rather than
        /// something wrong. It is skipped, never guessed at.
        #[test]
        fn a_bundle_without_a_readable_plist_contributes_nothing() {
            let root = scratch("unreadable");
            let app = root.join("Broken.app");
            std::fs::create_dir_all(app.join("Contents")).unwrap();
            std::fs::write(app.join("Contents/Info.plist"), b"not a plist at all").unwrap();
            assert!(ids_for(&app).is_empty());
            std::fs::remove_dir_all(&root).ok();
        }
    }
}

#[cfg(target_os = "macos")]
pub use inventory::installed;

/// Every other platform has no equivalent inventory, so nothing is claimed.
///
/// Linux has no bundle identifiers at all, and Windows records installations in
/// the registry rather than on disk in a form this could read. Reporting
/// `None` leaves the leftover labelling inert there rather than guessing.
#[cfg(not(target_os = "macos"))]
pub const fn installed() -> Option<&'static HashSet<String>> {
    None
}

/// Does anything in `installed` account for `bundle`?
///
/// Deliberately generous in both directions. `com.foo.app.helper` is accounted
/// for by an installed `com.foo.app`, and a shared vendor directory
/// `com.foo` is accounted for by an installed `com.foo.app` — matching only
/// exactly would call both of those leftovers while the owner is sitting in
/// `/Applications`. Every extra match here costs a row that is not offered; a
/// missed one costs a wrong claim about someone's disk.
pub fn accounted_for(installed: &HashSet<String>, bundle: &str) -> bool {
    let bundle = bundle.to_lowercase();
    installed.iter().any(|id| {
        id == &bundle
            || bundle
                .strip_prefix(id.as_str())
                .is_some_and(|r| r.starts_with('.'))
            || id
                .strip_prefix(bundle.as_str())
                .is_some_and(|r| r.starts_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_bundle_id_needs_at_least_three_labels() {
        assert!(looks_like_a_bundle_id("com.microsoft.VSCode"));
        assert!(looks_like_a_bundle_id("org.mozilla.firefox"));
        // A digit may start a label — this one is real, and installed on the
        // machine this was written on.
        assert!(looks_like_a_bundle_id("com.1password.safari"));
        // Ordinary cache directory names must not be read as identifiers.
        assert!(!looks_like_a_bundle_id("Google"));
        assert!(!looks_like_a_bundle_id("com.example"));
        assert!(!looks_like_a_bundle_id("node-gyp"));
        assert!(!looks_like_a_bundle_id("my.cache.dir with spaces"));
        assert!(!looks_like_a_bundle_id("a..b"));
    }

    /// A version number is not an identifier. Found by running the scan against
    /// a real machine rather than by reasoning about the predicate.
    #[test]
    fn a_versioned_directory_name_is_not_a_bundle_id() {
        assert!(!looks_like_a_bundle_id(
            "critterstack-consumer-generation-0.13.2"
        ));
        assert!(!looks_like_a_bundle_id("mytool-1.2.3"));
        assert!(!looks_like_a_bundle_id("1.2.3"));
    }

    #[test]
    fn apples_own_identifiers_are_never_leftovers() {
        assert!(is_apple("com.apple.Safari"));
        assert!(is_apple("COM.APPLE.finder"));
        assert!(!is_apple("com.applesauce.thing"));
        assert!(!is_apple("com.app"));
    }

    #[test]
    fn an_exact_identifier_is_accounted_for() {
        let installed = installed_set(&["com.microsoft.vscode"]);
        assert!(accounted_for(&installed, "com.microsoft.VSCode"));
        assert!(!accounted_for(&installed, "com.dead.app"));
    }

    /// An embedded helper's cache is not a leftover while its parent is
    /// installed. This is the false positive mole hit with the Adobe helpers.
    #[test]
    fn a_helper_is_accounted_for_by_its_parent_application() {
        let installed = installed_set(&["com.adobe.acrobat"]);
        assert!(accounted_for(&installed, "com.adobe.acrobat.helper"));
        assert!(accounted_for(&installed, "com.adobe.acrobat.armdc.helper"));
    }

    /// And a shared vendor directory is accounted for by any app under it.
    #[test]
    fn a_vendor_directory_is_accounted_for_by_an_app_beneath_it() {
        let installed = installed_set(&["com.google.chrome"]);
        assert!(accounted_for(&installed, "com.google"));
    }

    /// Reads the machine it runs on, so it asserts only what must hold
    /// anywhere: that an inventory can be built at all, and that it finds the
    /// applications any Mac has. Everything else here is printed rather than
    /// asserted, because the right answer differs per machine.
    #[test]
    #[ignore = "inventories the machine it runs on"]
    fn the_real_machine_can_be_inventoried() {
        let Some(ids) = installed() else {
            #[cfg(target_os = "macos")]
            panic!("a Mac should be inventoriable");
            #[cfg(not(target_os = "macos"))]
            return;
        };
        println!("installed bundle identifiers: {}", ids.len());
        assert!(
            ids.len() > 5,
            "a machine with applications on it should yield more than {} identifiers",
            ids.len()
        );
        assert!(
            ids.iter().any(|id| id.starts_with("com.apple.")),
            "the system applications should be in the inventory"
        );
        let mut sample: Vec<&String> = ids.iter().filter(|id| !is_apple(id)).take(15).collect();
        sample.sort();
        for id in sample {
            println!("  {id}");
        }
    }

    #[test]
    fn a_near_miss_is_not_accounted_for() {
        let installed = installed_set(&["com.google.chrome"]);
        // Prefix matching must respect label boundaries, or `com.google.chrome`
        // would account for an unrelated `com.google.chromecast-leftover`.
        assert!(!accounted_for(&installed, "com.googley.thing"));
        assert!(!accounted_for(&installed, "com.google.chromecast"));
    }
}

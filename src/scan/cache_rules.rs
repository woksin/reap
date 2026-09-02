//! The caches reap ships knowing about.
//!
//! Data, deliberately kept apart from the scanner that reads it — the scanner
//! is a hundred lines of behaviour and this is eight hundred lines of facts
//! about where a dozen vendors put their temporary files, and reading either
//! is easier when the other is not in the way.
//!
//! Adding an entry here is the same act as adding a `[[cache]]` to a config
//! file, and produces the same thing: these seed the rule set, config entries
//! extend it, and `replace_builtin_caches` swaps this list out entirely.
//!
//! Paths take `~` for the home directory and `%VARIABLE%` for an environment
//! variable. A path this machine does not have is a rule that does not apply,
//! which is the whole of how one list covers macOS, Linux and Windows — the
//! Xcode entries are inert on Linux, the `%LOCALAPPDATA%` ones everywhere that
//! is not Windows.

use crate::config::{CacheRule, RiskName};

/// `(path, group, label, detail, risk, prune command)`.
///
/// Seeds the rule set; `[[cache]]` entries in the config add to it, and
/// `replace_builtin_caches` swaps it out entirely.
type BuiltinCache = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    RiskName,
    &'static [&'static str],
);

const BUILTIN: &[BuiltinCache] = &[
    (
        "~/.npm/_cacache",
        "package managers",
        "npm cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/Yarn",
        "package managers",
        "yarn cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/yarn",
        "package managers",
        "yarn cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/pnpm/store",
        "package managers",
        "pnpm store",
        "hard-linked into existing installs — pruned, not deleted",
        RiskName::Rebuildable,
        &["pnpm", "store", "prune"],
    ),
    (
        "~/.local/share/pnpm/store",
        "package managers",
        "pnpm store",
        "hard-linked into existing installs — pruned, not deleted",
        RiskName::Rebuildable,
        &["pnpm", "store", "prune"],
    ),
    (
        "~/.bun/install/cache",
        "package managers",
        "bun cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.nuget/packages",
        "package managers",
        "NuGet global packages",
        "every restored .NET package — re-downloaded on next restore",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/Library/Caches/NuGetHttpCache",
        "package managers",
        "NuGet HTTP cache",
        "re-downloaded on next restore",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.m2/repository",
        "package managers",
        "Maven repository",
        "re-downloaded on next build",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.gradle/caches",
        "package managers",
        "Gradle caches",
        "re-downloaded and re-built on next build",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cargo/registry/cache",
        "package managers",
        "cargo registry cache",
        "downloaded .crate archives",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cargo/registry/src",
        "package managers",
        "cargo registry sources",
        "unpacked crate sources — re-extracted on next build",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cargo/git/checkouts",
        "package managers",
        "cargo git checkouts",
        "git dependencies — re-cloned on next build",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/go/pkg/mod",
        "package managers",
        "Go module cache",
        "re-downloaded on next build",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/pip",
        "package managers",
        "pip cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/pip",
        "package managers",
        "pip cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/uv",
        "package managers",
        "uv cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/Homebrew",
        "package managers",
        "Homebrew downloads",
        "bottle archives already installed",
        RiskName::Safe,
        &["brew", "cleanup", "--prune=all"],
    ),
    (
        "~/Library/Caches/go-build",
        "compilers",
        "Go build cache",
        "rebuilt on next compile",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/go-build",
        "compilers",
        "Go build cache",
        "rebuilt on next compile",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.dotnet/optimizationdata",
        "compilers",
        ".NET optimization data",
        "regenerated by the SDK",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.nvm/.cache",
        "compilers",
        "nvm download cache",
        "node tarballs already installed",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Developer/Xcode/DerivedData",
        "xcode",
        "Xcode DerivedData",
        "index and build products — rebuilt on next build",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Developer/Xcode/Archives",
        "xcode",
        "Xcode archives",
        "shipped app archives — needed to re-symbolicate old crash logs",
        RiskName::Irreversible,
        &[],
    ),
    (
        "~/Library/Developer/Xcode/iOS DeviceSupport",
        "xcode",
        "iOS device support",
        "symbols per device/OS — re-copied when you next attach a device",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/Library/Developer/CoreSimulator/Caches",
        "xcode",
        "Simulator caches",
        "regenerated by the simulator",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/ms-playwright",
        "browsers",
        "Playwright browsers",
        "re-downloaded by `playwright install`",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/ms-playwright",
        "browsers",
        "Playwright browsers",
        "re-downloaded by `playwright install`",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/puppeteer",
        "browsers",
        "Puppeteer browsers",
        "re-downloaded on next install",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.npm/_npx",
        "package managers",
        "npx package cache",
        "packages fetched to run a command once · fetched again next time",
        RiskName::Safe,
        &[],
    ),
    (
        // Yarn Berry keeps its own store here rather than in either of the two
        // places Yarn Classic used, so the rules for those never covered it.
        "~/.yarn/berry/cache",
        "package managers",
        "Yarn Berry cache",
        "package archives · re-downloaded on next install",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.yarn/berry/metadata",
        "package managers",
        "Yarn Berry metadata",
        "registry metadata · re-fetched on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.rustup/downloads",
        "compilers",
        "rustup downloads",
        "part-downloaded toolchains · fetched again by rustup",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.rustup/tmp",
        "compilers",
        "rustup scratch files",
        "what rustup was in the middle of · nothing finished lives here",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.pulumi/plugins",
        "package managers",
        "Pulumi plugins",
        "downloaded providers · re-downloaded on next up",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.pulumi/logs",
        "package managers",
        "Pulumi logs",
        "what the CLI wrote down about itself",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/pnpm",
        "package managers",
        "pnpm cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/deno",
        "package managers",
        "Deno cache",
        "re-downloaded on next run",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/pre-commit",
        "package managers",
        "pre-commit hooks",
        "rebuilt on next run",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/huggingface",
        "models",
        "Hugging Face cache",
        "downloaded model weights — large, and slow to re-fetch",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/torch",
        "models",
        "PyTorch cache",
        "downloaded model weights",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/JetBrains",
        "editors",
        "JetBrains caches",
        "indexes, rebuilt on next open",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/JetBrains",
        "editors",
        "JetBrains caches",
        "indexes and downloaded toolbox data, rebuilt or fetched by JetBrains",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/Library/Application Support/Code/CachedExtensionVSIXs",
        "editors",
        "VS Code extension downloads",
        "the archives extensions were installed from · already unpacked",
        RiskName::Safe,
        &[],
    ),
    (
        // Usually the largest single directory a developer machine has that no
        // other rule here reaches, and the only one in this table graded like a
        // personal file.
        //
        // Every package cache above is safe to offer because the record of what
        // to put back is a project file reap never touches — a Cargo.toml, a
        // package.json, a pom. This one's record is `extensions.json`, and it
        // lives *inside* the directory. Take it and the extensions come back
        // only if you can remember all eighty of them, or if Settings Sync had
        // them. reap cannot see whether it did, so it does not claim to: the
        // row is shown, because hiding the biggest thing on the disk is how
        // disks stay full, and it is graded so that nothing takes it without
        // being asked. Someone who syncs can say so with an `[[override]]`.
        "~/.vscode/extensions",
        "editors",
        "VS Code extensions",
        "downloaded extensions · the only record of which ones you had is inside them",
        RiskName::Irreversible,
        &[],
    ),
    (
        "~/.rustup/toolchains",
        "compilers",
        "Rust toolchains",
        "re-installed by rustup · which versions is in each project's manifest",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.nvm/versions",
        "compilers",
        "Node versions",
        "re-installed by nvm · which versions is in each project's .nvmrc",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.local/share/Trash",
        "system",
        "Trash",
        "already deleted — this empties it for good",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.Trash",
        "system",
        "Trash",
        "already deleted — this empties it for good",
        RiskName::Rebuildable,
        &[],
    ),
    // -- Windows -----------------------------------------------------------
    //
    // Written with `%VARIABLE%` for the same reason the rules above are written
    // with `~`: a path this machine does not have is a rule that does not
    // apply, so one table covers three operating systems without branching.
    (
        "%LOCALAPPDATA%/Temp",
        "system",
        "Temporary files",
        "what applications left behind · anything still open is skipped",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "%LOCALAPPDATA%/CrashDumps",
        "system",
        "Crash dumps",
        "memory snapshots from programs that stopped working",
        RiskName::Safe,
        &[],
    ),
    (
        "%SystemDrive%/$Recycle.Bin",
        "system",
        "Recycle Bin",
        "already deleted — this empties it for good",
        RiskName::Rebuildable,
        // Emptied through the shell rather than unlinked: the bin is indexed,
        // and deleting its files by hand leaves the index describing entries
        // that are no longer there.
        &[
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Clear-RecycleBin -Force -ErrorAction SilentlyContinue",
        ],
    ),
    (
        "%LOCALAPPDATA%/D3DSCache",
        "system",
        "Direct3D shader cache",
        "rebuilt by games and GPU drivers",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/NVIDIA/DXCache",
        "system",
        "NVIDIA shader cache",
        "rebuilt by the driver",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/NVIDIA/GLCache",
        "system",
        "NVIDIA OpenGL cache",
        "rebuilt by the driver",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Packages/Microsoft.Windows.Search_cw5n1h2txyewy/LocalState/AppIconCache",
        "system",
        "Search icon cache",
        "rebuilt by Windows Search",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/pip/cache",
        "package managers",
        "pip cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/npm-cache",
        "package managers",
        "npm cache",
        "re-downloaded on next install",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/NuGet/v3-cache",
        "package managers",
        "NuGet HTTP cache",
        "re-downloaded on next restore",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Temp/chocolatey",
        "package managers",
        "Chocolatey downloads",
        "installers for packages already installed",
        RiskName::Safe,
        &[],
    ),
    // -- Web browsers ------------------------------------------------------
    //
    // Every path here is a cache directory, never a profile: bookmarks,
    // history, passwords and open tabs live elsewhere and are not reap's
    // business. Clearing these logs nobody out.
    (
        "~/Library/Caches/Google/Chrome",
        "web browsers",
        "Chrome cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/google-chrome",
        "web browsers",
        "Chrome cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Google/Chrome/User Data/Default/Cache",
        "web browsers",
        "Chrome cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/Microsoft Edge",
        "web browsers",
        "Edge cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Microsoft/Edge/User Data/Default/Cache",
        "web browsers",
        "Edge cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/Firefox",
        "web browsers",
        "Firefox cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/mozilla",
        "web browsers",
        "Firefox cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        // The Firefox profile itself lives under %APPDATA%; what is under
        // %LOCALAPPDATA% is only its cache.
        "%LOCALAPPDATA%/Mozilla/Firefox/Profiles",
        "web browsers",
        "Firefox cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/com.apple.Safari",
        "web browsers",
        "Safari cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/BraveSoftware",
        "web browsers",
        "Brave cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/BraveSoftware",
        "web browsers",
        "Brave cache",
        "pages re-downloaded as you browse · you stay signed in",
        RiskName::Safe,
        &[],
    ),
    // -- Creative tools ----------------------------------------------------
    //
    // The reason a designer's disk fills up. None of this is under any
    // platform's cache directory, so nothing else here would ever find it —
    // Adobe's media cache in particular routinely outgrows everything else on
    // the machine, and is regenerated the next time a clip is opened.
    (
        "~/Library/Application Support/Adobe/Common/Media Cache Files",
        "creative tools",
        "Adobe media cache",
        "conformed audio and video · rebuilt when you reopen a project",
        RiskName::Safe,
        &[],
    ),
    (
        "%APPDATA%/Adobe/Common/Media Cache Files",
        "creative tools",
        "Adobe media cache",
        "conformed audio and video · rebuilt when you reopen a project",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Application Support/Adobe/Common/Media Cache",
        "creative tools",
        "Adobe media cache database",
        "the index over the media cache · rebuilt alongside it",
        RiskName::Safe,
        &[],
    ),
    (
        "%APPDATA%/Adobe/Common/Media Cache",
        "creative tools",
        "Adobe media cache database",
        "the index over the media cache · rebuilt alongside it",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Application Support/Adobe/Common/Peak Files",
        "creative tools",
        "Adobe waveform files",
        "the drawn audio waveforms · redrawn on next open",
        RiskName::Safe,
        &[],
    ),
    (
        "%APPDATA%/Adobe/Common/Peak Files",
        "creative tools",
        "Adobe waveform files",
        "the drawn audio waveforms · redrawn on next open",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/Adobe/After Effects",
        "creative tools",
        "After Effects disk cache",
        "rendered frames · re-rendered as you scrub the timeline",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Caches/Adobe Camera Raw/Cache",
        "creative tools",
        "Camera Raw cache",
        "decoded raw previews · rebuilt when you next open the photos",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Adobe/CameraRaw/Cache",
        "creative tools",
        "Camera Raw cache",
        "decoded raw previews · rebuilt when you next open the photos",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Movies/DaVinci Resolve/CacheClip",
        "creative tools",
        "DaVinci Resolve render cache",
        "cached clips · re-rendered, which takes real time",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/Videos/DaVinci Resolve/CacheClip",
        "creative tools",
        "DaVinci Resolve render cache",
        "cached clips · re-rendered, which takes real time",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "%USERPROFILE%/Videos/DaVinci Resolve/CacheClip",
        "creative tools",
        "DaVinci Resolve render cache",
        "cached clips · re-rendered, which takes real time",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/Library/Caches/Blender",
        "creative tools",
        "Blender cache",
        "simulation and render caches · recomputed on demand",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.cache/blender",
        "creative tools",
        "Blender cache",
        "simulation and render caches · recomputed on demand",
        RiskName::Rebuildable,
        &[],
    ),
    // -- Media and games ---------------------------------------------------
    (
        "~/Library/Caches/com.spotify.client",
        "media apps",
        "Spotify cache",
        "streamed audio · re-streamed as you listen",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/spotify",
        "media apps",
        "Spotify cache",
        "streamed audio · re-streamed as you listen",
        RiskName::Safe,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Spotify/Data",
        "media apps",
        "Spotify cache",
        "streamed audio · re-streamed as you listen",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Application Support/Spotify/PersistentCache/Storage",
        "media apps",
        "Spotify offline downloads",
        "music saved for offline · re-downloaded, and not without the internet",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "%LOCALAPPDATA%/Spotify/Storage",
        "media apps",
        "Spotify offline downloads",
        "music saved for offline · re-downloaded, and not without the internet",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/Library/Application Support/Steam/steamapps/shadercache",
        "media apps",
        "Steam shader cache",
        "recompiled the next time you launch the game",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.local/share/Steam/steamapps/shadercache",
        "media apps",
        "Steam shader cache",
        "recompiled the next time you launch the game",
        RiskName::Safe,
        &[],
    ),
    (
        "~/Library/Application Support/Steam/steamapps/downloading",
        "media apps",
        "Steam part-downloads",
        "interrupted downloads · they start again from the beginning",
        RiskName::Rebuildable,
        &[],
    ),
    (
        "~/.local/share/Steam/steamapps/downloading",
        "media apps",
        "Steam part-downloads",
        "interrupted downloads · they start again from the beginning",
        RiskName::Rebuildable,
        &[],
    ),
    // -- Desktop odds and ends --------------------------------------------
    (
        "~/Library/Logs",
        "system",
        "Application logs",
        "what your apps wrote down about themselves",
        RiskName::Safe,
        &[],
    ),
    (
        "~/.cache/thumbnails",
        "system",
        "Thumbnail cache",
        "regenerated as you browse your files",
        RiskName::Safe,
        &[],
    ),
];

/// Caches that are *live*, and the programs that make them so.
///
/// Kept apart from `BUILTIN` rather than added as a seventh column, because
/// this is a policy worth reading end to end: it is the complete list of places
/// where reap will refuse rather than proceed, and a reviewer should be able to
/// see all of it at once.
///
/// The bar for an entry is not "a program uses this". It is that deleting the
/// path underneath a running one **corrupts an operation in flight** instead of
/// merely costing a re-download. A package manager's content-addressed store
/// qualifies; a browser's render cache does not, which is why the browsers are
/// absent even though they are running far more often.
///
/// Keyed by path because that is already unique across the rule set — see
/// `no_two_rules_name_the_same_path`.
const OWNERS: &[(&str, &[&str])] = &[
    // Package managers writing content-addressed stores. A half-written entry
    // is indistinguishable from a good one on the next read.
    ("~/.npm/_cacache", &["npm"]),
    ("%LOCALAPPDATA%/npm-cache", &["npm"]),
    ("~/Library/pnpm/store", &["pnpm"]),
    ("~/.local/share/pnpm/store", &["pnpm"]),
    ("~/.cache/pnpm", &["pnpm"]),
    ("~/Library/Caches/Yarn", &["yarn"]),
    ("~/.cache/yarn", &["yarn"]),
    ("~/.yarn/berry/cache", &["yarn"]),
    ("~/.bun/install/cache", &["bun"]),
    ("~/.nuget/packages", &["dotnet", "nuget", "msbuild"]),
    ("~/.m2/repository", &["mvn", "java"]),
    ("~/.gradle/caches", &["gradle", "java"]),
    // Cargo takes a lock over these; removing one mid-build fails the build.
    ("~/.cargo/registry/cache", &["cargo"]),
    ("~/.cargo/registry/src", &["cargo"]),
    ("~/.cargo/git/checkouts", &["cargo"]),
    ("~/go/pkg/mod", &["go"]),
    ("~/Library/Caches/go-build", &["go"]),
    ("~/.cache/go-build", &["go"]),
    ("~/.cache/uv", &["uv"]),
    ("~/.cache/pip", &["pip", "pip3"]),
    ("~/Library/Caches/pip", &["pip", "pip3"]),
    // Xcode rebuilds DerivedData, but not while it is mid-build: the failure
    // lands as a stale-module error a long way from the cause.
    (
        "~/Library/Developer/Xcode/DerivedData",
        &["Xcode", "xcodebuild", "swift-frontend"],
    ),
];

/// The owners declared for a built-in rule, if it has any.
fn owners_for(path: &str) -> Vec<String> {
    OWNERS
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .map(|(_, owners)| owners.iter().map(|o| (*o).to_string()).collect())
        .unwrap_or_default()
}

pub fn builtin_rules() -> Vec<CacheRule> {
    BUILTIN
        .iter()
        .map(|(path, group, label, detail, risk, prune)| CacheRule {
            path: (*path).to_string(),
            group: (*group).to_string(),
            label: (*label).to_string(),
            detail: (*detail).to_string(),
            risk: *risk,
            prune: prune.iter().map(|a| (*a).to_string()).collect(),
            owner: owners_for(path),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_names_an_anchored_path() {
        // A relative path would be resolved against whatever directory reap
        // happened to be started in, and could match something nobody wrote it
        // for. Every path has to start at a home directory or at a variable.
        for (path, _, label, ..) in BUILTIN {
            assert!(
                path.starts_with("~/") || path.starts_with('%'),
                "{label} names {path}, which is not anchored anywhere"
            );
        }
    }

    #[test]
    fn no_two_rules_name_the_same_path() {
        // Two rules on one path report the same bytes twice, and the figure at
        // the top of the screen is the number people act on.
        let mut paths: Vec<&str> = BUILTIN.iter().map(|(p, ..)| *p).collect();
        paths.sort_unstable();
        let before = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), before, "a built-in cache path is listed twice");
    }

    #[test]
    fn no_rule_says_it_is_safe_while_describing_something_that_is_not() {
        // The detail line is what a non-developer reads instead of the risk
        // dot. A rule that says "downloaded" or "offline" is describing bytes
        // that come back over a network, which is the definition of the
        // rebuildable tier rather than the safe one.
        for (path, _, label, detail, risk, _) in BUILTIN {
            if *risk != RiskName::Safe {
                continue;
            }
            for word in ["offline", "not without the internet"] {
                assert!(
                    !detail.contains(word),
                    "{label} ({path}) is graded safe but its detail says {word:?}"
                );
            }
        }
    }

    #[test]
    fn a_rule_admitting_it_holds_the_only_record_is_not_graded_as_costing_time() {
        // What separates every package cache here from the one directory in it
        // that is graded like a personal file. A cache is safe to offer because
        // the record of what to put back is a project file reap never touches;
        // where that record is *inside* the directory instead, deleting it
        // costs something no amount of waiting returns.
        //
        // Both recoverable tiers promise the same thing in different currencies
        // — safe costs nothing, rebuildable costs time — and neither is true of
        // bytes that are the only copy of something. So a rule that says as
        // much in its own detail line has to be graded irreversible.
        for (path, _, label, detail, risk, _) in BUILTIN {
            let admits = ["only record", "only copy", "exists nowhere else"]
                .iter()
                .any(|word| detail.contains(word));
            assert!(
                !admits || *risk == RiskName::Irreversible,
                "{label} ({path}) says it holds the only record and is graded {risk:?}"
            );
        }
    }

    #[test]
    fn a_rule_that_prunes_rather_than_deletes_names_a_program() {
        // An empty `prune` means "remove the path". A `prune` holding only
        // arguments would try to run the first argument as a program.
        for (path, _, label, _, _, prune) in BUILTIN {
            if let Some(program) = prune.first() {
                assert!(
                    !program.starts_with('-'),
                    "{label} ({path}) prunes with {prune:?}, which starts at an argument"
                );
            }
        }
    }

    #[test]
    fn the_rules_survive_being_turned_into_what_the_scanner_reads() {
        let rules = builtin_rules();
        assert_eq!(rules.len(), BUILTIN.len());
        assert!(rules.iter().all(|r| !r.label.is_empty()));
        assert!(rules.iter().all(|r| !r.group.is_empty()));
    }
}

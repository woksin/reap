//! Telling you a new version exists, and getting out of the way while you take
//! it.
//!
//! reap is installed four different ways and only knows which one after the
//! fact, so updating is delegated to whatever put the binary there rather than
//! done by overwriting it. A tool that deletes files has no business also
//! replacing itself in `/usr/local/bin` behind a `sudo` it did not ask for.
//!
//! The check itself must never be the reason reap feels slow: it runs on its
//! own thread, gives up after a few seconds, and remembers the answer for a
//! day. Failure is silence — an unreachable network is not something to
//! interrupt someone about.

use std::path::Path;
use std::time::Duration;

const REPO: &str = "woksin/reap";
/// Long enough that nobody meets it twice in a session, short enough that a
/// release is noticed the day after it lands.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// How this copy of reap got here, and therefore how it should be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Under the Homebrew prefix.
    Homebrew,
    /// Built by `cargo install`.
    Cargo,
    /// A binary someone downloaded and put somewhere themselves.
    Manual,
}

impl Strategy {
    /// The command that performs the update, when one exists.
    pub fn command(self) -> Option<(&'static str, Vec<&'static str>)> {
        match self {
            // `brew upgrade` against a stale local copy of the tap reports
            // "already installed", so the fetch has to happen first.
            Self::Homebrew => Some(("brew", vec!["upgrade", "reap"])),
            Self::Cargo => Some((
                "cargo",
                vec![
                    "install",
                    "--git",
                    "https://github.com/woksin/reap",
                    "--force",
                ],
            )),
            Self::Manual => None,
        }
    }

    /// Run before `command`, for installers that cache their index.
    pub fn refresh_command(self) -> Option<(&'static str, Vec<&'static str>)> {
        match self {
            Self::Homebrew => Some(("brew", vec!["update"])),
            _ => None,
        }
    }

    /// What to tell someone whose installation reap cannot drive.
    pub fn instructions(self) -> String {
        match self {
            // Written in the shell the platform actually has: a curl pipe into
            // tar is not a thing anyone can paste into PowerShell. The Windows
            // asset is the executable itself, so there is nothing to unpack.
            Self::Manual if cfg!(windows) => format!(
                "Invoke-WebRequest {}/reap-{}.exe -OutFile {}\\reap.exe",
                latest_download_url(),
                asset_name(),
                install_dir().unwrap_or_else(|| ".".into()),
            ),
            Self::Manual => format!(
                "curl -fsSL {}/reap-{}.tar.gz | tar xz\n\
                 sudo mv reap {}",
                latest_download_url(),
                asset_name(),
                install_dir().unwrap_or_else(|| "/usr/local/bin".into()),
            ),
            Self::Homebrew => "brew update && brew upgrade reap".into(),
            Self::Cargo => "cargo install --git https://github.com/woksin/reap --force".into(),
        }
    }
}

fn latest_download_url() -> String {
    format!("https://github.com/{REPO}/releases/latest/download")
}

/// The directory this copy of reap is sitting in, when that can be worked out.
fn install_dir() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|d| d.display().to_string())
}

/// The release asset this machine would want.
///
/// Named for the machine rather than the Rust target triple that built it —
/// nobody choosing a download should have to know what "unknown-linux-gnu" is.
pub fn asset_name() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    if os == "windows" {
        return match std::env::consts::ARCH {
            "x86" => "windows-x86".to_string(),
            // Windows on ARM runs x64 binaries under emulation, and does it
            // well enough that a directory walk is indistinguishable. Pointing
            // an ARM machine at the x64 download is a working answer; a
            // windows-arm64 asset that is not published is a 404.
            _ => "windows-x86_64".to_string(),
        };
    }
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// Work out how this binary was installed, from where it lives.
pub fn detect() -> Strategy {
    let Ok(exe) = std::env::current_exe() else {
        return Strategy::Manual;
    };
    strategy_for_installed(&exe, std::fs::canonicalize(&exe).ok().as_deref())
}

/// The strategy for a binary sitting at `exe`, whose real file is `resolved`.
///
/// `current_exe` reports the path the binary was reached by, symlinks and all.
/// That matters on an Intel Mac, where Homebrew's prefix *is* `/usr/local`: the
/// real file lives in `/usr/local/Cellar/…` and `/usr/local/bin/reap` is a link
/// to it. Classifying the link alone finds no Homebrew marker and calls it a
/// manual install — which then tells someone to `sudo mv` a plain binary over
/// the link, orphaning the Cellar copy and leaving brew's manifest claiming a
/// file it no longer owns.
///
/// Resolution is only consulted when the literal path says nothing, so it can
/// add a recognised owner and never take one away. That also keeps the genuine
/// manual install at `/usr/local/bin/reap` — the location the install script
/// uses — correctly manual: a real file resolves to itself.
pub fn strategy_for_installed(exe: &Path, resolved: Option<&Path>) -> Strategy {
    match strategy_for(exe) {
        Strategy::Manual => resolved.map_or(Strategy::Manual, strategy_for),
        known => known,
    }
}

/// Split out so the decision can be specified without moving the binary.
pub fn strategy_for(exe: &Path) -> Strategy {
    let path = exe.to_string_lossy();
    // Homebrew keeps the real file in `<prefix>/Cellar/...` and links it into
    // `<prefix>/bin`, so either shape means brew owns it.
    if path.contains("/Cellar/") || path.contains("/homebrew/") || path.contains("/linuxbrew/") {
        return Strategy::Homebrew;
    }
    if path.contains("/.cargo/bin/") {
        return Strategy::Cargo;
    }
    Strategy::Manual
}

/// Is `latest` a version worth telling someone about?
///
/// Compared field by field rather than as text, so 1.10.0 is not judged older
/// than 1.9.0. Anything unparseable is treated as "no news".
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<(u64, u64, u64)> {
        let v = v.trim().trim_start_matches('v');
        // A prerelease is not a newer release of the thing it precedes.
        let v = v.split(['-', '+']).next()?;
        let mut parts = v.split('.').map(|p| p.parse::<u64>().ok());
        Some((
            parts.next()??,
            parts.next()??,
            parts.next().flatten().unwrap_or(0),
        ))
    };
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// The newest released version, from the day's cache or from GitHub.
///
/// Shelling out to `curl` rather than taking an HTTP client as a dependency:
/// this is one request a day, `curl` is already what the install instructions
/// use, and the alternative is a large tree of crates for it.
pub fn latest_version() -> Option<String> {
    if let Some(cached) = read_cache() {
        return Some(cached);
    }
    let out = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "5",
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .arg(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let tag = json.get("tag_name")?.as_str()?.trim_start_matches('v');
    if tag.is_empty() {
        return None;
    }
    write_cache(tag);
    Some(tag.to_string())
}

/// The version to mention, or nothing at all.
pub fn check(current: &str) -> Option<String> {
    if std::env::var_os("REAP_NO_UPDATE_CHECK").is_some() {
        return None;
    }
    latest_version().filter(|latest| is_newer(latest, current))
}

fn cache_path() -> Option<std::path::PathBuf> {
    crate::cache::cache_dir().map(|d| d.join("version-check.json"))
}

fn read_cache() -> Option<String> {
    let path = cache_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let checked_at = json.get("checked_at")?.as_u64()?;
    if crate::util::now_secs().saturating_sub(checked_at) > CHECK_INTERVAL.as_secs() {
        return None;
    }
    Some(json.get("latest")?.as_str()?.to_string())
}

fn write_cache(latest: &str) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = serde_json::json!({ "latest": latest, "checked_at": crate::util::now_secs() });
    let _ = std::fs::write(path, body.to_string());
}

/// One line for wherever a person is reading, or nothing.
///
/// Only when someone is actually there: a cron job's log is not a place to
/// mention new versions, and `--json` must stay machine-readable.
pub fn notice(current: &str) -> Option<String> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return None;
    }
    check(current).map(|latest| format!("\nUpdate available: {current} → {latest}   reap update"))
}

/// `reap update`.
pub fn run(current: &str) -> anyhow::Result<()> {
    let strategy = detect();

    // Asked for directly, so the day's cached answer is not good enough — the
    // point of typing this is to act on what is true now.
    let latest = std::env::var_os("REAP_NO_UPDATE_CHECK")
        .is_none()
        .then(|| {
            let _ = cache_path().map(std::fs::remove_file);
            latest_version()
        })
        .flatten();

    match &latest {
        Some(latest) if !is_newer(latest, current) => {
            println!("reap {current} is the latest release.");
            return Ok(());
        }
        Some(latest) => println!("reap {current} → {latest}"),
        None => println!("Could not reach GitHub to check for a newer release."),
    }

    let Some((program, args)) = strategy.command() else {
        println!(
            "\nThis copy was installed by hand, so reap will not replace it for you.\n\n{}",
            strategy.instructions()
        );
        return Ok(());
    };

    if let Some((program, args)) = strategy.refresh_command() {
        println!("$ {program} {}", args.join(" "));
        let _ = std::process::Command::new(program).args(&args).status();
    }

    println!("$ {program} {}", args.join(" "));
    let status = std::process::Command::new(program).args(&args).status()?;
    if !status.success() {
        anyhow::bail!(
            "{program} exited with {}. Update it yourself with:\n{}",
            status.code().unwrap_or(-1),
            strategy.instructions()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_homebrew_installation_is_recognised_by_either_of_its_two_shapes() {
        // The real file, and the symlink people actually have on their PATH.
        assert_eq!(
            strategy_for(&PathBuf::from("/opt/homebrew/Cellar/reap/1.0.0/bin/reap")),
            Strategy::Homebrew
        );
        assert_eq!(
            strategy_for(&PathBuf::from("/opt/homebrew/bin/reap")),
            Strategy::Homebrew
        );
        // Linux, where the prefix is named differently.
        assert_eq!(
            strategy_for(&PathBuf::from("/home/linuxbrew/.linuxbrew/bin/reap")),
            Strategy::Homebrew
        );
    }

    /// Homebrew's prefix on an Intel Mac *is* `/usr/local`, so the link on
    /// someone's PATH looks exactly like the manual install location. Only the
    /// file it points at can tell them apart.
    #[test]
    fn a_homebrew_link_that_looks_manual_is_recognised_by_what_it_points_at() {
        let link = PathBuf::from("/usr/local/bin/reap");
        let real = PathBuf::from("/usr/local/Cellar/reap/1.0.0/bin/reap");
        assert_eq!(
            strategy_for(&link),
            Strategy::Manual,
            "the link alone is indistinguishable from a manual install"
        );
        assert_eq!(
            strategy_for_installed(&link, Some(&real)),
            Strategy::Homebrew
        );
    }

    /// And the genuine manual install at the same path stays manual: a real
    /// file resolves to itself, so nothing is guessed on its behalf.
    #[test]
    fn a_real_file_at_the_manual_location_stays_manual() {
        let path = PathBuf::from("/usr/local/bin/reap");
        assert_eq!(strategy_for_installed(&path, Some(&path)), Strategy::Manual);
        assert_eq!(strategy_for_installed(&path, None), Strategy::Manual);
    }

    /// Resolution may add an owner and must never remove one, or a working
    /// Homebrew installation could be downgraded to "do it yourself" by a
    /// prefix that happens not to resolve.
    #[test]
    fn resolution_never_takes_away_an_owner_the_literal_path_established() {
        let brew = PathBuf::from("/opt/homebrew/bin/reap");
        let elsewhere = PathBuf::from("/tmp/somewhere/reap");
        assert_eq!(
            strategy_for_installed(&brew, Some(&elsewhere)),
            Strategy::Homebrew
        );
        assert_eq!(strategy_for_installed(&brew, None), Strategy::Homebrew);
    }

    /// The same thing against a real link on disk, so this is specified
    /// against the filesystem rather than against an assumption about it.
    #[test]
    #[cfg(unix)]
    fn the_link_is_followed_on_a_real_filesystem() {
        let root = std::env::temp_dir().join(format!("reap-update-{}", std::process::id()));
        let cellar = root.join("Cellar/reap/1.0.0/bin");
        std::fs::create_dir_all(&cellar).unwrap();
        let real = cellar.join("reap");
        std::fs::write(&real, b"binary").unwrap();

        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let link = bin.join("reap");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // What `current_exe` would hand us, and what it really is.
        assert_eq!(strategy_for(&link), Strategy::Manual);
        let resolved = std::fs::canonicalize(&link).unwrap();
        assert_eq!(
            strategy_for_installed(&link, Some(&resolved)),
            Strategy::Homebrew
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_cargo_installation_is_recognised() {
        assert_eq!(
            strategy_for(&PathBuf::from("/Users/x/.cargo/bin/reap")),
            Strategy::Cargo
        );
    }

    #[test]
    fn anything_else_is_left_to_its_owner() {
        // Guessing wrong here means running a package manager against a binary
        // it does not own, so anything unrecognised gets instructions instead.
        assert_eq!(
            strategy_for(&PathBuf::from("/usr/local/bin/reap")),
            Strategy::Manual
        );
        assert_eq!(
            strategy_for(&PathBuf::from("/home/x/bin/reap")),
            Strategy::Manual
        );
    }

    #[test]
    fn only_a_manual_installation_is_told_to_do_it_itself() {
        assert!(Strategy::Homebrew.command().is_some());
        assert!(Strategy::Cargo.command().is_some());
        assert!(Strategy::Manual.command().is_none());
    }

    #[test]
    fn versions_are_compared_as_numbers_not_as_text() {
        // The bug this exists to prevent: "1.10.0" sorts before "1.9.0".
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(is_newer("1.0.1", "1.0.0"));
    }

    #[test]
    fn the_same_version_is_not_news() {
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn a_leading_v_is_accepted_since_that_is_how_the_tag_reads() {
        assert!(is_newer("v1.1.0", "1.0.0"));
        assert!(!is_newer("v1.0.0", "v1.0.0"));
    }

    #[test]
    fn a_prerelease_is_not_a_newer_release_of_what_it_precedes() {
        assert!(!is_newer("1.0.0-rc1", "1.0.0"));
        assert!(is_newer("1.1.0-rc1", "1.0.0"));
    }

    #[test]
    fn an_unreadable_version_is_treated_as_no_news() {
        // Silence beats interrupting someone over a string reap cannot parse.
        assert!(!is_newer("banana", "1.0.0"));
        assert!(!is_newer("1.0.0", "banana"));
        assert!(!is_newer("", "1.0.0"));
    }

    #[test]
    fn the_asset_name_matches_something_the_release_actually_publishes() {
        // Exactly what the release workflow builds. Drift here sends someone to
        // a 404 with no way to tell why.
        let asset = asset_name();
        assert!(
            [
                "macos-arm64",
                "macos-x86_64",
                "linux-arm64",
                "linux-x86_64",
                "windows-x86_64",
                "windows-x86",
            ]
            .contains(&asset.as_str()),
            "no release asset is published for {asset}"
        );
    }

    #[test]
    fn a_manual_installation_is_told_to_update_itself_in_a_shell_it_has() {
        // A curl pipe into tar is not something anyone can paste into
        // PowerShell, and PowerShell is the only shell a fresh Windows has.
        let text = Strategy::Manual.instructions();
        if cfg!(windows) {
            assert!(text.contains("Invoke-WebRequest"), "{text}");
            assert!(text.contains(".exe"), "{text}");
        } else {
            assert!(text.contains("curl"), "{text}");
            assert!(text.contains("tar"), "{text}");
        }
        assert!(text.contains(&asset_name()), "{text}");
    }
}

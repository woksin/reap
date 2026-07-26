use super::ScanOpts;
use crate::config::{ArtifactRule, RiskName};
use crate::model::{Action, Candidate, Category, ScanEvent};
use crate::util::{age_days, human_age, tilde};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// `(directory, evidence, what regenerates it, risk)`.
///
/// Seeds the rule set; `[[artifact]]` entries in the config add to it, and
/// `replace_builtin_artifacts` swaps it out entirely.
type BuiltinArtifact = (
    &'static str,
    &'static [&'static str],
    &'static str,
    RiskName,
);

const BUILTIN: &[BuiltinArtifact] = &[
    (
        "node_modules",
        &["package.json"],
        "npm/pnpm install",
        RiskName::Rebuildable,
    ),
    (
        "target",
        &["Cargo.toml"],
        "cargo build",
        RiskName::Rebuildable,
    ),
    (
        "bin",
        &["*.csproj", "*.fsproj", "*.vbproj", "*.sln"],
        "dotnet build",
        RiskName::Rebuildable,
    ),
    (
        "obj",
        &["*.csproj", "*.fsproj", "*.vbproj", "*.sln"],
        "dotnet build",
        RiskName::Rebuildable,
    ),
    (
        "dist",
        &["package.json"],
        "npm run build",
        RiskName::Rebuildable,
    ),
    (
        "out",
        &["package.json"],
        "npm run build",
        RiskName::Rebuildable,
    ),
    (
        "build",
        &[
            "package.json",
            "CMakeLists.txt",
            "build.gradle",
            "build.gradle.kts",
        ],
        "the project build",
        RiskName::Rebuildable,
    ),
    (
        ".next",
        &["package.json"],
        "next build",
        RiskName::Rebuildable,
    ),
    (
        ".nuxt",
        &["package.json"],
        "nuxt build",
        RiskName::Rebuildable,
    ),
    (".turbo", &["package.json"], "turbo", RiskName::Safe),
    (
        ".svelte-kit",
        &["package.json"],
        "svelte-kit sync",
        RiskName::Rebuildable,
    ),
    (".angular", &["angular.json"], "ng build", RiskName::Safe),
    (".parcel-cache", &["package.json"], "parcel", RiskName::Safe),
    (
        "coverage",
        &["package.json", "pyproject.toml"],
        "the test suite",
        RiskName::Safe,
    ),
    (
        ".nyc_output",
        &["package.json"],
        "the test suite",
        RiskName::Safe,
    ),
    (
        ".venv",
        &["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"],
        "pip install",
        RiskName::Rebuildable,
    ),
    (
        "venv",
        &["pyproject.toml", "requirements.txt", "setup.py"],
        "pip install",
        RiskName::Rebuildable,
    ),
    ("__pycache__", &[], "python", RiskName::Safe),
    (".pytest_cache", &[], "pytest", RiskName::Safe),
    (".mypy_cache", &[], "mypy", RiskName::Safe),
    (".ruff_cache", &[], "ruff", RiskName::Safe),
    (".tox", &["tox.ini"], "tox", RiskName::Rebuildable),
    (
        ".gradle",
        &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
        "gradle",
        RiskName::Rebuildable,
    ),
    ("Pods", &["Podfile"], "pod install", RiskName::Rebuildable),
    (
        "vendor",
        &["composer.json"],
        "composer install",
        RiskName::Rebuildable,
    ),
    (
        "_build",
        &["mix.exs", "dune-project"],
        "mix compile",
        RiskName::Rebuildable,
    ),
    ("deps", &["mix.exs"], "mix deps.get", RiskName::Rebuildable),
    (
        ".terraform",
        &["*.tf"],
        "terraform init",
        RiskName::Rebuildable,
    ),
    (
        ".dart_tool",
        &["pubspec.yaml"],
        "dart pub get",
        RiskName::Rebuildable,
    ),
    (
        ".stack-work",
        &["stack.yaml"],
        "stack build",
        RiskName::Rebuildable,
    ),
    ("zig-cache", &["build.zig"], "zig build", RiskName::Safe),
    (
        "zig-out",
        &["build.zig"],
        "zig build",
        RiskName::Rebuildable,
    ),
];

pub fn builtin_rules() -> Vec<ArtifactRule> {
    BUILTIN
        .iter()
        .map(|(dir, evidence, regen, risk)| ArtifactRule {
            dir: (*dir).to_string(),
            evidence: evidence.iter().map(|e| (*e).to_string()).collect(),
            regen: (*regen).to_string(),
            risk: *risk,
        })
        .collect()
}

/// Index of the rule matching `name`, if its evidence is present.
fn match_rule(rules: &[ArtifactRule], name: &str, siblings: &HashSet<String>) -> Option<usize> {
    let idx = rules.iter().position(|r| r.dir == name)?;
    let rule = &rules[idx];
    if rule.evidence.is_empty() {
        return Some(idx);
    }
    let ok = rule.evidence.iter().any(|ev| match ev.strip_prefix("*.") {
        Some(ext) => siblings
            .iter()
            .any(|f| f.rsplit_once('.').is_some_and(|(_, e)| e == ext)),
        None => siblings.contains(ev.as_str()),
    });
    ok.then_some(idx)
}

struct Hit {
    path: PathBuf,
    /// Index into the resolved rule set.
    rule: usize,
}

pub fn scan(opts: &ScanOpts, tx: &Sender<ScanEvent>) {
    let rules = &opts.rules.artifacts;
    let mut hits = Vec::new();
    for root in &opts.roots {
        collect(root, 0, opts.max_depth, rules, opts, &mut hits);
    }
    // Catch strays sitting directly in $HOME — an `npm install` run in the
    // wrong directory leaves a node_modules no root would ever cover.
    if let Some(home) = super::home_dir() {
        let depth = opts.max_depth;
        collect(&home, depth, depth, rules, opts, &mut hits);
    }
    hits.sort_by(|a, b| a.path.cmp(&b.path));
    hits.dedup_by(|a, b| a.path == b.path);

    let _ = tx.send(ScanEvent::Status(format!(
        "sizing {} artifact directories",
        hits.len()
    )));

    // Sizing is the expensive part, so fan it out and stream results back as
    // each directory finishes.
    hits.par_iter().for_each_with(tx.clone(), |tx, hit| {
        let rule = &rules[hit.rule];
        let size = opts.cache.size_of(&hit.path);
        if size < opts.min_size {
            return;
        }
        let age = age_days(&hit.path);
        let project = hit
            .path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let detail = match age {
            Some(d) => format!(
                "{} · untouched {} · restore with {}",
                tilde(hit.path.parent().unwrap_or(&hit.path)),
                human_age(d),
                rule.regen
            ),
            None => format!("restore with {}", rule.regen),
        };

        let cand = Candidate::new(
            Category::Artifacts,
            rule.dir.clone(),
            format!("{project}/{}", rule.dir),
            detail,
            size,
            Into::into(rule.risk),
            Action::Remove(hit.path.clone()),
        )
        .with_age(age);
        super::emit(tx, opts, cand);
    });
}

fn collect(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    rules: &[ArtifactRule],
    opts: &ScanOpts,
    out: &mut Vec<Hit>,
) {
    if depth > max_depth {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };

    let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
    let mut files: HashSet<String> = HashSet::new();
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            subdirs.push((name, entry.path()));
        } else if ft.is_file() {
            files.insert(name);
        }
    }

    for (name, path) in subdirs {
        // Marker matching runs before the dot-directory filter, because plenty
        // of build output hides in `.next`, `.venv` and friends.
        if let Some(rule) = match_rule(rules, &name, &files) {
            // Skipping ignored paths here avoids sizing them at all.
            if !opts.rules.ignore.matches_path(&path) {
                out.push(Hit { path, rule });
            }
            continue;
        }
        if name.starts_with('.') || opts.rules.is_never_descend(&name) {
            continue;
        }
        collect(&path, depth + 1, max_depth, rules, opts, out);
    }
}

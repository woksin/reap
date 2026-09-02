mod accounting;
mod app;
mod cache;
mod config;
mod guide;
mod liveness;
mod model;
mod reaper;
mod recipes;
mod scan;
mod settings;
#[cfg(test)]
mod specs;
mod trash;
mod ui;
#[cfg(test)]
mod ui_tests;
mod update;
mod util;

use anyhow::Result;
use app::{App, Focus, Mode};
use clap::Parser;
use model::{Category, Risk};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use scan::ScanOpts;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use util::human;

/// Find and assess common stale developer and application data.
///
/// These doc comments are the `--help` text, so they are written for someone
/// reading a terminal rather than rustdoc.
#[allow(
    rustdoc::broken_intra_doc_links,
    reason = "the bracketed defaults, e.g. [30], are help text rather than doc \
              links; escaping them would print the backslashes in --help"
)]
#[derive(Parser)]
#[command(name = "reap", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Directory to scan for repositories and build artifacts. Repeatable.
    /// Defaults to conventional roots under home and local mounted volumes.
    #[arg(short, long = "path", value_name = "DIR")]
    paths: Vec<PathBuf>,

    /// Minimum age for candidates whose age reap can measure. [30]
    #[arg(long, value_name = "DAYS")]
    stale_days: Option<u64>,

    /// Ignore anything smaller than this, e.g. 50MB. [1MB]
    #[arg(long, value_name = "SIZE")]
    min_size: Option<String>,

    /// How deep to descend from each scan root. [8]
    #[arg(long, value_name = "N")]
    depth: Option<usize>,

    /// Show what would be removed without touching anything.
    #[arg(long)]
    dry_run: bool,

    /// Print findings and exit instead of opening the interface.
    #[arg(long)]
    list: bool,

    /// Print findings as JSON and exit. Machine-readable counterpart to --list.
    #[arg(long)]
    json: bool,

    /// Reap without the interface, for cron and scripts. Prints the plan and
    /// changes nothing unless --yes is also given.
    #[arg(long)]
    reap: bool,

    /// The most dangerous thing --reap will take. [safe]
    #[arg(long, value_name = "LEVEL", value_enum)]
    risk: Option<RiskCeiling>,

    /// Select what a quick-reap recipe would, by its key. Overrides --risk,
    /// since a recipe carries its own ceiling.
    #[arg(long, value_name = "KEY")]
    recipe: Option<char>,

    /// Actually carry out --reap. Without it, the plan is printed and nothing
    /// is touched.
    #[arg(long)]
    yes: bool,

    /// Skip the read-only disk usage catalogue.
    #[arg(long)]
    no_inventory: bool,

    /// Skip the Docker scan.
    #[arg(long)]
    no_docker: bool,

    /// Skip the cache scan.
    #[arg(long)]
    no_caches: bool,

    /// Skip the coding-agent scan — agent caches, packages, session history.
    #[arg(long)]
    no_agents: bool,

    /// Skip the personal scan — downloads, installers, device backups.
    #[arg(long)]
    no_personal: bool,

    /// Move path removals to trash. Git, Docker and cleaner commands are unchanged;
    /// trashed space returns only once the trash is emptied.
    #[arg(long)]
    trash: bool,

    /// Re-measure every directory instead of reusing cached sizes.
    #[arg(long)]
    no_cache: bool,

    /// Configuration file. Defaults to $`XDG_CONFIG_HOME/reap/config.toml`,
    /// or ~/.config/reap/config.toml.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Write a documented starter configuration and exit.
    #[arg(long)]
    write_config: bool,

    /// Never offer deletion candidates matching this pattern. Read-only usage
    /// remains visible. Repeatable; added to the configuration file's list.
    #[arg(long = "ignore", value_name = "PATTERN")]
    ignores: Vec<String>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Update reap to the latest release, using whatever installed it.
    Update,
    /// Print the walkthrough — the same one `?` shows in the interface.
    Guide,
}

/// How far up the risk scale an unattended run is allowed to go.
///
/// Named for the config's vocabulary rather than the enum's, so one word means
/// one thing across the flag, the file and the interface.
#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
enum RiskCeiling {
    Safe,
    Rebuildable,
    Irreversible,
}

/// The ceiling `--reap` runs under when none is named.
///
/// Named rather than written inline at the one place that reads it, so the
/// default is a thing a test can hold rather than a literal buried in an
/// expression. Nothing on the headless path asks for confirmation, so what this
/// is set to is the whole of the protection.
const DEFAULT_CEILING: RiskCeiling = RiskCeiling::Safe;

impl From<RiskCeiling> for Risk {
    fn from(c: RiskCeiling) -> Self {
        match c {
            RiskCeiling::Safe => Self::Safe,
            RiskCeiling::Rebuildable => Self::Caution,
            RiskCeiling::Irreversible => Self::Danger,
        }
    }
}

/// CLI arguments win over the configuration file, which wins over the defaults.
fn resolve<T>(flag: Option<T>, configured: Option<T>, default: T) -> T {
    flag.or(configured).unwrap_or(default)
}

/// Parse a size written the way a person types it, e.g. `50MB`, into bytes.
///
/// `None` means the string was not a size this understands, which is a
/// different thing from zero. Refusing to guess matters for the same reason it
/// does in the Docker parser: this figure decides what is too small to bother
/// showing, so reading `50XB` as 50 bytes would quietly put thousands of extra
/// entries in front of a delete key, and reading nonsense as 0 would remove the
/// floor altogether.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is checked finite and non-negative just above, and a \
              float-to-int cast saturates rather than wraps"
)]
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 = num.trim().parse().ok()?;
    if !num.is_finite() || num < 0.0 {
        return None;
    }
    // SI by default to match the display units; the binary suffixes are
    // accepted too for anyone who types them out of habit.
    let mult: f64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1.0,
        "K" | "KB" => 1e3,
        "M" | "MB" => 1e6,
        "G" | "GB" => 1e9,
        "T" | "TB" => 1e12,
        "KIB" => 1024.0,
        "MIB" => 1024f64.powi(2),
        "GIB" => 1024f64.powi(3),
        "TIB" => 1024f64.powi(4),
        _ => return None,
    };
    Some((num * mult) as u64)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Update) => return update::run(env!("CARGO_PKG_VERSION")),
        Some(Command::Guide) => {
            print!("{}", guide::plain());
            return Ok(());
        }
        None => {}
    }

    let config_path = cli
        .config
        .clone()
        .or_else(config::default_path)
        .unwrap_or_else(|| PathBuf::from("reap.toml"));

    if cli.write_config {
        if config_path.exists() {
            anyhow::bail!(
                "{} already exists — delete it first, or pass --config to write elsewhere",
                config_path.display()
            );
        }
        config::Config::write_template(&config_path)?;
        println!("Wrote {}", config_path.display());
        return Ok(());
    }

    // A malformed config is fatal rather than ignored: silently falling back to
    // defaults would change which files this tool offers to delete.
    let mut cfg = config::Config::load(&config_path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", config_path.display()))?;
    cfg.ignore.extend(cli.ignores.iter().cloned());

    let roots = if !cli.paths.is_empty() {
        cli.paths.clone()
    } else if !cfg.scan.roots.is_empty() {
        cfg.scan.roots.iter().map(|r| config::expand(r)).collect()
    } else {
        scan::default_roots()
    };
    let roots = scan::normalise_roots(roots);

    let min_size = cli
        .min_size
        .clone()
        .or_else(|| cfg.scan.min_size.clone())
        .unwrap_or_else(|| "1MB".to_string());

    let sizes = std::sync::Arc::new(cache::SizeCache::load(!cli.no_cache));
    let opts = ScanOpts {
        rules: std::sync::Arc::new(scan::Rules::from_config(&cfg)),
        cache: sizes.clone(),
        roots,
        stale_days: resolve(cli.stale_days, cfg.scan.stale_days, 30),
        min_size: parse_size(&min_size).ok_or_else(|| {
            anyhow::anyhow!("cannot read {min_size:?} as a size — try something like 50MB")
        })?,
        max_depth: resolve(cli.depth, cfg.scan.depth, 8),
        skip_inventory: cli.no_inventory || cfg.scan.inventory == Some(false),
        skip_docker: cli.no_docker || cfg.scan.docker == Some(false),
        skip_caches: cli.no_caches || cfg.scan.caches == Some(false),
        skip_agents: cli.no_agents || cfg.scan.agents == Some(false),
        skip_personal: cli.no_personal || cfg.scan.personal == Some(false),
        scan_home_strays: cli.paths.is_empty() && cfg.scan.roots.is_empty(),
    };

    let trash = cli.trash || cfg.scan.trash == Some(true);

    if cli.json {
        let result = json_mode(opts);
        sizes.save();
        return result;
    }

    if cli.reap {
        let result = reap_mode(opts, &cfg, &cli, trash);
        sizes.save();
        return result;
    }

    if cli.list {
        let result = list_mode(opts);
        sizes.save();
        return result;
    }

    let mut terminal = ratatui::init();
    let result = run(
        &mut terminal,
        App::new(opts, cli.dry_run, trash, cfg, config_path),
    );
    ratatui::restore();
    sizes.save();
    result
}

/// Findings as JSON, for anything that wants to decide for itself.
///
/// The schema names risks and categories with the words the config and the
/// interface use, so a script and a person are talking about the same thing.
/// `bytes` is always present; `path` only when the action is a removal.
#[expect(
    clippy::too_many_lines,
    reason = "the JSON schema and its unique-byte, inventory and filesystem summaries are built together"
)]
fn json_mode(opts: ScanOpts) -> Result<()> {
    let roots = opts.roots.clone();
    let items = app::collect_headless(opts);

    let entries: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            let mut entry = serde_json::json!({
                "category": i.category.title().to_lowercase(),
                "group": i.group,
                "label": i.label,
                "detail": i.detail,
                "bytes": i.size,
                "risk": if i.eligibility == model::Eligibility::Informational {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(i.risk.label().to_string())
                },
                "eligibility": i.eligibility.label(),
                "selectable": i.selectable(),
                "age_days": i.age_days,
                "command": i.action.describe(),
            });
            if let Some(path) = &i.footprint {
                entry["path"] = serde_json::json!(path.to_string_lossy());
            }
            entry
        })
        .collect();

    let assessed: Vec<_> = items
        .iter()
        .filter(|item| {
            matches!(
                item.eligibility,
                model::Eligibility::Reclaimable | model::Eligibility::Recent
            )
        })
        .cloned()
        .collect();
    let assessed_exclusive = accounting::exclusive_sizes(&assessed);
    let by_risk: serde_json::Value = [Risk::Safe, Risk::Caution, Risk::Danger]
        .iter()
        .map(|risk| {
            let matching: Vec<_> = assessed
                .iter()
                .zip(&assessed_exclusive)
                .filter(|(item, _)| item.selectable() && item.risk == *risk)
                .collect();
            (
                risk.label().to_string(),
                serde_json::json!({
                    "items": matching.len(),
                    "bytes": matching.iter().map(|(_, bytes)| **bytes).sum::<u64>(),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    // `selectable`, not eligibility alone, and for the same reason the
    // interface uses it: a row that is reclaimable but carries no action
    // returns no disk when taken. Both surfaces call the one method so they
    // cannot drift into reporting different totals for the same scan.
    let total = assessed
        .iter()
        .zip(&assessed_exclusive)
        .filter(|(item, _)| item.selectable())
        .map(|(_, bytes)| *bytes)
        .sum::<u64>();
    let reclaimable_items = assessed.iter().filter(|item| item.selectable()).count();

    let mut projected: std::collections::BTreeMap<
        String,
        (std::collections::BTreeSet<String>, u64, u64, u64),
    > = std::collections::BTreeMap::new();
    let mut logical_unassigned = 0u64;
    for (item, bytes) in assessed.iter().zip(&assessed_exclusive) {
        if !item.selectable() || *bytes == 0 {
            continue;
        }
        let stat = item.footprint.as_deref().and_then(util::disk_stat);
        if let Some(stat) = stat {
            let mount = stat.mount.display().to_string();
            let entry = projected
                .entry(stat.pool.clone())
                .or_insert_with(|| (std::collections::BTreeSet::new(), stat.free, stat.total, 0));
            entry.0.insert(mount);
            entry.1 = entry.1.min(stat.free);
            entry.2 = entry.2.max(stat.total);
            entry.3 = entry.3.saturating_add(*bytes);
        } else {
            logical_unassigned = logical_unassigned.saturating_add(*bytes);
        }
    }
    let projections: Vec<_> = projected
        .into_iter()
        .map(|(pool, (mounts, free, capacity, reclaimable))| {
            serde_json::json!({
                "pool": pool,
                "mounts": mounts,
                "free_bytes": free,
                "total_bytes": capacity,
                "reclaimable_bytes": reclaimable,
                "projected_free_bytes": free.saturating_add(reclaimable),
            })
        })
        .collect();

    let catalogued = items
        .iter()
        .filter(|item| item.eligibility == model::Eligibility::Informational)
        .map(|item| item.size)
        .sum::<u64>();
    let recent_bytes = assessed
        .iter()
        .zip(assessed_exclusive)
        .filter(|(item, _)| item.eligibility == model::Eligibility::Recent)
        .map(|(_, size)| size)
        .sum::<u64>();

    let mut out = serde_json::json!({
        "total_bytes": total,
        "reclaimable_bytes": total,
        "catalogued_bytes": catalogued,
        "recent_bytes": recent_bytes,
        "items": entries.len(),
        "reclaimable_items": reclaimable_items,
        "roots": roots.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
        "projected_reclaim_by_filesystem": projections,
        "logical_reclaim_without_host_path_bytes": logical_unassigned,
        "by_risk": by_risk,
        "findings": entries,
    });
    let current_path = std::env::current_dir()?;
    if let Some((mount, free, capacity)) = util::disk_capacity(&current_path) {
        let mut current_pool = None;
        let mut catalogued_in_pool = 0u64;
        for item in items
            .iter()
            .filter(|item| item.eligibility == model::Eligibility::Informational)
        {
            let Some(path) = item.footprint.as_deref() else {
                continue;
            };
            let shares_pool = util::shares_volume(&current_path, path) || {
                if current_pool.is_none() {
                    current_pool = util::disk_stat(&current_path);
                }
                current_pool.as_ref().is_some_and(|current| {
                    util::disk_stat(path).is_some_and(|stat| stat.shares_pool(current))
                })
            };
            if shares_pool {
                catalogued_in_pool = catalogued_in_pool.saturating_add(item.size);
            }
        }
        let used = capacity.saturating_sub(free);
        out["disk"] = serde_json::json!({
            "mount": mount,
            "free_bytes": free,
            "used_bytes": used,
            "total_bytes": capacity,
            "catalogued_bytes_in_pool": catalogued_in_pool,
            "system_or_unclassified_bytes": used.saturating_sub(catalogued_in_pool),
            "projection_note": "Docker logical resources may not immediately release the same number of host bytes; separate filesystems are not pooled",
        });
    }

    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

/// Reap without the interface: `reap --reap --risk safe --yes`, for cron.
///
/// The interface makes you look at what you selected and type a word for the
/// irreversible. Neither is available here, so the deliberate act is the flags
/// themselves — `--yes` to touch anything at all, and a `--risk` that has to be
/// raised by hand past the safe default before work can be lost.
#[expect(
    clippy::too_many_lines,
    reason = "headless selection, confirmation policy, execution and reporting are one CLI transaction"
)]
fn reap_mode(opts: ScanOpts, cfg: &config::Config, cli: &Cli, trash: bool) -> Result<()> {
    let items = app::collect_headless(opts);

    let (chosen, how): (Vec<model::Candidate>, String) = if let Some(key) = cli.recipe {
        let recipes = recipes::compile(cfg);
        let recipe = recipes
            .iter()
            .find(|r| r.key == key)
            .ok_or_else(|| anyhow::anyhow!("no recipe bound to {key:?}"))?;
        (
            items
                .into_iter()
                .filter(|item| item.selectable() && recipe.covers(item))
                .collect(),
            recipe.name.clone(),
        )
    } else {
        let ceiling: Risk = cli.risk.unwrap_or(DEFAULT_CEILING).into();
        (
            items
                .into_iter()
                .filter(|item| item.selectable() && item.risk <= ceiling)
                .collect(),
            format!("everything up to {}", ceiling.label()),
        )
    };

    let total = accounting::selection_size(chosen.iter());
    let exclusive = accounting::exclusive_sizes(&chosen);
    println!("{} — {} items, {}", how, chosen.len(), human(total));
    for risk in [Risk::Safe, Risk::Caution, Risk::Danger] {
        let matching: Vec<_> = chosen
            .iter()
            .zip(&exclusive)
            .filter(|(item, _)| item.risk == risk)
            .collect();
        if matching.is_empty() {
            continue;
        }
        println!(
            "  {} {:<14} {:>10}   {} items",
            risk.dot(),
            risk.label(),
            human(matching.iter().map(|(_, bytes)| **bytes).sum::<u64>()),
            matching.len()
        );
    }

    if chosen.is_empty() {
        return Ok(());
    }
    if !cli.yes {
        println!("\nNothing was touched. Add --yes to carry this out.");
        return Ok(());
    }
    if chosen.iter().any(|i| i.risk == Risk::Danger) {
        // Said out loud even though the flags asked for it, because this is the
        // one line in the output that a scrollback search will find later.
        println!("\n▲ This includes work that exists nowhere else.");
    }

    let reap_opts = reaper::ReapOpts {
        dry_run: cli.dry_run,
        trash,
    };
    let log = std::sync::Mutex::new((0u64, 0usize, Vec::<String>::new()));
    reaper::run_all(chosen, reap_opts, |report| {
        if let Ok(mut log) = log.lock() {
            if report.ok {
                log.0 += report.freed;
                log.1 += 1;
            } else {
                log.2.push(format!(
                    "{}: {}",
                    report.label,
                    report.error.unwrap_or_default()
                ));
            }
        }
    });

    let (freed, ok, failures) = log.into_inner().unwrap_or((0, 0, Vec::new()));
    println!(
        "\n{} {} · {ok} succeeded{}",
        if cli.dry_run {
            "Would free"
        } else if trash {
            "Moved to the trash:"
        } else {
            "Freed"
        },
        human(freed),
        if failures.is_empty() {
            String::new()
        } else {
            format!(", {} failed", failures.len())
        }
    );
    for failure in &failures {
        eprintln!("  ✗ {failure}");
    }
    if trash && !cli.dry_run {
        println!("The space comes back when the trash is emptied.");
    }
    if let Some(notice) = update::notice(env!("CARGO_PKG_VERSION")) {
        println!("{notice}");
    }
    // A cron job that half-worked should say so through its exit status.
    if !failures.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "plain output mirrors the UI's categories, state accounting, risk summary and pool-safe projection"
)]
fn list_mode(opts: ScanOpts) -> Result<()> {
    let items = app::collect_headless(opts);
    let assessed: Vec<_> = items
        .iter()
        .filter(|item| {
            matches!(
                item.eligibility,
                model::Eligibility::Reclaimable | model::Eligibility::Recent
            )
        })
        .cloned()
        .collect();
    let assessed_bytes = accounting::exclusive_sizes(&assessed);
    let inventory: Vec<_> = items
        .iter()
        .filter(|item| item.eligibility == model::Eligibility::Informational)
        .cloned()
        .collect();
    let inventory_bytes = accounting::exclusive_sizes(&inventory);

    let assessed_size =
        |category: Category, group: Option<&str>, eligibility: model::Eligibility| {
            assessed
                .iter()
                .zip(&assessed_bytes)
                .filter(|(item, _)| {
                    item.category == category
                        && item.eligibility == eligibility
                        && group.is_none_or(|group| item.group == group)
                })
                .map(|(_, bytes)| *bytes)
                .sum::<u64>()
        };
    let usage_size = |category: Category, group: Option<&str>| {
        inventory
            .iter()
            .zip(&inventory_bytes)
            .filter(|(item, _)| {
                item.category == category && group.is_none_or(|group| item.group == group)
            })
            .map(|(_, bytes)| *bytes)
            .sum::<u64>()
    };

    for category in Category::ALL {
        let in_category: Vec<_> = items
            .iter()
            .filter(|item| item.category == category)
            .collect();
        if in_category.is_empty() {
            continue;
        }
        println!(
            "\n{} — {} reclaimable · {} recent · {} usage ({} rows)",
            category.title(),
            human(assessed_size(
                category,
                None,
                model::Eligibility::Reclaimable
            )),
            human(assessed_size(category, None, model::Eligibility::Recent)),
            human(usage_size(category, None)),
            in_category.len()
        );

        let groups: std::collections::BTreeSet<&str> =
            in_category.iter().map(|item| item.group.as_str()).collect();
        for group in groups {
            let members: Vec<_> = in_category
                .iter()
                .copied()
                .filter(|item| item.group == group)
                .collect();
            println!(
                "\n  {} · {} reclaimable · {} recent · {} usage · {} rows",
                group,
                human(assessed_size(
                    category,
                    Some(group),
                    model::Eligibility::Reclaimable
                )),
                human(assessed_size(
                    category,
                    Some(group),
                    model::Eligibility::Recent
                )),
                human(usage_size(category, Some(group))),
                members.len()
            );
            for item in members {
                println!(
                    "    {:>9}  {:<46} [{:<11}] {}",
                    if item.size == 0 {
                        "—".into()
                    } else {
                        human(item.size)
                    },
                    item.label,
                    item.eligibility.label(),
                    item.detail
                );
            }
        }
    }

    let total = assessed
        .iter()
        .zip(&assessed_bytes)
        .filter(|(item, _)| item.eligibility == model::Eligibility::Reclaimable)
        .map(|(_, bytes)| *bytes)
        .sum::<u64>();
    let recent = assessed
        .iter()
        .zip(&assessed_bytes)
        .filter(|(item, _)| item.eligibility == model::Eligibility::Recent)
        .map(|(_, bytes)| *bytes)
        .sum::<u64>();
    let catalogued = inventory_bytes.iter().sum::<u64>();

    println!("\n{}", "─".repeat(72));
    for risk in [Risk::Safe, Risk::Caution, Risk::Danger] {
        let matching: Vec<_> = assessed
            .iter()
            .zip(&assessed_bytes)
            .filter(|(item, _)| {
                item.eligibility == model::Eligibility::Reclaimable && item.risk == risk
            })
            .collect();
        if matching.is_empty() {
            continue;
        }
        println!(
            "  {} {:<14} {:>10}   {} items",
            risk.dot(),
            risk.label(),
            human(matching.iter().map(|(_, bytes)| **bytes).sum::<u64>()),
            matching.len()
        );
    }
    println!("\n  Reclaimable: {}", human(total));
    println!("  Recent:      {}", human(recent));
    println!("  Catalogued:  {}", human(catalogued));

    let current_path = std::env::current_dir()?;
    if let Some((free, capacity)) = util::disk_free(&current_path) {
        let paths: Option<Vec<_>> = assessed
            .iter()
            .zip(&assessed_bytes)
            .filter(|(item, bytes)| {
                item.eligibility == model::Eligibility::Reclaimable && **bytes > 0
            })
            .map(|(item, _)| item.footprint.as_deref())
            .collect();
        let one_pool = paths.is_some_and(|paths| {
            paths
                .iter()
                .all(|path| util::shares_volume(&current_path, path))
                || util::disk_stat(&current_path).is_some_and(|current| {
                    paths.iter().all(|path| {
                        util::disk_stat(path).is_some_and(|stat| stat.shares_pool(&current))
                    })
                })
        });
        if one_pool {
            println!(
                "  Disk: {} free of {} — reaping everything would leave ≈ {} free",
                human(free),
                human(capacity),
                human(free.saturating_add(total))
            );
        } else {
            println!(
                "  Disk: {} free of {} — multiple storage pools; no combined projection",
                human(free),
                human(capacity)
            );
        }
    }
    if let Some(notice) = update::notice(env!("CARGO_PKG_VERSION")) {
        println!("{notice}");
    }
    Ok(())
}

fn run(terminal: &mut ratatui::DefaultTerminal, mut app: App) -> Result<()> {
    const TICK: Duration = Duration::from_millis(80);

    let mut tick: u64 = 0;
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::render(f, &app, tick))?;

        // Poll briefly so scanner output and the spinner both stay live.
        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            if ctrl && matches!(key.code, KeyCode::Char('c')) {
                break;
            }
            handle_key(&mut app, key.code);
        }

        if last_tick.elapsed() >= TICK {
            tick += 1;
            last_tick = Instant::now();
        }
        app.poll();

        if app.quit {
            break;
        }
    }
    Ok(())
}

/// Hand a keypress to whichever mode currently owns the screen.
fn handle_key(app: &mut App, code: KeyCode) {
    // Drawn over every screen, so it is dismissed before every screen's keys —
    // and by any of them, since the one thing someone wants after reading a
    // legend is to get back to what they were doing.
    if app.legend {
        app.legend = false;
        return;
    }
    // Everywhere except the screens where a letter is being typed into
    // something, which is the one place a shortcut must not steal it.
    if code == KeyCode::Char('L')
        && matches!(
            app.mode,
            Mode::Browsing | Mode::Recipes | Mode::Report | Mode::Settings
        )
        && !app.settings_editing()
    {
        app.legend = true;
        return;
    }

    match app.mode {
        Mode::Help => in_help(app, code),
        Mode::Search => in_search(app, code),
        Mode::Confirm => in_confirm(app, code),
        // Deletion is in flight; the only way out is the process signal.
        Mode::Reaping => {}
        Mode::Report => in_report(app, code),
        Mode::Recipes => in_recipes(app, code),
        // Two keyboards in one screen: while something is being typed every
        // printable key belongs to the text, and only when it is not do the
        // single-letter actions mean anything.
        Mode::Settings if app.settings_editing() => in_settings_edit(app, code),
        Mode::Settings => in_settings(app, code),
        Mode::Browsing => in_browsing(app, code),
    }
}

/// A document, so it scrolls rather than closing under the first key someone
/// presses to read further down it.
const fn in_help(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => app.help_scroll = app.help_scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => app.help_scroll += 1,
        KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(10),
        KeyCode::PageDown => app.help_scroll += 10,
        KeyCode::Home => app.help_scroll = 0,
        _ => {
            app.help_scroll = 0;
            app.mode = Mode::Browsing;
        }
    }
}

fn in_search(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.search.clear();
            app.mode = Mode::Browsing;
            app.rebuild();
        }
        KeyCode::Enter => app.mode = Mode::Browsing,
        KeyCode::Backspace => {
            app.search.pop();
            app.rebuild();
        }
        KeyCode::Char(c) => {
            app.search.push(c);
            app.item_idx = 0;
            app.rebuild();
        }
        _ => {}
    }
}

fn in_confirm(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.mode = Mode::Browsing,
        // Held until the acknowledgement the selection demands is given. Until
        // then Enter falls through to the wildcard and does nothing — it is not
        // a `Char`, so the text arm below never sees it.
        KeyCode::Enter if app.confirm_satisfied() => app.start_reap(),
        KeyCode::Backspace => {
            app.confirm_input.pop();
        }
        KeyCode::Char(c) => app.confirm_input.push(c),
        _ => {}
    }
}

fn in_report(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('r') => {
            app.mode = Mode::Browsing;
            app.rescan();
        }
        // Only offered when this run actually trashed something.
        KeyCode::Char('e') if !app.trashed.is_empty() => app.empty_trash(),
        KeyCode::Esc | KeyCode::Enter => app.mode = Mode::Browsing,
        _ => {}
    }
}

/// One key per standing decision. Anything unbound closes the palette rather
/// than doing nothing, so a mistyped key never leaves you stuck in front of a
/// list of ways to delete things.
fn in_recipes(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q' | '?') => app.mode = Mode::Browsing,
        KeyCode::Enter => app.apply_highlighted_recipe(),
        // A recipe's own key wins over the vim aliases: a user who binds `j`
        // gets their recipe, and the arrows still navigate. This arm has to
        // stay above them to keep that order; the arrows are unaffected by it,
        // since it only ever matches a `Char`.
        KeyCode::Char(c) if app.recipes.iter().any(|r| r.key == c) => app.apply_recipe(c),
        KeyCode::Up | KeyCode::Char('k') => app.move_recipe_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_recipe_cursor(1),
        _ => app.mode = Mode::Browsing,
    }
}

/// The settings screen while a value is being typed into it.
fn in_settings_edit(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.with_settings(|s, _| {
            s.edit = None;
            Some("cancelled".into())
        }),
        KeyCode::Enter => app.commit_settings_edit(),
        KeyCode::Backspace => app.with_settings(|s, _| {
            s.edit.as_mut()?.buffer.pop();
            None
        }),
        KeyCode::Char(c) => app.with_settings(|s, _| {
            s.edit.as_mut()?.buffer.push(c);
            None
        }),
        _ => {}
    }
}

/// The settings screen when nothing is being typed, so letters are actions.
fn in_settings(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => app.close_settings(),
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Up | KeyCode::Char('k') => app.with_settings(|s, _| {
            s.move_cursor(-1);
            None
        }),
        KeyCode::Down | KeyCode::Char('j') => app.with_settings(|s, _| {
            s.move_cursor(1);
            None
        }),
        KeyCode::PageUp => app.with_settings(|s, _| {
            s.move_cursor(-10);
            None
        }),
        KeyCode::PageDown => app.with_settings(|s, _| {
            s.move_cursor(10);
            None
        }),
        // Further than any list is long; the cursor saturates at the ends.
        KeyCode::Home => app.with_settings(|s, _| {
            s.move_cursor(isize::MIN);
            None
        }),
        KeyCode::End => app.with_settings(|s, _| {
            s.move_cursor(isize::MAX);
            None
        }),
        // One key for "do the obvious thing here", which is a different thing
        // on a heading, a switch and an add row.
        KeyCode::Enter | KeyCode::Char(' ') => {
            use crate::settings::Row;
            // Taken by value: every branch below reaches back into `app`
            // mutably, and a borrow of the row would still be alive.
            match app.settings.as_ref().and_then(|s| s.current()).cloned() {
                Some(Row::Heading(_)) => app.with_settings(|s, cfg| {
                    s.toggle_section(cfg);
                    None
                }),
                Some(Row::Add(_)) => app.with_settings(|s, _| s.begin_add()),
                Some(Row::Setting(setting)) if setting.is_switch() => {
                    app.change_settings(settings::Settings::toggle_switch);
                }
                Some(Row::Setting(_)) => app.with_settings(settings::Settings::begin_edit),
                _ => {}
            }
        }
        KeyCode::Char('e') => app.with_settings(settings::Settings::begin_edit),
        KeyCode::Char('n') => app.with_settings(settings::Settings::begin_rename),
        KeyCode::Char('a') => app.with_settings(|s, _| s.begin_add()),
        KeyCode::Char('x') => app.change_settings(settings::Settings::toggle_off),
        KeyCode::Char('g') => app.change_settings(settings::Settings::cycle_grade),
        KeyCode::Char('d') => app.change_settings(settings::Settings::delete),
        _ => {}
    }
}

fn in_browsing(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('C') => app.open_settings(),
        // Esc backs out of things rather than quitting: leaving a tool that
        // deletes files should take a deliberate keystroke.
        KeyCode::Esc => {
            if app.search.is_empty() {
                app.clear_selection();
            } else {
                app.search.clear();
                app.rebuild();
            }
        }
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Char('R') => app.mode = Mode::Recipes,
        KeyCode::Char('/') => app.mode = Mode::Search,
        KeyCode::Tab | KeyCode::BackTab => {
            app.focus = if app.focus == Focus::Sidebar {
                Focus::Items
            } else {
                Focus::Sidebar
            };
        }
        KeyCode::Left | KeyCode::Char('h') => app.focus = Focus::Sidebar,
        KeyCode::Right | KeyCode::Char('l') => app.focus = Focus::Items,
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::PageUp => app.move_cursor(-10),
        KeyCode::PageDown => app.move_cursor(10),
        // Further than any list is long; the cursor saturates at the ends.
        KeyCode::Home => app.move_cursor(isize::MIN),
        KeyCode::End => app.move_cursor(isize::MAX),
        KeyCode::Enter if app.focus == Focus::Sidebar => app.toggle_expand(),
        KeyCode::Char(' ') => {
            if app.focus == Focus::Items {
                app.toggle_current();
            } else {
                app.toggle_expand();
            }
        }
        KeyCode::Char('a') => app.set_all_visible(true),
        KeyCode::Char('s') => app.select_safe(),
        KeyCode::Char('n') => app.clear_selection(),
        KeyCode::Char('o') => app.cycle_sort(),
        KeyCode::Char('f') => app.cycle_risk_filter(),
        KeyCode::Char('v') => app.toggle_range(),
        KeyCode::Char('i') => app.inspect_current(),
        KeyCode::Char('x') => app.ignore_current(),
        KeyCode::Char('d') => app.begin_confirm(),
        KeyCode::Char('r') => app.rescan(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words on the command line and the levels inside reap are two
    /// different vocabularies, and `--risk` is the only thing standing in front
    /// of `--reap --yes`. A mapping that drifted would not fail loudly; it would
    /// quietly raise the ceiling on a run nobody is watching.
    #[test]
    fn each_ceiling_names_the_level_a_person_asked_for() {
        assert_eq!(Risk::from(RiskCeiling::Safe), Risk::Safe);
        assert_eq!(Risk::from(RiskCeiling::Rebuildable), Risk::Caution);
        assert_eq!(Risk::from(RiskCeiling::Irreversible), Risk::Danger);
    }

    /// Asking for nothing in particular must not authorise anything that costs
    /// work. This reads the same constant `--reap` does, so raising the default
    /// cannot pass unnoticed.
    #[test]
    fn asking_for_no_ceiling_admits_only_what_is_safe() {
        let ceiling: Risk = DEFAULT_CEILING.into();
        assert_eq!(ceiling, Risk::Safe);
        assert!(Risk::Caution > ceiling, "rebuildable must be excluded");
        assert!(Risk::Danger > ceiling, "irreversible must be excluded");
    }
}

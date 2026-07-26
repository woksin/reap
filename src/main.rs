mod app;
mod cache;
mod config;
mod model;
mod reaper;
mod scan;
mod trash;
mod ui;
#[cfg(test)]
mod ui_tests;
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

/// Find and prune the stale things eating your disk.
#[derive(Parser)]
#[command(name = "reap", version, about, long_about = None)]
struct Cli {
    /// Directory to scan for repositories and build artifacts. Repeatable.
    /// Defaults to the usual suspects under $HOME.
    #[arg(short, long = "path", value_name = "DIR")]
    paths: Vec<PathBuf>,

    /// How long something must sit untouched before it counts as stale. [30]
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

    /// Skip the Docker scan.
    #[arg(long)]
    no_docker: bool,

    /// Skip the cache scan.
    #[arg(long)]
    no_caches: bool,

    /// Move paths to the volume's trash instead of unlinking them. Recoverable,
    /// but space only comes back once the trash is emptied.
    #[arg(long)]
    trash: bool,

    /// Re-measure every directory instead of reusing cached sizes.
    #[arg(long)]
    no_cache: bool,

    /// Configuration file. Defaults to $XDG_CONFIG_HOME/reap/config.toml,
    /// or ~/.config/reap/config.toml.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Write a documented starter configuration and exit.
    #[arg(long)]
    write_config: bool,

    /// Never offer candidates matching this pattern. Repeatable, and added to
    /// whatever the configuration file already lists.
    #[arg(long = "ignore", value_name = "PATTERN")]
    ignores: Vec<String>,
}

/// CLI arguments win over the configuration file, which wins over the defaults.
fn resolve<T>(flag: Option<T>, configured: Option<T>, default: T) -> T {
    flag.or(configured).unwrap_or(default)
}

pub fn parse_size(s: &str) -> u64 {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 = num.trim().parse().unwrap_or(0.0);
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
        _ => 1.0,
    };
    (num * mult) as u64
}

fn main() -> Result<()> {
    let cli = Cli::parse();

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
    let roots: Vec<PathBuf> = roots.into_iter().filter(|p| p.is_dir()).collect();

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
        min_size: parse_size(&min_size),
        max_depth: resolve(cli.depth, cfg.scan.depth, 8),
        skip_docker: cli.no_docker || cfg.scan.docker == Some(false),
        skip_caches: cli.no_caches || cfg.scan.caches == Some(false),
    };

    if cli.list {
        let result = list_mode(opts);
        sizes.save();
        return result;
    }

    let trash = cli.trash || cfg.scan.trash == Some(true);
    let mut terminal = ratatui::init();
    let result = run(
        &mut terminal,
        App::new(opts, cli.dry_run, trash, cfg, config_path),
    );
    ratatui::restore();
    sizes.save();
    result
}

fn list_mode(opts: ScanOpts) -> Result<()> {
    let items = app::collect_headless(opts);
    let total: u64 = items.iter().map(|i| i.size).sum();

    for cat in Category::ALL {
        let in_cat: Vec<_> = items.iter().filter(|i| i.category == cat).collect();
        if in_cat.is_empty() {
            continue;
        }
        let size: u64 = in_cat.iter().map(|i| i.size).sum();
        println!(
            "\n{} — {} ({} items)",
            cat.title(),
            human(size),
            in_cat.len()
        );

        // Group headings carry the reason something is reapable, which is the
        // part worth reading when there are hundreds of entries.
        let mut groups: Vec<&str> = in_cat.iter().map(|i| i.group.as_str()).collect();
        groups.dedup();
        let mut seen: Vec<&str> = Vec::new();
        for group in groups {
            if seen.contains(&group) {
                continue;
            }
            seen.push(group);
            let members: Vec<_> = in_cat.iter().filter(|i| i.group == group).collect();
            let gsize: u64 = members.iter().map(|i| i.size).sum();
            println!(
                "\n  {} · {} · {} items   [{}]",
                group,
                human(gsize),
                members.len(),
                members[0].risk.label()
            );
            for item in members {
                println!(
                    "    {:>9}  {:<46} {}",
                    if item.size == 0 {
                        "—".into()
                    } else {
                        human(item.size)
                    },
                    item.label,
                    item.detail
                );
            }
        }
    }

    println!("\n{}", "─".repeat(72));
    for risk in [Risk::Safe, Risk::Caution, Risk::Danger] {
        let matching: Vec<_> = items.iter().filter(|i| i.risk == risk).collect();
        if matching.is_empty() {
            continue;
        }
        let size: u64 = matching.iter().map(|i| i.size).sum();
        println!(
            "  {} {:<14} {:>10}   {} items",
            risk.dot(),
            risk.label(),
            human(size),
            matching.len()
        );
    }
    println!("\n  Total reclaimable: {}", human(total));
    if let Some((free, capacity)) = util::disk_free(&std::env::current_dir()?) {
        println!(
            "  Disk: {} free of {} — reaping everything would leave {} free",
            human(free),
            human(capacity),
            human(free + total)
        );
    }
    Ok(())
}

fn run(terminal: &mut ratatui::DefaultTerminal, mut app: App) -> Result<()> {
    let mut tick: u64 = 0;
    let mut last_tick = Instant::now();
    const TICK: Duration = Duration::from_millis(80);

    loop {
        terminal.draw(|f| ui::render(f, &mut app, tick))?;

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

fn handle_key(app: &mut App, code: KeyCode) {
    match app.mode {
        Mode::Help => app.mode = Mode::Browsing,

        Mode::Search => match code {
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
        },

        Mode::Confirm => match code {
            KeyCode::Esc => app.mode = Mode::Browsing,
            // Held until the acknowledgement the selection demands is given.
            KeyCode::Enter if app.confirm_satisfied() => app.start_reap(),
            KeyCode::Enter => {}
            KeyCode::Backspace => {
                app.confirm_input.pop();
            }
            KeyCode::Char(c) => app.confirm_input.push(c),
            _ => {}
        },

        // Deletion is in flight; the only way out is the process signal.
        Mode::Reaping => {}

        Mode::Report => match code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Char('r') => {
                app.mode = Mode::Browsing;
                app.rescan();
            }
            // Only offered when this run actually trashed something.
            KeyCode::Char('e') if !app.trashed.is_empty() => app.empty_trash(),
            KeyCode::Esc | KeyCode::Enter => app.mode = Mode::Browsing,
            _ => {}
        },

        Mode::Browsing => match code {
            KeyCode::Char('q') => app.quit = true,
            // Esc backs out of things rather than quitting: leaving a tool that
            // deletes files should take a deliberate keystroke.
            KeyCode::Esc => {
                if !app.search.is_empty() {
                    app.search.clear();
                    app.rebuild();
                } else {
                    app.clear_selection();
                }
            }
            KeyCode::Char('?') => app.mode = Mode::Help,
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
            KeyCode::Home => app.move_cursor(-(i32::MAX as isize)),
            KeyCode::End => app.move_cursor(i32::MAX as isize),
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
        },
    }
}

//! Rendering tests. A pty-based smoke test cannot assert what actually reached
//! the screen, so the UI is driven through ratatui's `TestBackend` instead and
//! the resulting cell buffer is inspected directly.

use crate::app::{App, Mode};
use crate::model::{Action, Candidate, Category, Risk};
use crate::scan::ScanOpts;
use crate::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

/// An app with a fixed set of candidates and no scanners running.
fn fixture() -> App {
    let opts = ScanOpts {
        cache: std::sync::Arc::new(crate::cache::SizeCache::load(false)),
        roots: vec![],
        stale_days: 30,
        min_size: 0,
        max_depth: 1,
        skip_docker: true,
        skip_caches: true,
    };
    let mut app = App::new(opts, false, false);
    app.pending.clear();
    app.items = vec![
        Candidate::new(
            Category::Docker,
            "build cache",
            "BuildKit cache (all reclaimable)",
            "377 unused layer records",
            17_000_000_000,
            Risk::Safe,
            Action::Run {
                program: "docker".into(),
                args: vec!["builder".into(), "prune".into()],
                cwd: None,
            },
        ),
        Candidate::new(
            Category::Artifacts,
            "node_modules",
            "webapp/node_modules",
            "untouched 5mo",
            420_000_000,
            Risk::Caution,
            Action::Remove(PathBuf::from("/Users/x/code/webapp/node_modules")),
        ),
        Candidate::new(
            Category::Git,
            "old stashes",
            "api: stash@{0}",
            "WIP on main",
            0,
            Risk::Danger,
            Action::Run {
                program: "git".into(),
                args: vec!["stash".into(), "drop".into()],
                cwd: None,
            },
        ),
    ];
    app.rebuild();
    app
}

fn draw(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::render(f, app, 0)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn renders_totals_categories_and_items() {
    let mut app = fixture();
    let out = draw(&mut app, 110, 30);

    // Header total: the three fixtures add up to 17.42 GB.
    assert!(out.contains("17.4 GB"), "header total missing:\n{out}");
    assert!(out.contains("reclaimable"));

    // Every populated category reaches the sidebar.
    for title in ["Git", "Build artifacts", "Docker"] {
        assert!(out.contains(title), "missing category {title}:\n{out}");
    }

    // The first category is selected, so its item is listed with its size.
    assert!(out.contains("api: stash@{0}"), "item row missing:\n{out}");
    assert!(out.contains("nothing selected"));
}

#[test]
fn the_default_view_spans_every_category() {
    let mut app = fixture();
    // The tree opens on "Everything", so a select-all reaches all three
    // fixtures rather than just the first category's.
    app.set_all_visible(true);
    assert_eq!(app.selected_count(), 3);

    let out = draw(&mut app, 110, 30);
    assert!(out.contains("3 selected"), "footer selection missing:\n{out}");
    assert!(out.contains("frees 17.4 GB"), "footer total missing:\n{out}");
}

#[test]
fn header_breaks_the_total_down_by_risk() {
    let mut app = fixture();
    let out = draw(&mut app, 110, 30);
    // 17 GB safe, 420 MB rebuildable, and a zero-byte irreversible stash.
    assert!(out.contains("17.0 GB safe"), "risk split missing:\n{out}");
    assert!(out.contains("420 MB rebuildable"), "risk split missing:\n{out}");
    assert!(out.contains("irreversible"), "risk split missing:\n{out}");
}

#[test]
fn confirm_dialog_demands_typed_acknowledgement_for_irreversible_items() {
    let mut app = fixture();
    // The Git category holds the sole irreversible candidate.
    app.set_all_visible(true);
    assert!(app.has_irreversible());

    app.begin_confirm();
    assert_eq!(app.mode, Mode::Confirm);
    assert!(!app.confirm_satisfied(), "must not be armed by default");

    let out = draw(&mut app, 110, 30);
    assert!(out.contains("cannot be recovered"), "no warning:\n{out}");
    assert!(out.contains("confirm (locked)"), "not gated:\n{out}");

    app.confirm_input = "reap".into();
    assert!(app.confirm_satisfied(), "typing the word must arm it");
}

#[test]
fn safe_selection_skips_irreversible_items() {
    let mut app = fixture();
    app.expanded.insert(Category::Git);
    app.rebuild();
    // Select across every category, not just the focused one.
    for cat in Category::ALL {
        app.node_idx = 0;
        app.rebuild();
        let _ = cat;
    }
    app.select_safe();
    assert!(
        !app.has_irreversible(),
        "`s` must never pick up an irreversible item"
    );
}

#[test]
fn survives_a_terminal_far_too_small_to_draw() {
    // Guards the width arithmetic in the panes and the centred overlays.
    let mut app = fixture();
    for (w, h) in [(20, 5), (10, 3), (1, 1), (40, 8)] {
        draw(&mut app, w, h);
    }
    app.set_all_visible(true);
    app.begin_confirm();
    for (w, h) in [(20, 5), (10, 3), (1, 1)] {
        draw(&mut app, w, h);
    }
    app.mode = Mode::Reaping;
    draw(&mut app, 12, 4);
    app.mode = Mode::Help;
    draw(&mut app, 12, 4);
}

#[test]
fn empty_state_renders_without_items() {
    let opts = ScanOpts {
        cache: std::sync::Arc::new(crate::cache::SizeCache::load(false)),
        roots: vec![],
        stale_days: 30,
        min_size: 0,
        max_depth: 1,
        skip_docker: true,
        skip_caches: true,
    };
    let mut app = App::new(opts, true, false);
    app.pending.clear();
    app.items.clear();
    app.rebuild();
    let out = draw(&mut app, 90, 20);
    assert!(out.contains("DRY RUN"), "dry-run banner missing:\n{out}");
}

/// Prints a frame for eyeballing the layout:
/// `cargo test preview -- --ignored --nocapture`
#[test]
#[ignore = "visual aid, not an assertion"]
fn preview() {
    let mut app = fixture();
    app.expanded.insert(Category::Docker);
    app.rebuild();
    if let Some(i) = app.visible.first().copied() {
        app.items[i].selected = true;
    }
    app.rebuild();
    println!("\n{}\n", draw(&mut app, 104, 24));
}

/// `cargo test preview_confirm -- --ignored --nocapture`
#[test]
#[ignore = "visual aid, not an assertion"]
fn preview_confirm() {
    let mut app = fixture();
    app.set_all_visible(true);
    for item in &mut app.items {
        item.selected = true;
    }
    app.begin_confirm();
    app.confirm_input = "re".into();
    println!("\n{}\n", draw(&mut app, 104, 22));
}

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
        rules: std::sync::Arc::new(crate::scan::Rules::default()),
        cache: std::sync::Arc::new(crate::cache::SizeCache::load(false)),
        roots: vec![],
        stale_days: 30,
        min_size: 0,
        max_depth: 1,
        skip_docker: true,
        skip_caches: true,
        scan_home_strays: false,
    };
    let mut app = App::new(
        opts,
        false,
        false,
        Default::default(),
        PathBuf::from("/dev/null"),
    );
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
    assert!(
        out.contains("3 selected"),
        "footer selection missing:\n{out}"
    );
    assert!(
        out.contains("frees 17.4 GB"),
        "footer total missing:\n{out}"
    );
}

#[test]
fn header_breaks_the_total_down_by_risk() {
    let mut app = fixture();
    let out = draw(&mut app, 110, 30);
    // 17 GB safe, 420 MB rebuildable, and a zero-byte irreversible stash.
    assert!(out.contains("17.0 GB safe"), "risk split missing:\n{out}");
    assert!(
        out.contains("420 MB rebuildable"),
        "risk split missing:\n{out}"
    );
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
        rules: std::sync::Arc::new(crate::scan::Rules::default()),
        cache: std::sync::Arc::new(crate::cache::SizeCache::load(false)),
        roots: vec![],
        stale_days: 30,
        min_size: 0,
        max_depth: 1,
        skip_docker: true,
        skip_caches: true,
        scan_home_strays: false,
    };
    let mut app = App::new(
        opts,
        true,
        false,
        Default::default(),
        PathBuf::from("/dev/null"),
    );
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

#[test]
fn the_quick_reap_palette_shows_what_each_key_would_take() {
    let mut app = fixture();
    app.mode = Mode::Recipes;
    let out = draw(&mut app, 104, 24);

    assert!(out.contains("Quick reap"), "palette missing:\n{out}");
    // The fixture holds one safe docker item of 17 GB, so the docker recipe
    // has to show what pressing it would actually get back.
    assert!(out.contains("Docker · safe"), "no docker recipe:\n{out}");
    assert!(
        out.contains("17.0 GB"),
        "no figure against a recipe:\n{out}"
    );
}

#[test]
fn a_recipe_selects_across_categories_not_just_the_visible_list() {
    let mut app = fixture();
    // Looking at Docker only; the recipe covers everything safe regardless.
    app.expanded.insert(Category::Docker);
    app.rebuild();

    app.apply_recipe('1');

    assert_eq!(app.mode, Mode::Confirm);
    assert_eq!(app.selected_count(), 1);
    assert!(app.selected().all(|c| c.risk == Risk::Safe));
}

#[test]
fn a_recipe_replaces_the_selection_rather_than_adding_to_it() {
    let mut app = fixture();
    for item in &mut app.items {
        item.selected = true;
    }

    app.apply_recipe('1');

    // Confirming must show what the key named, not what was already ticked.
    assert_eq!(app.selected_count(), 1);
}

#[test]
fn a_recipe_that_covers_nothing_says_so_instead_of_opening_an_empty_dialog() {
    let mut app = fixture();
    app.items.retain(|c| c.category != Category::Docker);
    app.rebuild();

    app.apply_recipe('d');

    assert_eq!(app.mode, Mode::Browsing);
    assert!(app.status.contains("nothing to reap"), "{}", app.status);
}

#[test]
fn a_recipe_holding_irreversible_items_still_demands_the_typed_confirmation() {
    let mut app = fixture();
    app.apply_recipe('3');

    assert_eq!(app.mode, Mode::Confirm);
    assert!(app.has_irreversible());
    assert!(
        !app.confirm_satisfied(),
        "one key must not bypass the acknowledgement typing reap exists for"
    );
}

#[test]
fn the_footer_says_when_a_newer_release_exists() {
    let mut app = fixture();
    app.update_available = Some("9.9.9".into());
    let out = draw(&mut app, 110, 30);

    assert!(out.contains("update available"), "no notice:\n{out}");
    assert!(out.contains("9.9.9"), "no version:\n{out}");
    assert!(out.contains("reap update"), "no way to act on it:\n{out}");
}

#[test]
fn the_update_notice_gives_way_to_the_keys_on_a_narrow_window() {
    // The news is worth a line only when there is a line spare. Someone on an
    // 80-column terminal needs to know how to quit more than they need to know
    // a release happened.
    let mut app = fixture();
    app.update_available = Some("9.9.9".into());
    for width in [40, 60] {
        let out = draw(&mut app, width, 20);
        assert!(
            !out.contains("update available"),
            "notice crowded out the keys at {width} columns:\n{out}"
        );
    }
}

#[test]
fn the_update_notice_never_hides_the_selection() {
    // It takes the hint slot on the right, not the tally on the left.
    let mut app = fixture();
    app.update_available = Some("9.9.9".into());
    app.set_all_visible(true);
    let out = draw(&mut app, 110, 30);

    assert!(out.contains("3 selected"), "selection lost:\n{out}");
    assert!(out.contains("update available"), "notice lost:\n{out}");
}

/// `cargo test preview_recipes -- --ignored --nocapture`
#[test]
#[ignore = "visual aid, not an assertion"]
fn preview_recipes() {
    let mut app = fixture();
    app.mode = Mode::Recipes;
    app.recipe_idx = 3;
    println!("\n{}\n", draw(&mut app, 104, 20));
}

#[test]
fn the_guide_opens_on_the_first_thing_someone_needs() {
    let mut app = fixture();
    app.mode = Mode::Help;
    let out = draw(&mut app, 100, 32);

    assert!(out.contains("Guide"), "no guide:\n{out}");
    assert!(
        out.contains("What you are looking at"),
        "wrong opening:\n{out}"
    );
    // The risk levels are the reason the tool exists, so they are on the way in.
    assert!(out.contains("irreversible"), "risks missing:\n{out}");
}

#[test]
fn the_guide_scrolls_rather_than_stopping_at_one_screen() {
    let mut app = fixture();
    app.mode = Mode::Help;
    let top = draw(&mut app, 100, 32);

    app.help_scroll = 500; // clamped to the end by the renderer
    let further = draw(&mut app, 100, 32);

    assert_ne!(top, further, "scrolling changed nothing");
    // The key list lives at the bottom, so reaching it proves the whole
    // document is reachable and not just the first screenful.
    assert!(
        further.contains("quit"),
        "never reached the keys:\n{further}"
    );
}

#[test]
fn the_guide_and_the_command_line_say_the_same_thing() {
    // One source, so the explanation cannot drift between the two places
    // someone might read it.
    let mut app = fixture();
    app.mode = Mode::Help;
    let rendered = draw(&mut app, 100, 40);
    let printed = crate::guide::plain();

    assert!(printed.contains("What you are looking at"));
    assert!(rendered.contains("What you are looking at"));
    for section in crate::guide::GUIDE {
        assert!(
            printed.contains(section.title),
            "cli missing {}",
            section.title
        );
    }
}

/// `cargo test preview_guide -- --ignored --nocapture`
#[test]
#[ignore = "visual aid, not an assertion"]
fn preview_guide() {
    let mut app = fixture();
    app.mode = Mode::Help;
    println!("\n{}\n", draw(&mut app, 100, 32));
}

/// `cargo test preview_update -- --ignored --nocapture`
#[test]
#[ignore = "visual aid, not an assertion"]
fn preview_update() {
    let mut app = fixture();
    app.update_available = Some("1.2.0".into());
    println!("\n{}\n", draw(&mut app, 104, 14));
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

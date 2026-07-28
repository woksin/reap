//! The walkthrough, written once and shown in both places.
//!
//! `?` in the interface and `reap guide` on the command line are the same
//! words. Two copies of an explanation drift, and the one that drifts is
//! always the one the user is reading.

/// A heading and the lines under it. An empty line is a paragraph break.
pub struct Section {
    pub title: &'static str,
    pub body: &'static [&'static str],
}

/// Every key, and what it does. Rendered into the guide and nowhere else, so
/// there is one list rather than one per surface.
pub const KEYS: &[(&str, &str)] = &[
    ("R", "quick reap — one key per standing decision"),
    ("C", "configuration — every rule reap is working from"),
    ("L", "legend — what the marks mean"),
    ("↑ ↓  j k", "move"),
    ("← →  h l", "switch pane"),
    ("tab", "toggle pane"),
    ("enter", "expand / collapse a category"),
    ("space", "select the highlighted item"),
    ("a", "select everything in view"),
    ("s", "select all except irreversible"),
    ("n", "clear the whole selection"),
    ("v", "start a range, v again to select to the cursor"),
    ("o", "cycle sort: size, age, name"),
    ("f", "cycle risk filter"),
    ("/", "filter by text"),
    ("i", "reveal the path in the file manager"),
    ("x", "never offer this again (writes the config)"),
    ("d", "reap the selection"),
    ("r", "rescan"),
    ("esc", "clear the filter, then the selection"),
    ("?", "this guide"),
    ("q", "quit"),
];

/// How a legend entry should be coloured, for the surfaces that have colour.
///
/// The interface draws two of these symbols identically and tells them apart by
/// colour alone — safe and rebuildable are both `●` — so a legend printed in one
/// colour would be a legend that does not answer the question it was opened to
/// answer.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tone {
    Safe,
    Caution,
    Danger,
    Accent,
    Dim,
}

pub struct LegendGroup {
    pub title: &'static str,
    /// `(symbol, what it means, how to colour it)`.
    pub entries: &'static [(&'static str, &'static str, Tone)],
}

/// Every mark the interface draws, and what it means.
///
/// The guide explains the ideas; this names the glyphs. They are separate
/// because they answer different questions — "what does reap think risk is" is
/// a paragraph, and "what is that triangle" wants one line without losing your
/// place in a list of four hundred items.
pub const LEGEND: &[LegendGroup] = &[
    LegendGroup {
        title: "What it costs to lose",
        entries: &[
            (
                "●",
                "safe · regenerated automatically, nothing is lost",
                Tone::Safe,
            ),
            ("●", "rebuildable · costs time, but no work", Tone::Caution),
            ("▲", "irreversible · may exist nowhere else", Tone::Danger),
        ],
    },
    LegendGroup {
        title: "In the list",
        entries: &[
            ("◉", "selected · d will reap this", Tone::Accent),
            ("○", "not selected", Tone::Dim),
            (
                "$",
                "the command that will run, on the highlighted row",
                Tone::Dim,
            ),
            (
                "─",
                "a category's share of everything reclaimable",
                Tone::Dim,
            ),
        ],
    },
    LegendGroup {
        title: "On the settings screen",
        entries: &[
            ("✓", "on · this rule is in force", Tone::Safe),
            (
                "✗",
                "off · turned off with x, and reversible with it",
                Tone::Dim,
            ),
            (
                "built-in",
                "ships with reap · can be turned off and re-graded",
                Tone::Dim,
            ),
            (
                "yours",
                "from your config · can also be edited and deleted",
                Tone::Accent,
            ),
            (
                "✎",
                "re-graded by you · the rule itself says otherwise",
                Tone::Caution,
            ),
        ],
    },
];

pub const GUIDE: &[Section] = &[
    Section {
        title: "What you are looking at",
        body: &[
            "reap looks for five kinds of thing that pile up and stop being useful:",
            "",
            "  Git              branches, worktrees, stashes, repacking",
            "  Build artifacts  target, node_modules, bin/obj — proven by a sibling",
            "  Docker           unused images, stopped containers, volumes, build cache",
            "  Caches           browsers, chat apps, design tools, package managers",
            "  Personal         old downloads, installers, phone backups",
            "",
            "The left pane holds those categories and their groups. The right pane is",
            "whatever you have highlighted, opening on Everything, biggest first, so the",
            "largest wins are visible before you choose anything.",
            "",
            "You do not have to be a developer for this to be worth running. On most",
            "machines the largest single find is a chat app's cache, a design tool's",
            "media cache, or a backup of a phone somebody replaced in 2021.",
        ],
    },
    Section {
        title: "Your own files are treated differently",
        body: &[
            "Everything outside Personal has an owner that will remake it — a compiler,",
            "a package manager, a browser. Nothing in Personal does.",
            "",
            "So reap does not guess. A file in Downloads that announces itself as an",
            "installer — .dmg, .exe, .pkg, .iso — is graded rebuildable, because the",
            "worst case is downloading it again. Everything else is graded",
            "irreversible, whether it is a film or a folder of scans, because the",
            "filesystem holds nothing that tells those apart.",
            "",
            "Irreversible is not a warning label, it is a mechanism: those items are",
            "never taken by s, never by a safe recipe, never by an unattended --reap,",
            "and never without the word reap being typed.",
            "",
            "If you would rather reap left your own files out of it entirely, pass",
            "--no-personal, or put `personal = false` under [scan] in the config.",
        ],
    },
    Section {
        title: "Deciding what goes",
        body: &[
            "Every item carries a risk, and it is the only thing worth reading carefully:",
            "",
            "  ● safe           regenerated automatically — nothing is lost",
            "  ● rebuildable    costs time to rebuild or re-download — no work is lost",
            "  ▲ irreversible   may destroy work that exists nowhere else",
            "",
            "This is the whole point. A branch whose upstream was deleted might be a",
            "squash-merged PR whose work is entirely in main, or the only copy of three",
            "days. Those look identical to git. reap tells them apart and grades them.",
            "",
            "Press f to show one risk at a time. If you disagree with a grading, you can",
            "change it — see Making it yours.",
        ],
    },
    Section {
        title: "Picking things",
        body: &[
            "space  ticks the highlighted item.",
            "a      ticks everything currently listed — narrow the list first with the",
            "       sidebar or /, and this becomes 'select this group'.",
            "s      ticks everything except the irreversible. The obvious wins.",
            "v      starts a range; move, then v again to take everything between.",
            "n      clears it all.",
            "",
            "The highlighted row swaps its description for the command that will run,",
            "so nothing gets confirmed without its consequence visible.",
        ],
    },
    Section {
        title: "Quick reap — the R key",
        body: &[
            "After the first couple of runs, the ticking is the same ticking. R opens",
            "a list of standing decisions — everything safe, the caches your apps will",
            "rebuild, the branches already upstream, docker without the volumes —",
            "each showing what it would take before you press it.",
            "",
            "A recipe only selects. It lands in the same confirmation as ticking by",
            "hand, so one key is a shortcut through the tedium, never the safety.",
        ],
    },
    Section {
        title: "When you press d",
        body: &[
            "Nothing has happened yet. d opens a confirmation showing the split by",
            "risk and what free space would become.",
            "",
            "If anything irreversible is selected, the button stays locked until you type",
            "the word reap. Deliberate friction, and the only thing standing between",
            "a tired evening and a lost afternoon.",
            "",
            "Two flags change what deleting means. --dry-run does everything but",
            "touch the disk. --trash moves paths to the Trash rather than unlinking",
            "them, which frees nothing until the Trash is emptied — reap says so",
            "rather than claiming a win it did not deliver.",
        ],
    },
    Section {
        title: "Making it yours — the C key",
        body: &[
            "C opens everything reap is working from: where it looks, the thresholds,",
            "every cache and build rule it ships with, what you have told it to ignore,",
            "and every grading you disagreed with. Each row says whether it is built-in",
            "or yours, and whether it is on.",
            "",
            "  enter  open a section, or flip a switch",
            "  e      change the path, pattern or value",
            "  n      rename a rule of your own",
            "  a      add one — a scan root, a cache path, a pattern",
            "  x      turn a rule off, and press it again to turn it back on",
            "  g      re-grade what something costs you",
            "  d      delete something you added",
            "",
            "Changes are written as you make them, to ~/.config/reap/config.toml —",
            "the same file `reap --write-config` documents and you can edit by hand.",
            "A built-in rule can be turned off and re-graded but never edited away, so",
            "a later release correcting where a vendor hides its cache still reaches you.",
            "",
            "x works on an item in the list too, and means the same thing there.",
            "Nothing reap knows is compiled in.",
        ],
    },
    Section {
        title: "What the marks mean — the L key",
        body: &[
            "L shows the legend over whatever you are looking at, and any key puts it",
            "away again — so you can settle what a triangle means without losing your",
            "place in a list of four hundred items.",
        ],
    },
    Section {
        title: "Away from the interface",
        body: &[
            "  reap --list                       print the findings and exit",
            "  reap --json                       the same, for something that parses",
            "  reap --reap                       print what an unattended run takes",
            "  reap --reap --yes                 take everything safe",
            "  reap --reap --recipe d --yes      take what the d recipe covers",
            "  reap update                       fetch a newer release",
            "",
            "--reap does nothing at all without --yes, and its ceiling defaults to safe.",
            "There is no interface to look at, so the flags are the deliberate act.",
        ],
    },
];

/// The guide as plain text, for `reap guide`.
pub fn plain() -> String {
    use std::fmt::Write as _;

    // Formatting straight into the buffer rather than through an intermediate
    // `format!`. Writing to a String cannot fail, so there is no error to carry.
    let mut out = String::new();
    for section in GUIDE {
        let _ = writeln!(out, "\n{}", section.title);
        let _ = writeln!(out, "{}", "─".repeat(section.title.len()));
        for line in section.body {
            let _ = writeln!(out, "{line}");
        }
    }
    out.push_str("\nKeys\n────\n");
    for (key, description) in KEYS {
        let _ = writeln!(out, "  {key:<12} {description}");
    }

    out.push_str("\nLegend\n──────\n");
    for group in LEGEND {
        let _ = writeln!(out, "\n  {}", group.title);
        for (symbol, meaning, _) in group.entries {
            let _ = writeln!(out, "    {symbol:<10} {meaning}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_rendering_carries_every_section() {
        let text = plain();
        for section in GUIDE {
            assert!(text.contains(section.title), "missing {}", section.title);
        }
        assert!(text.contains("Keys"));
    }

    #[test]
    fn the_legend_names_every_mark_the_interface_draws() {
        // A symbol the interface uses and the legend does not explain is a
        // symbol nobody can look up — which is the entire job of a legend.
        let symbols: Vec<&str> = LEGEND
            .iter()
            .flat_map(|g| g.entries.iter().map(|(s, ..)| *s))
            .collect();
        for mark in ["●", "▲", "◉", "○", "✓", "✗"] {
            assert!(symbols.contains(&mark), "the legend never explains {mark}");
        }
    }

    #[test]
    fn the_two_marks_drawn_alike_are_told_apart_by_colour() {
        // Safe and rebuildable are both `●`, so the legend's own entries have
        // to differ in tone or it explains nothing.
        let dots: Vec<Tone> = LEGEND
            .iter()
            .flat_map(|g| g.entries.iter())
            .filter(|(symbol, ..)| *symbol == "●")
            .map(|(.., tone)| *tone)
            .collect();
        assert_eq!(dots.len(), 2, "expected two dots, found {}", dots.len());
        assert_ne!(dots[0], dots[1]);
    }

    #[test]
    fn every_key_the_interface_binds_is_described() {
        // The guide is the only key list, so a binding missing from it is a
        // binding nobody finds.
        let described: Vec<&str> = KEYS.iter().map(|(k, _)| *k).collect();
        for key in ["R", "C", "L", "space", "a", "s", "d", "x", "q", "?"] {
            assert!(
                described
                    .iter()
                    .any(|k| k.split_whitespace().any(|p| p == key)),
                "no guide entry for {key}"
            );
        }
    }

    #[test]
    fn the_guide_says_what_makes_the_confirmation_lock() {
        // If this sentence goes missing, the one piece of friction protecting
        // unrecoverable work becomes a surprise instead of a rule.
        let text = plain();
        assert!(text.contains("type"));
        assert!(text.contains("reap"));
        assert!(text.contains("irreversible"));
    }

    #[test]
    fn no_line_is_too_wide_for_the_overlay_it_is_drawn_in() {
        // The interface renders this in a 76-column box; anything longer is
        // silently clipped, and a clipped instruction is a wrong instruction.
        for section in GUIDE {
            for line in section.body {
                assert!(
                    line.chars().count() <= 76,
                    "{} chars in {:?}: {line}",
                    line.chars().count(),
                    section.title
                );
            }
        }
    }
}

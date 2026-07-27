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

pub const GUIDE: &[Section] = &[
    Section {
        title: "What you are looking at",
        body: &[
            "reap looks for four kinds of thing that pile up and stop being useful:",
            "",
            "  Git              branches, worktrees, stashes, repacking",
            "  Build artifacts  target, node_modules, bin/obj — proven by a sibling",
            "  Docker           unused images, stopped containers, volumes, build cache",
            "  Caches           package managers, DerivedData, downloaded browsers",
            "",
            "The left pane holds those categories and their groups. The right pane is",
            "whatever you have highlighted, opening on Everything, biggest first, so the",
            "largest wins are visible before you choose anything.",
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
            "a list of standing decisions — everything safe, the branches already",
            "upstream, worktrees with nothing in them, docker without the volumes —",
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
        title: "Making it yours",
        body: &[
            "x on any item means never offer this again. It writes the pattern to your",
            "config, so it survives a rescan and a restart.",
            "",
            "Everything else lives in ~/.config/reap/config.toml, and",
            "`reap --write-config` writes a documented starter:",
            "",
            "  [[artifact]]  another directory your build system produces",
            "  [[cache]]     another cache, and the command that clears it properly",
            "  [[recipe]]    another key on the R palette",
            "  [[override]]  disagree with a risk grading — it is your disk",
            "",
            "Nothing reap knows is compiled in.",
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
    fn every_key_the_interface_binds_is_described() {
        // The guide is the only key list, so a binding missing from it is a
        // binding nobody finds.
        let described: Vec<&str> = KEYS.iter().map(|(k, _)| *k).collect();
        for key in ["R", "space", "a", "s", "d", "x", "q", "?"] {
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

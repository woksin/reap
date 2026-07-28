use crate::app::{App, Focus, Mode, Node};
use crate::model::{Category, Risk};
use crate::util::{human, human_age, rows};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap,
};

const ACCENT: Color = Color::Rgb(125, 211, 252);
const DIM: Color = Color::Rgb(110, 118, 129);
const SAFE: Color = Color::Rgb(134, 239, 172);
const CAUTION: Color = Color::Rgb(250, 204, 21);
const DANGER: Color = Color::Rgb(248, 113, 113);
const SELECTED: Color = Color::Rgb(196, 181, 253);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const fn risk_color(risk: Risk) -> Color {
    match risk {
        Risk::Safe => SAFE,
        Risk::Caution => CAUTION,
        Risk::Danger => DANGER,
    }
}

const fn category_color(cat: Category) -> Color {
    match cat {
        Category::Git => Color::Rgb(244, 143, 177),
        Category::Artifacts => Color::Rgb(129, 199, 245),
        Category::Docker => Color::Rgb(126, 231, 219),
        Category::Caches => Color::Rgb(252, 191, 122),
        Category::Personal => Color::Rgb(196, 181, 253),
    }
}

fn block(title: &str, focused: bool) -> Block<'_> {
    let border = if focused { ACCENT } else { DIM };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border))
        .title(Line::from(vec![
            Span::styled("─ ", Style::new().fg(border)),
            Span::styled(
                title.to_string(),
                Style::new()
                    .fg(if focused { ACCENT } else { DIM })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ─", Style::new().fg(border)),
        ]))
}

/// Lay `left` and `right` out on one line with the gap between them.
///
/// When the two do not fit, the left side is trimmed rather than letting the
/// line overflow: the size on the right is the number being scanned for, so it
/// has to stay readable at any width.
fn justify<'a>(left: Vec<Span<'a>>, right: Vec<Span<'a>>, width: usize) -> Line<'a> {
    let span_width = |s: &Span<'_>| s.content.chars().count();
    let right_w: usize = right.iter().map(span_width).sum();
    let left_w: usize = left.iter().map(span_width).sum();

    let mut spans = if left_w + right_w + 1 > width {
        let budget = width.saturating_sub(right_w + 2);
        let mut kept: Vec<Span<'_>> = Vec::new();
        let mut used = 0usize;
        for span in left {
            let w = span_width(&span);
            if used + w <= budget {
                used += w;
                kept.push(span);
            } else {
                let room = budget.saturating_sub(used);
                if room > 0 {
                    let text: String = span.content.chars().take(room).collect();
                    kept.push(Span::styled(text, span.style));
                }
                break;
            }
        }
        kept.push(Span::styled("…", Style::new().fg(DIM)));
        kept
    } else {
        left
    };

    let used: usize = spans.iter().map(span_width).sum();
    let gap = width.saturating_sub(used + right_w).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    Line::from(spans)
}

pub fn render(f: &mut Frame<'_>, app: &App, tick: u64) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .areas(f.area());

    render_header(f, app, header, tick);

    let [sidebar, items] =
        Layout::horizontal([Constraint::Length(36), Constraint::Min(20)]).areas(body);
    render_sidebar(f, app, sidebar);
    render_items(f, app, items);
    render_footer(f, app, footer);

    match app.mode {
        Mode::Confirm => render_confirm(f, app),
        Mode::Reaping => render_reaping(f, app),
        Mode::Report => render_report(f, app),
        Mode::Help => render_help(f, app),
        Mode::Recipes => render_recipes(f, app),
        Mode::Settings => render_settings(f, app),
        _ => {}
    }

    // Last, and over whatever is already there: the legend answers one question
    // about the screen you are on without taking you off it.
    if app.legend {
        render_legend(f, app);
    }
}

fn render_header(f: &mut Frame<'_>, app: &App, area: Rect, tick: u64) {
    // A bordered block's usable width is two columns narrower than its area.
    let inner = area.width.saturating_sub(2) as usize;

    let mut left = vec![
        Span::raw(" "),
        Span::styled("reap", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("  ", Style::new()),
    ];
    if app.scanning() {
        // Reduced in u64 first, so the index is in range before it is narrowed
        // and no cast can move it.
        let step = (tick / 2) % SPINNER.len() as u64;
        let frame = SPINNER[usize::try_from(step).unwrap_or(0)];
        left.push(Span::styled(frame, Style::new().fg(ACCENT)));
        left.push(Span::styled(
            format!(" {}", app.status),
            Style::new().fg(DIM),
        ));
    } else {
        left.push(Span::styled(
            format!("{} items found", app.items.len()),
            Style::new().fg(DIM),
        ));
    }
    if app.dry_run {
        left.push(Span::styled(
            "   DRY RUN",
            Style::new().fg(CAUTION).add_modifier(Modifier::BOLD),
        ));
    }
    if app.trash {
        left.push(Span::styled(
            "   TRASH",
            Style::new().fg(SELECTED).add_modifier(Modifier::BOLD),
        ));
    }

    let right = vec![
        Span::styled(
            human(app.total_size()),
            Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" reclaimable ", Style::new().fg(DIM)),
    ];

    // Second line: how the total splits by risk, and what it would do to the
    // disk. "How much can I get back safely" is the question behind the tool.
    let mut split: Vec<Span<'_>> = vec![Span::raw(" ")];
    for risk in [Risk::Safe, Risk::Caution, Risk::Danger] {
        let size = app.risk_size(risk);
        if app.risk_count(risk) == 0 {
            continue;
        }
        split.push(Span::styled(
            format!("{} ", risk.dot()),
            Style::new().fg(risk_color(risk)),
        ));
        split.push(Span::styled(
            human(size),
            Style::new()
                .fg(risk_color(risk))
                .add_modifier(Modifier::BOLD),
        ));
        split.push(Span::styled(
            format!(" {}   ", risk.label()),
            Style::new().fg(DIM),
        ));
    }

    let disk = match app.disk {
        Some((free, total)) => vec![
            Span::styled(human(free), Style::new().fg(Color::Rgb(200, 205, 215))),
            Span::styled(" free of ", Style::new().fg(DIM)),
            Span::styled(human(total), Style::new().fg(DIM)),
            Span::styled(" → ", Style::new().fg(DIM)),
            Span::styled(
                human(free + app.total_size()),
                Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ],
        None => vec![],
    };

    f.render_widget(
        Paragraph::new(vec![
            justify(left, right, inner),
            justify(split, disk, inner),
        ])
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(DIM)),
        ),
        area,
    );
}

/// The "everything" row: the cross-category total, above a rule.
fn everything_row(app: &App, width: usize) -> ListItem<'static> {
    let head = justify(
        vec![
            Span::styled("   ", Style::new()),
            Span::styled(
                "Everything",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" ({})", app.items.len()), Style::new().fg(DIM)),
        ],
        vec![Span::styled(
            format!("{} ", human(app.total_size())),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        )],
        width,
    );
    ListItem::new(vec![
        head,
        Line::from(Span::styled(
            format!("  {}", "─".repeat(width.saturating_sub(3))),
            Style::new().fg(Color::Rgb(55, 60, 70)),
        )),
    ])
}

/// A category row, with a bar showing its share of everything found.
fn category_row(app: &App, cat: Category, width: usize, total: u64) -> ListItem<'static> {
    let size = app.category_size(cat);
    let arrow = if app.expanded.contains(&cat) {
        "▾"
    } else {
        "▸"
    };
    let color = category_color(cat);
    let head = justify(
        vec![
            Span::styled(format!(" {arrow} "), Style::new().fg(color)),
            Span::styled(
                cat.title().to_string(),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({})", app.category_count(cat)),
                Style::new().fg(DIM),
            ),
        ],
        vec![Span::styled(
            format!("{} ", human(size)),
            Style::new().fg(color),
        )],
        width,
    );

    // Proportional bar so the biggest offender is obvious at a glance.
    let bar_w = width.saturating_sub(3);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a ratio scaled to a bar at most a terminal wide; the result \
                  is clamped to `bar_w` on the next line"
    )]
    let filled = ((size as f64 / total as f64) * bar_w as f64).round() as usize;
    let filled = filled.min(bar_w);
    let bar = Line::from(vec![
        Span::raw("  "),
        Span::styled("█".repeat(filled), Style::new().fg(color)),
        Span::styled(
            "─".repeat(bar_w - filled),
            Style::new().fg(Color::Rgb(55, 60, 70)),
        ),
    ]);
    ListItem::new(vec![head, bar])
}

/// A group row, indented under its category.
fn group_row(app: &App, cat: Category, group: &str, width: usize) -> ListItem<'static> {
    let size = app.group_size(cat, group);
    let count = app.group_count(cat, group);
    let line = justify(
        vec![
            Span::raw("     "),
            Span::styled(
                group.to_string(),
                Style::new().fg(Color::Rgb(200, 205, 215)),
            ),
            Span::styled(format!(" ({count})"), Style::new().fg(DIM)),
        ],
        vec![Span::styled(
            format!("{} ", human(size)),
            Style::new().fg(DIM),
        )],
        width,
    );
    ListItem::new(line)
}

fn render_sidebar(f: &mut Frame<'_>, app: &App, area: Rect) {
    let focused = app.focus == Focus::Sidebar;
    let width = area.width.saturating_sub(2) as usize;
    let total = app.total_size().max(1);

    let rows: Vec<ListItem<'static>> = app
        .nodes
        .iter()
        .map(|node| match node {
            Node::All => everything_row(app, width),
            Node::Category(cat) => category_row(app, *cat, width, total),
            Node::Group(cat, group) => group_row(app, *cat, group, width),
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(app.node_idx));
    let list = List::new(rows)
        .block(block("Categories", focused))
        .highlight_style(
            Style::new()
                .bg(Color::Rgb(38, 44, 56))
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, area, &mut state);
}

fn render_items(f: &mut Frame<'_>, app: &App, area: Rect) {
    let focused = app.focus == Focus::Items;
    let width = area.width.saturating_sub(2) as usize;

    let title = match app.nodes.get(app.node_idx) {
        Some(Node::All) => format!("Everything · biggest first ({})", app.visible.len()),
        Some(Node::Category(c)) => c.title().to_string(),
        Some(Node::Group(c, g)) => format!("{} › {}", c.title(), g),
        None => "Nothing found".to_string(),
    };
    let mut title = if app.search.is_empty() {
        title
    } else {
        format!("{title}  /{}", app.search)
    };
    if let Some(risk) = app.risk_filter {
        title = format!("{title}  ·  {} only", risk.label());
    }
    if app.range_anchor.is_some() {
        title = format!("{title}  ·  RANGE");
    }

    if app.visible.is_empty() {
        let msg = if app.scanning() {
            "scanning…"
        } else {
            "nothing here — this category came up clean"
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::new().fg(DIM)))
                .alignment(Alignment::Center)
                .block(block(&title, focused)),
            area,
        );
        return;
    }

    let rows: Vec<ListItem<'_>> = app
        .visible
        .iter()
        .map(|&i| {
            let item = &app.items[i];
            let (mark, mark_style) = if item.selected {
                ("◉", Style::new().fg(SELECTED).add_modifier(Modifier::BOLD))
            } else {
                ("○", Style::new().fg(DIM))
            };

            let size = if item.size == 0 {
                "—".to_string()
            } else {
                human(item.size)
            };
            let age = item.age_days.map(human_age).unwrap_or_default();

            let head = justify(
                vec![
                    Span::styled(format!(" {mark} "), mark_style),
                    Span::styled(
                        item.label.clone(),
                        Style::new()
                            .fg(Color::Rgb(230, 237, 243))
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                vec![
                    Span::styled(format!("{age:>6}  "), Style::new().fg(DIM)),
                    Span::styled(
                        format!("{size:>9}"),
                        Style::new().fg(if item.size == 0 { DIM } else { SAFE }),
                    ),
                    Span::styled(
                        format!("  {} ", item.risk.dot()),
                        Style::new().fg(risk_color(item.risk)),
                    ),
                ],
                width,
            );

            // Show the exact command on the highlighted row, so nothing is
            // confirmed without its consequence being visible first.
            let highlighted = app.visible.get(app.item_idx) == Some(&i);
            let detail = if highlighted {
                Line::from(vec![
                    Span::styled("   $ ", Style::new().fg(ACCENT)),
                    Span::styled(
                        truncate(&item.action.describe(), width.saturating_sub(6)),
                        Style::new().fg(Color::Rgb(200, 205, 215)),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        truncate(&item.detail, width.saturating_sub(4)),
                        Style::new().fg(DIM),
                    ),
                ])
            };

            ListItem::new(vec![head, detail])
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(app.item_idx));
    let list = List::new(rows)
        .block(block(&title, focused))
        .highlight_style(Style::new().bg(Color::Rgb(38, 44, 56)));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_footer(f: &mut Frame<'_>, app: &App, area: Rect) {
    // A bordered block's usable width is two columns narrower than its area.
    let inner = area.width.saturating_sub(2) as usize;

    let count = app.selected_count();
    let mut left = if count == 0 {
        vec![Span::styled(" nothing selected", Style::new().fg(DIM))]
    } else {
        vec![
            Span::styled(" ◉ ", Style::new().fg(SELECTED)),
            Span::styled(
                format!("{count} selected"),
                Style::new().fg(SELECTED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · frees ", Style::new().fg(DIM)),
            Span::styled(
                human(app.selected_size()),
                Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
            ),
        ]
    };
    if app.has_irreversible() {
        left.push(Span::styled(
            "  ▲ includes irreversible",
            Style::new().fg(DANGER),
        ));
    }

    // Drop hints rather than let them run under the border on a narrow window.
    let used: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let room = inner.saturating_sub(used + 2);
    // Takes the hint slot rather than adding a line: worth saying, not worth a
    // row of the window someone is reading. Dropped entirely when it will not
    // fit, since the keys matter more than the news.
    let update = app.update_available.as_ref().map(|latest| {
        format!(
            "update available {} → {latest} · reap update",
            env!("CARGO_PKG_VERSION")
        )
    });
    if app.mode != Mode::Search
        && let Some(update) = update.filter(|u| u.chars().count() <= room)
    {
        let right = vec![
            Span::styled(update, Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ];
        f.render_widget(
            Paragraph::new(justify(left, right, inner)).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(DIM)),
            ),
            area,
        );
        return;
    }

    let hints = if app.mode == Mode::Search {
        "type to filter · enter keep · esc clear".to_string()
    } else {
        // Widest first, and the terminal decides. Each step drops what can be
        // found another way before what cannot: sorting is discoverable from
        // the column headings, but nothing on screen hints that `C` exists.
        let full = format!(
            "R quick · C config · L legend · space pick · a all · o sort:{} · / find · d reap · ? help",
            app.sort.label()
        );
        let medium = "R quick · C config · space pick · a all · d reap · ? help".to_string();
        [full, medium, "d reap · ? help".to_string()]
            .into_iter()
            .find(|hint| hint.chars().count() <= room)
            .unwrap_or_else(|| "? help".to_string())
    };
    let right = vec![Span::styled(hints, Style::new().fg(DIM)), Span::raw(" ")];

    f.render_widget(
        Paragraph::new(justify(left, right, inner)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(DIM)),
        ),
        area,
    );
}

/// One line per risk level the selection actually contains.
///
/// Levels with nothing in them are left out rather than shown as zero: the
/// point of this block is what you are about to lose, and a row of zeroes
/// buries it.
fn risk_breakdown(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for risk in [Risk::Safe, Risk::Caution, Risk::Danger] {
        let matching: Vec<_> = app.selected().filter(|i| i.risk == risk).collect();
        if matching.is_empty() {
            continue;
        }
        let size: u64 = matching.iter().map(|i| i.size).sum();
        lines.push(Line::from(vec![
            Span::styled(
                format!("    {} ", risk.dot()),
                Style::new().fg(risk_color(risk)),
            ),
            Span::styled(
                format!("{:<14}", risk.label()),
                Style::new().fg(risk_color(risk)),
            ),
            Span::styled(
                format!(
                    "{:>3} item{}",
                    matching.len(),
                    if matching.len() == 1 { "" } else { "s" }
                ),
                Style::new().fg(DIM),
            ),
            Span::styled(format!("{:>12}", human(size)), Style::new().fg(DIM)),
        ]));
    }
    lines
}

fn render_confirm(f: &mut Frame<'_>, app: &App) {
    let irreversible = app.has_irreversible();

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Reaping ", Style::new().fg(Color::Rgb(230, 237, 243))),
            Span::styled(
                format!("{}", app.reap_total()),
                Style::new().fg(SELECTED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " items · frees ",
                Style::new().fg(Color::Rgb(230, 237, 243)),
            ),
            Span::styled(
                human(app.selected_size()),
                Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    lines.extend(risk_breakdown(app));

    if let Some((free, _)) = app.disk {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("    free space  ", Style::new().fg(DIM)),
            Span::styled(human(free), Style::new().fg(Color::Rgb(200, 205, 215))),
            Span::styled("  →  ", Style::new().fg(DIM)),
            Span::styled(
                human(free + app.selected_size()),
                Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    lines.push(Line::from(""));
    if app.dry_run {
        lines.push(Line::from(Span::styled(
            "  DRY RUN — nothing will actually be deleted.",
            Style::new().fg(CAUTION).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    if irreversible {
        lines.push(Line::from(Span::styled(
            "  ▲ Some selected items cannot be recovered.",
            Style::new().fg(DANGER).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled("    Type ", Style::new().fg(DIM)),
            Span::styled("reap", Style::new().fg(DANGER).add_modifier(Modifier::BOLD)),
            Span::styled(" to confirm:  ", Style::new().fg(DIM)),
            Span::styled(
                app.confirm_input.clone(),
                Style::new()
                    .fg(Color::Rgb(230, 237, 243))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("▏", Style::new().fg(ACCENT)),
        ]));
        lines.push(Line::from(""));
    }

    let ready = app.confirm_satisfied();
    lines.push(Line::from(vec![
        Span::styled(
            "  enter ",
            Style::new()
                .fg(if ready { SAFE } else { DIM })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if ready { "confirm" } else { "confirm (locked)" },
            Style::new().fg(if ready { SAFE } else { DIM }),
        ),
        Span::styled("   esc ", Style::new().fg(DIM).add_modifier(Modifier::BOLD)),
        Span::styled("cancel", Style::new().fg(DIM)),
    ]));
    lines.push(Line::from(""));

    // Size the dialog to what it actually contains rather than a guess.
    let area = centered(64, rows(lines.len()).saturating_add(2), f.area());
    f.render_widget(Clear, area);

    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(if irreversible { DANGER } else { ACCENT }))
                .title(Line::from(Span::styled(
                    " Confirm ",
                    Style::new()
                        .fg(if irreversible { DANGER } else { ACCENT })
                        .add_modifier(Modifier::BOLD),
                ))),
        ),
        area,
    );
}

fn render_reaping(f: &mut Frame<'_>, app: &App) {
    let area = centered(60, 8, f.area());
    f.render_widget(Clear, area);

    let done = app.reap_log.len();
    let total = app.reap_total().max(1);
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts of selected items, nowhere near the 2^53 where an f64 \
                  stops counting exactly"
    )]
    let ratio = (done as f64 / total as f64).clamp(0.0, 1.0);

    let outer = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Line::from(Span::styled(
            " Reaping ",
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let [gauge_area, text_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(inner);

    f.render_widget(
        Gauge::default()
            .gauge_style(Style::new().fg(SAFE).bg(Color::Rgb(45, 51, 62)))
            .ratio(ratio)
            .label(format!("{done} / {total}")),
        gauge_area,
    );

    let current = app
        .reap_log
        .last()
        .map_or_else(|| "starting…".into(), |(l, _, _)| l.clone());
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate(&current, (inner.width as usize).saturating_sub(4))
                ),
                Style::new().fg(DIM),
            )),
            Line::from(vec![
                Span::styled("  freed so far: ", Style::new().fg(DIM)),
                Span::styled(
                    human(app.freed),
                    Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
                ),
            ]),
        ]),
        text_area,
    );
}

/// What went into the trash, and the one key that empties it.
///
/// Offered only for what this run put there, so pressing it can never reach
/// something the user trashed themselves earlier.
fn trash_notice(count: usize) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("  ", Style::new()),
            Span::styled(
                format!("{count} items are recoverable from the Trash."),
                Style::new().fg(SELECTED),
            ),
        ]),
        Line::from(vec![
            Span::styled("  e ", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(
                "delete just those permanently and reclaim the space",
                Style::new().fg(DIM),
            ),
        ]),
        Line::from(""),
    ]
}

/// The first few failures, with the reason each gave.
///
/// Capped so a run that failed wholesale still leaves the summary and the keys
/// visible rather than pushing them off a dialog sized to fit.
fn failure_lines(failures: &[&crate::app::ReapLogEntry]) -> Vec<Line<'static>> {
    if failures.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<Line<'static>> = failures
        .iter()
        .take(6)
        .map(|(label, _, err)| {
            Line::from(vec![
                Span::styled("  ✗ ", Style::new().fg(DANGER)),
                Span::styled(label.clone(), Style::new().fg(Color::Rgb(230, 237, 243))),
                Span::styled(
                    format!("  {}", err.clone().unwrap_or_default()),
                    Style::new().fg(DIM),
                ),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines
}

fn render_report(f: &mut Frame<'_>, app: &App) {
    let failures: Vec<&crate::app::ReapLogEntry> =
        app.reap_log.iter().filter(|(_, ok, _)| !ok).collect();
    let height = rows(9 + failures.len().min(6));
    let area = centered(70, height, f.area());
    f.render_widget(Clear, area);

    let ok_count = app.reap_log.len() - failures.len();
    let trashing = !app.trashed.is_empty();
    let headline = if app.dry_run {
        "  Would free "
    } else if trashing {
        "  Moved to Trash "
    } else {
        "  Freed "
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(headline, Style::new().fg(Color::Rgb(230, 237, 243))),
            Span::styled(
                human(app.freed),
                Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ·  {ok_count} succeeded"), Style::new().fg(DIM)),
            Span::styled(
                if failures.is_empty() {
                    String::new()
                } else {
                    format!(", {} failed", failures.len())
                },
                Style::new().fg(DANGER),
            ),
        ]),
    ];

    // The estimate is a sum of measured sizes; the disk is the authority on
    // what actually came back. Showing both keeps the tool honest, and the
    // gap is meaningful when items were trashed rather than deleted.
    if !app.dry_run
        && let Some(actual) = app.measured_freed()
    {
        lines.push(Line::from(vec![
            Span::styled("  disk free rose by ", Style::new().fg(DIM)),
            Span::styled(
                human(actual),
                Style::new()
                    .fg(if actual == 0 { CAUTION } else { SAFE })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if trashing {
                    "   (the rest is waiting in the Trash)"
                } else {
                    ""
                },
                Style::new().fg(DIM),
            ),
        ]));
    }
    lines.push(Line::from(""));

    if trashing {
        lines.extend(trash_notice(app.trashed.len()));
    }
    lines.extend(failure_lines(&failures));

    lines.push(Line::from(vec![
        Span::styled("  r ", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("rescan", Style::new().fg(DIM)),
        Span::styled(
            "   esc ",
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("back", Style::new().fg(DIM)),
        Span::styled(
            "   q ",
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("quit", Style::new().fg(DIM)),
    ]));

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(SAFE))
                .title(Line::from(Span::styled(
                    " Done ",
                    Style::new().fg(SAFE).add_modifier(Modifier::BOLD),
                ))),
        ),
        area,
    );
}

/// The quick-reap palette.
///
/// Every recipe shows what it would take before it is pressed. A recipe that
/// covers nothing right now is dimmed rather than hidden: it is still part of
/// what this tool can do, and a list that changes shape between runs is a list
/// nobody learns.
fn render_recipes(f: &mut Frame<'_>, app: &App) {
    let entries: Vec<(&crate::recipes::Recipe, usize, u64)> = app
        .recipes
        .iter()
        .map(|r| {
            let (n, bytes) = app.recipe_yield(r);
            (r, n, bytes)
        })
        .collect();

    let area = centered(74, rows(entries.len() + 7), f.area());
    f.render_widget(Clear, area);

    let mut lines = vec![Line::from("")];
    for (i, (recipe, count, bytes)) in entries.iter().enumerate() {
        let empty = *count == 0;
        let here = i == app.recipe_idx.min(entries.len().saturating_sub(1));
        let key_style = if empty {
            Style::new().fg(DIM)
        } else {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        };
        let name_style = match (empty, here) {
            (true, _) => Style::new().fg(DIM),
            (false, true) => Style::new()
                .fg(Color::Rgb(230, 237, 243))
                .add_modifier(Modifier::BOLD),
            (false, false) => Style::new().fg(Color::Rgb(200, 205, 215)),
        };
        let risk_colour = match recipe.max_risk {
            Risk::Safe => SAFE,
            Risk::Caution => CAUTION,
            Risk::Danger => DANGER,
        };

        lines.push(Line::from(vec![
            Span::styled(if here { " ▸ " } else { "   " }, Style::new().fg(ACCENT)),
            Span::styled(format!("{}  ", recipe.key), key_style),
            Span::styled(format!("{:<42}", recipe.name), name_style),
            Span::styled(format!("{count:>4}  "), Style::new().fg(DIM)),
            Span::styled(
                format!("{:>9}", human(*bytes)),
                if empty {
                    Style::new().fg(DIM)
                } else {
                    Style::new().fg(risk_colour).add_modifier(Modifier::BOLD)
                },
            ),
        ]));
    }

    // The highlighted recipe's own sentence, the same way the item list swaps a
    // description for the command that will actually run: the palette says what
    // a key leaves behind, not only what it takes.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "   {}",
            entries
                .get(app.recipe_idx.min(entries.len().saturating_sub(1)))
                .map_or("", |(r, _, _)| r.detail.as_str())
        ),
        Style::new().fg(SELECTED),
    )));
    lines.push(Line::from(Span::styled(
        "   a key runs it · ↑↓ move · enter run · esc back",
        Style::new().fg(DIM),
    )));

    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Line::from(Span::styled(
                    " Quick reap ",
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
                ))),
        ),
        area,
    );
}

fn render_help(f: &mut Frame<'_>, app: &App) {
    // Nearly the whole window: this is a document, and a document in a small
    // box is a document nobody reads.
    let area = centered(80, f.area().height.saturating_sub(2), f.area());
    f.render_widget(Clear, area);

    let mut lines: Vec<Line<'_>> = vec![Line::from("")];
    for section in crate::guide::GUIDE {
        lines.push(Line::from(Span::styled(
            format!("  {}", section.title),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for body in section.body {
            lines.push(Line::from(Span::styled(
                format!("  {body}"),
                Style::new().fg(Color::Rgb(200, 205, 215)),
            )));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "  Keys",
        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (key, description) in crate::guide::KEYS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("   {key:<12}"),
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                description.to_string(),
                Style::new().fg(Color::Rgb(200, 205, 215)),
            ),
        ]));
    }
    lines.push(Line::from(""));

    // Clamped here rather than on the keypress, because how far you can
    // scroll depends on a window size the key handler cannot see.
    let visible = area.height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(visible);
    let scroll = app.help_scroll.min(max_scroll);

    let position = if max_scroll == 0 {
        String::new()
    } else {
        format!(" {}% ", (scroll * 100).div_ceil(max_scroll.max(1)))
    };

    f.render_widget(
        Paragraph::new(lines).scroll((rows(scroll), 0)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Line::from(Span::styled(
                    " Guide ",
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
                )))
                .title_bottom(Line::from(Span::styled(
                    if max_scroll == 0 {
                        "  ↑↓ scroll · esc close  ".to_string()
                    } else {
                        format!("  ↑↓ scroll{position}· esc close  ")
                    },
                    Style::new().fg(DIM),
                ))),
        ),
        area,
    );
}

/// What one settings row says: the thing, what it is, and the three columns
/// that make the list scannable down its right edge.
struct SettingsLine {
    indent: usize,
    name: String,
    /// The path, pattern or value — the part that answers "which one".
    detail: String,
    /// Blank where risk does not apply to this kind of row.
    risk: Option<(String, bool)>,
    origin: Option<crate::settings::Origin>,
    /// `Some(false)` draws the off mark; `None` draws nothing.
    enabled: Option<bool>,
}

impl SettingsLine {
    fn new(name: impl Into<String>) -> Self {
        Self {
            indent: 4,
            name: name.into(),
            detail: String::new(),
            risk: None,
            origin: None,
            enabled: None,
        }
    }
}

/// A section heading: the disclosure arrow, the title, and how many rules are
/// filed under it.
fn heading_line(
    settings: &crate::settings::Settings,
    cfg: &crate::config::Config,
    section: crate::settings::Section,
) -> SettingsLine {
    SettingsLine {
        indent: 1,
        name: format!(
            "{} {} ({})",
            if settings.expanded.contains(&section) {
                "▾"
            } else {
                "▸"
            },
            section.title(),
            settings.count(cfg, section)
        ),
        detail: section.blurb().to_string(),
        ..SettingsLine::new("")
    }
}

/// A line for a rule that can be turned off and re-graded.
///
/// A cache rule and an artifact rule differ only in where their name and their
/// detail come from — they carry the same marks, so they are described once.
fn rule_line(
    cfg: &crate::config::Config,
    pattern: &str,
    declared: crate::config::RiskName,
    origin: crate::settings::Origin,
    name: String,
    detail: String,
) -> SettingsLine {
    let (risk, regraded) = crate::settings::effective_risk(cfg, pattern, declared);
    SettingsLine {
        name,
        detail,
        risk: Some((risk_name(risk), regraded)),
        origin: Some(origin),
        enabled: Some(!crate::settings::is_off(cfg, pattern)),
        indent: 4,
    }
}

fn settings_line(app: &App, row: &crate::settings::Row) -> SettingsLine {
    use crate::settings::{Origin, Row};
    let cfg = &app.config;
    let Some(settings) = app.settings.as_ref() else {
        return SettingsLine::new("");
    };
    let missing = || SettingsLine::new("—");

    match row {
        Row::Heading(section) => heading_line(settings, cfg, *section),

        Row::Root(i) => match cfg.scan.roots.get(*i) {
            Some(root) => SettingsLine {
                detail: if crate::config::expand(root).is_dir() {
                    String::new()
                } else {
                    "nothing there".into()
                },
                origin: Some(Origin::Yours),
                ..SettingsLine::new(root)
            },
            None => missing(),
        },

        Row::Setting(setting) => {
            let (value, origin) = setting.value(cfg);
            SettingsLine {
                name: format!("{:<26} {value}", setting.label()),
                detail: setting.detail().to_string(),
                origin: Some(origin),
                enabled: setting.is_switch().then(|| setting.is_on(cfg)),
                ..SettingsLine::new("")
            }
        }

        Row::Cache(origin, i) => match settings.cache_rule(cfg, *origin, *i) {
            Some(rule) => rule_line(
                cfg,
                &crate::settings::cache_off_pattern(rule),
                rule.risk,
                *origin,
                rule.label.clone(),
                if rule.prune.is_empty() {
                    rule.path.clone()
                } else {
                    format!("{}  $ {}", rule.path, rule.prune.join(" "))
                },
            ),
            None => missing(),
        },

        Row::Artifact(origin, i) => match settings.artifact_rule(cfg, *origin, *i) {
            Some(rule) => rule_line(
                cfg,
                &crate::settings::artifact_off_pattern(rule),
                rule.risk,
                *origin,
                rule.dir.clone(),
                if rule.evidence.is_empty() {
                    "any directory with this name".into()
                } else {
                    format!("beside {}", rule.evidence.join(", "))
                },
            ),
            None => missing(),
        },

        Row::Ignore(i) => match cfg.ignore.get(*i) {
            Some(pattern) => SettingsLine {
                origin: Some(Origin::Yours),
                ..SettingsLine::new(pattern)
            },
            None => missing(),
        },

        Row::Override(i) => match cfg.overrides.get(*i) {
            Some(rule) => SettingsLine {
                detail: format!("now counts as {}", risk_name(rule.risk)),
                risk: Some((risk_name(rule.risk), true)),
                origin: Some(Origin::Yours),
                ..SettingsLine::new(rule.matches.join(", "))
            },
            None => missing(),
        },

        Row::Recipe(origin, i) => match settings.recipe_rule(cfg, *origin, *i) {
            Some(rule) => SettingsLine {
                name: format!("{}  {}", rule.key, rule.name),
                detail: rule.detail.clone(),
                risk: Some((format!("up to {}", risk_name(rule.max_risk)), false)),
                origin: Some(*origin),
                ..SettingsLine::new("")
            },
            None => missing(),
        },

        Row::Add(section) => SettingsLine {
            indent: 4,
            ..SettingsLine::new(format!("+ {}", section.adds().unwrap_or("add")))
        },
    }
}

fn risk_name(risk: crate::config::RiskName) -> String {
    Risk::from(risk).label().to_string()
}

/// Everything reap is working from, and the means to change it.
// Right-hand columns of the settings list, laid out from the edge inwards so
// they line up down the whole list whatever a row happens to be. A space either
// side of the state mark, so it does not sit against the border.
const STATE: usize = 3;
const ORIGIN: usize = 9;
const RISK: usize = 14;
const GAP: usize = 2;

/// One row of the settings list.
///
/// Split out of the loop so the column arithmetic and the styling can be read
/// without the scrolling and the framing around them.
fn settings_row(app: &App, row: &crate::settings::Row, here: bool, inner: usize) -> Line<'static> {
    use crate::settings::{Origin, Row};

    let line = settings_line(app, row);
    let heading = matches!(row, Row::Heading(_));
    let off = line.enabled == Some(false);

    let name_style = match (heading, here, off) {
        (_, _, true) => Style::new().fg(DIM),
        (true, _, _) => Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        (false, true, _) => Style::new()
            .fg(Color::Rgb(230, 237, 243))
            .add_modifier(Modifier::BOLD),
        (false, false, _) => Style::new().fg(Color::Rgb(200, 205, 215)),
    };

    // Everything the cursor marker, the indent and the right-hand columns do
    // not need. Worked out per row because a heading is indented less than the
    // rules under it, and the columns still have to line up.
    let body = inner.saturating_sub(1 + line.indent + GAP + RISK + ORIGIN + STATE);
    // The name gets whatever the detail does not need, so a long path is
    // shortened before a label is.
    let name_width = (body / 2)
        .max(body.saturating_sub(line.detail.chars().count() + GAP))
        .min(body.saturating_sub(GAP));
    let detail_width = body.saturating_sub(name_width + GAP);

    let mut spans = vec![
        Span::styled(
            format!(
                "{}{}",
                if here { "▸" } else { " " },
                " ".repeat(line.indent)
            ),
            Style::new().fg(ACCENT),
        ),
        Span::styled(
            format!("{:<name_width$}", truncate(&line.name, name_width)),
            name_style,
        ),
        Span::styled(
            format!(
                "{:GAP$}{:<detail_width$}",
                "",
                truncate(&line.detail, detail_width)
            ),
            Style::new().fg(DIM),
        ),
        Span::raw(" ".repeat(GAP)),
    ];

    spans.push(match &line.risk {
        Some((name, regraded)) => Span::styled(
            format!(
                "{:<RISK$}",
                format!("{name}{}", if *regraded { " ✎" } else { "" })
            ),
            Style::new().fg(if off {
                DIM
            } else {
                risk_color(risk_from_name(name))
            }),
        ),
        None => Span::raw(" ".repeat(RISK)),
    });
    spans.push(match line.origin {
        Some(origin) => Span::styled(
            format!("{:>ORIGIN$}", origin.label()),
            Style::new().fg(if origin == Origin::Yours {
                SELECTED
            } else {
                DIM
            }),
        ),
        None => Span::raw(" ".repeat(ORIGIN)),
    });
    spans.push(match line.enabled {
        Some(true) => Span::styled(" ✓ ", Style::new().fg(SAFE)),
        Some(false) => Span::styled(" ✗ ", Style::new().fg(DIM)),
        None => Span::raw(" ".repeat(STATE)),
    });

    Line::from(spans)
}

fn render_settings(f: &mut Frame<'_>, app: &App) {
    let Some(settings) = app.settings.as_ref() else {
        return;
    };

    let area = centered(112, f.area().height, f.area());
    f.render_widget(Clear, area);

    let inner = area.width.saturating_sub(2) as usize;
    // Inside the border there are `height - 2` lines, and three of them are
    // chrome: a leading blank, a blank above the footer, and the footer. One
    // line over and the footer is the line that gets clipped, which is the one
    // saying how to leave.
    let visible = (area.height.saturating_sub(5)) as usize;

    // Keep the cursor near the middle rather than at an edge, so moving through
    // a section shows what is coming as well as what has passed.
    let scroll = if settings.rows.len() <= visible {
        0
    } else {
        settings
            .cursor
            .saturating_sub(visible / 2)
            .min(settings.rows.len() - visible)
    };

    let mut lines: Vec<Line<'_>> = vec![Line::from("")];
    for (i, row) in settings.rows.iter().enumerate().skip(scroll).take(visible) {
        lines.push(settings_row(app, row, i == settings.cursor, inner));
    }

    lines.push(Line::from(""));
    lines.push(settings_footer(settings, inner));

    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Line::from(Span::styled(
                    " Configuration ",
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
                )))
                .title_bottom(Line::from(Span::styled(
                    format!(" {} ", crate::util::tilde(&app.config_path)),
                    Style::new().fg(DIM),
                ))),
        ),
        area,
    );
}

/// The prompt while typing, and the applicable keys the rest of the time.
///
/// Which keys those are depends on the row: offering `d` against a built-in
/// would be offering something that cannot happen, and a footer that lies about
/// what is possible is worse than one that says less.
fn settings_footer(settings: &crate::settings::Settings, width: usize) -> Line<'static> {
    use crate::settings::{Origin, Row};

    if let Some(edit) = &settings.edit {
        return Line::from(vec![
            Span::styled(format!("  {} ", edit.prompt), Style::new().fg(ACCENT)),
            Span::styled("› ", Style::new().fg(DIM)),
            Span::styled(
                edit.buffer.clone(),
                Style::new()
                    .fg(Color::Rgb(230, 237, 243))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("▏", Style::new().fg(ACCENT)),
            Span::styled("   enter save · esc cancel", Style::new().fg(DIM)),
        ]);
    }

    let mut keys: Vec<&str> = Vec::new();
    match settings.current() {
        Some(Row::Heading(_)) => keys.push("enter open"),
        Some(Row::Add(_)) => keys.push("enter add"),
        Some(Row::Setting(setting)) => keys.push(if setting.is_switch() {
            "space toggle"
        } else {
            "e change"
        }),
        Some(Row::Cache(origin, _)) => {
            keys.extend(["x on/off", "g re-grade"]);
            if *origin == Origin::Yours {
                keys.extend(["e path", "n rename", "d delete"]);
            }
        }
        Some(Row::Artifact(origin, _)) => {
            keys.extend(["x on/off", "g re-grade"]);
            if *origin == Origin::Yours {
                keys.extend(["e edit", "d delete"]);
            }
        }
        Some(Row::Root(_) | Row::Ignore(_)) => keys.extend(["e edit", "d delete"]),
        Some(Row::Override(_)) => keys.push("d remove"),
        Some(Row::Recipe(Origin::Yours, _)) => keys.push("d delete"),
        Some(Row::Recipe(Origin::Builtin, _)) | None => {}
    }
    keys.extend(["a add", "L legend", "esc back"]);

    let hint = format!("  {}", keys.join(" · "));
    // The status has whatever the keys leave, since the keys are the part
    // someone is looking for when they do not already know what happened.
    let room = width.saturating_sub(hint.chars().count() + 4);
    Line::from(vec![
        Span::styled(hint, Style::new().fg(DIM)),
        Span::styled(
            format!("   {}", truncate(&settings.status, room)),
            Style::new().fg(SELECTED),
        ),
    ])
}

/// Map a printed risk name back to the level, for colouring.
fn risk_from_name(name: &str) -> Risk {
    match name {
        n if n.contains("safe") => Risk::Safe,
        n if n.contains("irreversible") => Risk::Danger,
        _ => Risk::Caution,
    }
}

const fn tone_color(tone: crate::guide::Tone) -> Color {
    use crate::guide::Tone;
    match tone {
        Tone::Safe => SAFE,
        Tone::Caution => CAUTION,
        Tone::Danger => DANGER,
        Tone::Accent => ACCENT,
        Tone::Dim => DIM,
    }
}

/// The marks, and what they mean. Small on purpose: it is opened mid-list to
/// settle one question, and a full-screen document would lose your place.
fn render_legend(f: &mut Frame<'_>, _app: &App) {
    // A title and a trailing blank per group, then the leading blank, the
    // closing line, and the two rows of border.
    let line_count: usize = crate::guide::LEGEND
        .iter()
        .map(|g| g.entries.len() + 2)
        .sum();
    let area = centered(66, rows(line_count + 4), f.area());
    f.render_widget(Clear, area);

    let mut lines: Vec<Line<'_>> = vec![Line::from("")];
    for group in crate::guide::LEGEND {
        lines.push(Line::from(Span::styled(
            format!("  {}", group.title),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        for (symbol, meaning, tone) in group.entries {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {symbol:<10}"),
                    Style::new()
                        .fg(tone_color(*tone))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*meaning, Style::new().fg(Color::Rgb(200, 205, 215))),
            ]));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  ? for the full guide · any key closes",
        Style::new().fg(DIM),
    )));

    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Line::from(Span::styled(
                    " Legend ",
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
                ))),
        ),
        area,
    );
}

fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

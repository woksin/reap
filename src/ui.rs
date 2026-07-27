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
        _ => {}
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
        let full = format!(
            "R quick · space pick · a all · o sort:{} · / find · d reap · ? help",
            app.sort.label()
        );
        let medium = "R quick · space pick · a all · d reap · ? help".to_string();
        if full.chars().count() <= room {
            full
        } else if medium.chars().count() <= room {
            medium
        } else {
            "? help".to_string()
        }
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

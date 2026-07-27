use crate::model::{Candidate, Category, ReapEvent, Risk, ScanEvent};
use crate::reaper;
use crate::scan::{self, ScanOpts};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Sidebar,
    Items,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Mode {
    Browsing,
    Search,
    Confirm,
    Reaping,
    Report,
    Help,
    /// The quick-reap palette: one key per standing decision.
    Recipes,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Sort {
    Size,
    Age,
    Name,
}

impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Sort::Size => "size",
            Sort::Age => "age",
            Sort::Name => "name",
        }
    }
    fn next(self) -> Self {
        match self {
            Sort::Size => Sort::Age,
            Sort::Age => Sort::Name,
            Sort::Name => Sort::Size,
        }
    }
}

/// One line in the left-hand tree.
#[derive(Clone, PartialEq, Eq)]
pub enum Node {
    /// Everything, across categories — the "biggest wins" view.
    All,
    Category(Category),
    Group(Category, String),
}

pub struct App {
    pub items: Vec<Candidate>,
    pub nodes: Vec<Node>,
    pub visible: Vec<usize>,

    pub expanded: HashSet<Category>,
    pub pending: HashSet<Category>,
    pub status: String,

    pub focus: Focus,
    pub mode: Mode,
    pub sort: Sort,
    pub node_idx: usize,
    pub item_idx: usize,
    pub search: String,
    /// Typed acknowledgement in the confirm dialog when irreversible items are selected.
    pub confirm_input: String,

    /// Only show candidates at this risk level.
    pub risk_filter: Option<Risk>,
    /// Anchor for a range selection, set by `v`.
    pub range_anchor: Option<usize>,

    pub reap_log: Vec<(String, bool, Option<String>)>,
    pub freed: u64,
    pub dry_run: bool,
    /// Move paths to the trash instead of unlinking them.
    pub trash: bool,
    /// Where this run put things, so only those can be emptied later.
    pub trashed: Vec<std::path::PathBuf>,
    /// Free space before the reap started, for measuring what actually changed.
    disk_before: Option<u64>,
    /// Free space measured after it finished.
    pub disk_after: Option<u64>,

    /// Free and total bytes on the volume we were launched from.
    pub disk: Option<(u64, u64)>,

    /// The user's configuration, and where it lives, so `x` can persist.
    pub config: crate::config::Config,
    pub config_path: std::path::PathBuf,
    /// One-key selections, built-in plus whatever the config adds.
    pub recipes: Vec<crate::recipes::Recipe>,
    /// Cursor in the palette, so the highlighted recipe can explain itself.
    pub recipe_idx: usize,

    /// A newer release, once the background check has found one.
    pub update_available: Option<String>,
    update_rx: Option<Receiver<String>>,

    pub opts: ScanOpts,
    scan_rx: Option<Receiver<ScanEvent>>,
    reap_rx: Option<Receiver<ReapEvent>>,
    pub quit: bool,
}

impl App {
    pub fn new(
        opts: ScanOpts,
        dry_run: bool,
        trash: bool,
        config: crate::config::Config,
        config_path: std::path::PathBuf,
    ) -> Self {
        let mut app = Self {
            items: Vec::new(),
            nodes: Vec::new(),
            visible: Vec::new(),
            expanded: HashSet::new(),
            pending: HashSet::new(),
            status: "starting scan".into(),
            focus: Focus::Sidebar,
            mode: Mode::Browsing,
            sort: Sort::Size,
            node_idx: 0,
            item_idx: 0,
            search: String::new(),
            confirm_input: String::new(),
            risk_filter: None,
            range_anchor: None,
            reap_log: Vec::new(),
            freed: 0,
            dry_run,
            trash,
            trashed: Vec::new(),
            disk_before: None,
            disk_after: None,
            disk: std::env::current_dir()
                .ok()
                .and_then(|d| crate::util::disk_free(&d)),
            recipes: crate::recipes::compile(&config),
            recipe_idx: 0,
            update_available: None,
            // Off the main thread and answered at most once a day, so a slow
            // or unreachable network delays nothing and says nothing.
            update_rx: {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    if let Some(latest) = crate::update::check(env!("CARGO_PKG_VERSION")) {
                        let _ = tx.send(latest);
                    }
                });
                Some(rx)
            },
            config,
            config_path,
            opts,
            scan_rx: None,
            reap_rx: None,
            quit: false,
        };
        app.rescan();
        app
    }

    pub fn rescan(&mut self) {
        self.items.clear();
        self.reap_log.clear();
        self.freed = 0;
        self.pending = Category::ALL.into_iter().collect();
        self.status = "scanning".into();
        let (tx, rx) = channel();
        self.scan_rx = Some(rx);
        scan::spawn_all(self.opts.clone(), tx);
        self.rebuild();
    }

    pub fn scanning(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Drain both worker channels. Returns true when something changed.
    pub fn poll(&mut self) -> bool {
        let mut dirty = false;

        if let Some(rx) = &self.update_rx
            && let Ok(latest) = rx.try_recv()
        {
            self.update_available = Some(latest);
            self.update_rx = None;
            dirty = true;
        }

        if let Some(rx) = &self.scan_rx {
            // Bound the drain so a fast scanner cannot starve the render loop.
            for _ in 0..512 {
                match rx.try_recv() {
                    Ok(ScanEvent::Found(c)) => {
                        self.items.push(*c);
                        dirty = true;
                    }
                    Ok(ScanEvent::Status(s)) => {
                        self.status = s;
                        dirty = true;
                    }
                    Ok(ScanEvent::Done(cat)) => {
                        self.pending.remove(&cat);
                        dirty = true;
                    }
                    Err(_) => break,
                }
            }
            if !self.scanning() {
                self.status = "scan complete".into();
            }
        }

        if let Some(rx) = &self.reap_rx {
            loop {
                match rx.try_recv() {
                    Ok(ReapEvent::Progress(r)) => {
                        self.freed += r.freed;
                        if let Some(dest) = r.trashed.clone() {
                            self.trashed.push(dest);
                        }
                        self.reap_log.push((r.label.clone(), r.ok, r.error.clone()));
                        dirty = true;
                    }
                    Ok(ReapEvent::Finished) => {
                        self.mode = Mode::Report;
                        self.reap_rx = None;
                        // Measure what the disk actually did, rather than
                        // trusting the sum of per-item estimates.
                        self.disk_after = std::env::current_dir()
                            .ok()
                            .and_then(|d| crate::util::disk_free(&d))
                            .map(|(free, _)| free);
                        for item in self.items.iter().filter(|i| i.selected) {
                            if let crate::model::Action::Remove(p) = &item.action {
                                self.opts.cache.forget(p);
                            }
                        }
                        // Reaped items are gone; drop them from the list.
                        self.items.retain(|i| !i.selected);
                        dirty = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
        }

        if dirty {
            self.rebuild();
        }
        dirty
    }

    /// Recompute the sidebar tree and the filtered, sorted item view.
    pub fn rebuild(&mut self) {
        let selected_node = self.nodes.get(self.node_idx).cloned();

        // Sidebar: every category that has items, with its groups underneath
        // when expanded.
        let mut nodes = Vec::new();
        if !self.items.is_empty() {
            nodes.push(Node::All);
        }
        for cat in Category::ALL {
            if !self.items.iter().any(|i| i.category == cat) {
                continue;
            }
            nodes.push(Node::Category(cat));
            if self.expanded.contains(&cat) {
                let mut groups: Vec<(String, u64)> = Vec::new();
                for item in self.items.iter().filter(|i| i.category == cat) {
                    match groups.iter_mut().find(|(g, _)| g == &item.group) {
                        Some((_, size)) => *size += item.size,
                        None => groups.push((item.group.clone(), item.size)),
                    }
                }
                groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                nodes.extend(groups.into_iter().map(|(g, _)| Node::Group(cat, g)));
            }
        }
        self.nodes = nodes;

        // Keep the cursor on the same node across rebuilds where possible.
        if let Some(prev) = selected_node
            && let Some(idx) = self.nodes.iter().position(|n| *n == prev)
        {
            self.node_idx = idx;
        }
        self.node_idx = self.node_idx.min(self.nodes.len().saturating_sub(1));

        let needle = self.search.to_lowercase();
        let node = self.nodes.get(self.node_idx).cloned();
        let mut visible: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| match &node {
                Some(Node::Category(c)) => item.category == *c,
                Some(Node::Group(c, g)) => item.category == *c && &item.group == g,
                Some(Node::All) | None => true,
            })
            .filter(|(_, item)| self.risk_filter.is_none_or(|r| item.risk == r))
            .filter(|(_, item)| {
                needle.is_empty()
                    || item.label.to_lowercase().contains(&needle)
                    || item.detail.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();

        match self.sort {
            Sort::Size => visible.sort_by(|a, b| {
                self.items[*b]
                    .size
                    .cmp(&self.items[*a].size)
                    .then_with(|| self.items[*a].label.cmp(&self.items[*b].label))
            }),
            Sort::Age => visible.sort_by(|a, b| {
                self.items[*b]
                    .age_days
                    .unwrap_or(0)
                    .cmp(&self.items[*a].age_days.unwrap_or(0))
                    .then_with(|| self.items[*b].size.cmp(&self.items[*a].size))
            }),
            Sort::Name => visible.sort_by(|a, b| self.items[*a].label.cmp(&self.items[*b].label)),
        }

        self.visible = visible;
        self.item_idx = self.item_idx.min(self.visible.len().saturating_sub(1));
    }

    // ---- derived totals -------------------------------------------------

    pub fn total_size(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }

    /// Total bytes carried by every candidate at a given risk level.
    pub fn risk_size(&self, risk: Risk) -> u64 {
        self.items
            .iter()
            .filter(|i| i.risk == risk)
            .map(|i| i.size)
            .sum()
    }

    pub fn risk_count(&self, risk: Risk) -> usize {
        self.items.iter().filter(|i| i.risk == risk).count()
    }

    pub fn selected(&self) -> impl Iterator<Item = &Candidate> {
        self.items.iter().filter(|i| i.selected)
    }

    pub fn selected_count(&self) -> usize {
        self.selected().count()
    }

    pub fn selected_size(&self) -> u64 {
        self.selected().map(|i| i.size).sum()
    }

    pub fn has_irreversible(&self) -> bool {
        self.selected().any(|i| i.risk == Risk::Danger)
    }

    pub fn category_size(&self, cat: Category) -> u64 {
        self.items
            .iter()
            .filter(|i| i.category == cat)
            .map(|i| i.size)
            .sum()
    }

    pub fn category_count(&self, cat: Category) -> usize {
        self.items.iter().filter(|i| i.category == cat).count()
    }

    pub fn group_size(&self, cat: Category, group: &str) -> u64 {
        self.items
            .iter()
            .filter(|i| i.category == cat && i.group == group)
            .map(|i| i.size)
            .sum()
    }

    pub fn group_count(&self, cat: Category, group: &str) -> usize {
        self.items
            .iter()
            .filter(|i| i.category == cat && i.group == group)
            .count()
    }

    // ---- actions --------------------------------------------------------

    pub fn move_cursor(&mut self, delta: isize) {
        let (len, idx) = match self.focus {
            Focus::Sidebar => (self.nodes.len(), &mut self.node_idx),
            Focus::Items => (self.visible.len(), &mut self.item_idx),
        };
        if len == 0 {
            return;
        }
        let next = (*idx as isize + delta).clamp(0, len as isize - 1);
        *idx = next as usize;
        if self.focus == Focus::Sidebar {
            self.item_idx = 0;
            self.rebuild();
        }
    }

    pub fn toggle_current(&mut self) {
        if let Some(&i) = self.visible.get(self.item_idx) {
            self.items[i].selected = !self.items[i].selected;
        }
    }

    pub fn set_all_visible(&mut self, selected: bool) {
        for &i in &self.visible {
            self.items[i].selected = selected;
        }
    }

    pub fn clear_selection(&mut self) {
        for item in &mut self.items {
            item.selected = false;
        }
    }

    /// What a recipe would tick, without ticking it — so the palette can show
    /// the payoff before the key is pressed.
    pub fn recipe_yield(&self, recipe: &crate::recipes::Recipe) -> (usize, u64) {
        self.items
            .iter()
            .filter(|i| recipe.covers(i))
            .fold((0, 0), |(n, bytes), i| (n + 1, bytes + i.size))
    }

    pub fn move_recipe_cursor(&mut self, delta: isize) {
        if self.recipes.is_empty() {
            return;
        }
        let last = self.recipes.len() as isize - 1;
        self.recipe_idx = (self.recipe_idx as isize + delta).clamp(0, last) as usize;
    }

    /// Run whichever recipe the palette cursor is on.
    pub fn apply_highlighted_recipe(&mut self) {
        if let Some(key) = self.recipes.get(self.recipe_idx).map(|r| r.key) {
            self.apply_recipe(key);
        }
    }

    /// Run a recipe: replace the selection with what it covers, and go
    /// straight to the confirmation.
    ///
    /// Replacing rather than adding is the point — a recipe states the whole
    /// of what to reap, and folding it into an existing selection would put
    /// items in front of the confirm dialog that the key did not name.
    /// Selection spans every item, not just the visible ones: "everything
    /// docker can spare" means that whatever the sidebar is currently showing.
    pub fn apply_recipe(&mut self, key: char) {
        let Some(recipe) = self.recipes.iter().find(|r| r.key == key) else {
            return;
        };
        let (name, covered): (String, Vec<bool>) = (
            recipe.name.clone(),
            self.items.iter().map(|i| recipe.covers(i)).collect(),
        );

        for (item, covered) in self.items.iter_mut().zip(covered) {
            item.selected = covered;
        }

        let count = self.selected_count();
        if count == 0 {
            // Nothing to confirm, and dropping into an empty dialog would look
            // like a failure rather than an empty result.
            self.mode = Mode::Browsing;
            self.status = format!("{name}: nothing to reap");
            return;
        }
        self.status = format!("{name}: {count} items");
        self.begin_confirm();
    }

    /// Select every item in view that is not irreversible — the "obvious wins".
    pub fn select_safe(&mut self) {
        for &i in &self.visible {
            if self.items[i].risk != Risk::Danger {
                self.items[i].selected = true;
            }
        }
    }

    pub fn toggle_expand(&mut self) {
        if let Some(node) = self.nodes.get(self.node_idx) {
            let (Node::Category(cat) | Node::Group(cat, _)) = node else {
                return; // the All view has nothing to expand
            };
            let cat = *cat;
            if !self.expanded.remove(&cat) {
                self.expanded.insert(cat);
            }
            self.rebuild();
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        self.rebuild();
    }

    /// Cycle the view between all candidates and one risk level at a time.
    pub fn cycle_risk_filter(&mut self) {
        self.risk_filter = match self.risk_filter {
            None => Some(Risk::Safe),
            Some(Risk::Safe) => Some(Risk::Caution),
            Some(Risk::Caution) => Some(Risk::Danger),
            Some(Risk::Danger) => None,
        };
        self.item_idx = 0;
        self.rebuild();
    }

    /// Start a range selection, or apply the one in progress.
    ///
    /// Picking hundreds of entries one keystroke at a time is the main cost of
    /// a large scan, so `v` anchors here, movement extends, and `v` again
    /// selects everything between.
    pub fn toggle_range(&mut self) {
        match self.range_anchor.take() {
            None => self.range_anchor = Some(self.item_idx),
            Some(anchor) => {
                let (lo, hi) = if anchor <= self.item_idx {
                    (anchor, self.item_idx)
                } else {
                    (self.item_idx, anchor)
                };
                // Match the anchor's state so a second pass clears the range.
                let target = self
                    .visible
                    .get(lo)
                    .map(|&i| !self.items[i].selected)
                    .unwrap_or(true);
                for &i in self.visible.get(lo..=hi).unwrap_or(&[]) {
                    self.items[i].selected = target;
                }
            }
        }
    }

    /// Reveal the highlighted candidate's path in Finder.
    pub fn inspect_current(&mut self) {
        let Some(&i) = self.visible.get(self.item_idx) else {
            return;
        };
        match &self.items[i].action {
            crate::model::Action::Remove(path) => {
                let shown = crate::util::tilde(path);
                // Finder can select the entry itself; xdg-open only opens a
                // directory, so it gets the parent.
                let (program, args): (&str, Vec<std::ffi::OsString>) = if cfg!(target_os = "macos")
                {
                    ("open", vec!["-R".into(), path.into()])
                } else {
                    (
                        "xdg-open",
                        vec![path.parent().unwrap_or(path).as_os_str().to_owned()],
                    )
                };
                match std::process::Command::new(program).args(args).spawn() {
                    Ok(_) => self.status = format!("revealed {shown}"),
                    Err(e) => self.status = format!("could not reveal {shown}: {e}"),
                }
            }
            // Nothing on disk to point at for a command candidate.
            crate::model::Action::Run { .. } => {
                self.status = self.items[i].action.describe();
            }
        }
    }

    /// Add the highlighted candidate to the persistent ignore list.
    ///
    /// Prefers the path when there is one, so the rule survives the item being
    /// renamed; otherwise falls back to `category/group`, which ignores the
    /// whole class rather than one transient entry.
    pub fn ignore_current(&mut self) {
        let Some(&i) = self.visible.get(self.item_idx) else {
            return;
        };
        let pattern = match &self.items[i].action {
            crate::model::Action::Remove(path) => crate::util::tilde(path),
            crate::model::Action::Run { .. } => format!(
                "{}/{}",
                self.items[i].category.title().to_lowercase(),
                self.items[i].group
            ),
        };

        if !self.config.add_ignore(pattern.clone()) {
            self.status = format!("already ignored: {pattern}");
            return;
        }
        match self.config.save(&self.config_path) {
            Ok(()) => {
                self.status = format!("ignoring {pattern}");
                // Apply immediately rather than waiting for the next scan.
                let ignore = crate::config::IgnoreSet::new(&self.config.ignore);
                self.items.retain(|c| !ignore.matches_candidate(c));
                self.rebuild();
            }
            Err(e) => {
                self.config.ignore.retain(|p| p != &pattern);
                self.status = format!("could not write {}: {e}", self.config_path.display());
            }
        }
    }

    /// Permanently remove what this run put in the trash.
    pub fn empty_trash(&mut self) {
        let removed = crate::reaper::empty_trashed(&self.trashed);
        self.status = format!("emptied {removed} items from the trash");
        self.trashed.clear();
        self.disk_after = std::env::current_dir()
            .ok()
            .and_then(|d| crate::util::disk_free(&d))
            .map(|(free, _)| free);
    }

    /// What the disk actually gave back, when it could be measured.
    pub fn measured_freed(&self) -> Option<u64> {
        let before = self.disk_before?;
        let after = self.disk_after?;
        Some(after.saturating_sub(before))
    }

    pub fn begin_confirm(&mut self) {
        if self.selected_count() == 0 {
            self.status = "nothing selected".into();
            return;
        }
        self.confirm_input.clear();
        self.mode = Mode::Confirm;
    }

    /// True once the user has satisfied whatever gate the selection requires.
    pub fn confirm_satisfied(&self) -> bool {
        !self.has_irreversible() || self.confirm_input.trim() == "reap"
    }

    pub fn start_reap(&mut self) {
        let items: Vec<Candidate> = self.selected().cloned().collect();
        if items.is_empty() {
            return;
        }
        self.reap_log.clear();
        self.freed = 0;
        self.trashed.clear();
        self.disk_after = None;
        self.disk_before = std::env::current_dir()
            .ok()
            .and_then(|d| crate::util::disk_free(&d))
            .map(|(free, _)| free);

        let (tx, rx) = channel::<ReapEvent>();
        self.reap_rx = Some(rx);
        self.mode = Mode::Reaping;
        reaper::spawn(
            items,
            reaper::ReapOpts {
                dry_run: self.dry_run,
                trash: self.trash,
            },
            tx,
        );
    }

    pub fn reap_total(&self) -> usize {
        self.selected_count()
    }
}

/// Run every scanner to completion without a UI, for `--list`.
pub fn collect_headless(opts: ScanOpts) -> Vec<Candidate> {
    let (tx, rx): (Sender<ScanEvent>, Receiver<ScanEvent>) = channel();
    scan::spawn_all(opts, tx);
    let mut items = Vec::new();
    let mut pending: HashSet<Category> = Category::ALL.into_iter().collect();
    while !pending.is_empty() {
        match rx.recv() {
            Ok(ScanEvent::Found(c)) => items.push(*c),
            Ok(ScanEvent::Done(cat)) => {
                pending.remove(&cat);
            }
            Ok(ScanEvent::Status(_)) => {}
            Err(_) => break,
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.size));
    items
}

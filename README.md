# reap

An interactive terminal app for finding and pruning the stale things that eat your disk —
git branches and worktrees, build artifacts, Docker images and caches, package manager caches.

Nothing is deleted until you pick it and confirm.

```
╭──────────────────────────────────────────────────────────────────────────────╮
│ reap  312 items found                                     63.8 GB reclaimable │
╰──────────────────────────────────────────────────────────────────────────────╯
╭─ Categories ───────────────╮╭─ Docker › build cache ──────────────────────────╮
│ ▸ Git (200)          268 MB││ ◉ BuildKit cache (all reclaimable)     17.0 GB ●│
│  █─────────────────────────││   386 unused layer records · rebuilds slower one│
│ ▸ Build artifacts (86)     ││                                                 │
│  ████████──────────────────││                                                 │
│ ▾ Docker (56)       22.5 GB││                                                 │
│  ███████───────────────────││                                                 │
│     build cache (1) 17.0 GB││                                                 │
│     unused images (12)     ││                                                 │
╰────────────────────────────╯╰─────────────────────────────────────────────────╯
╭──────────────────────────────────────────────────────────────────────────────╮
│ ◉ 1 selected · frees 17.0 GB       space pick · a all · d reap · ? help       │
╰──────────────────────────────────────────────────────────────────────────────╯
```

## Install

```bash
cargo install --path .
```

## Use

```bash
reap                      # scan and open the interface
reap --list               # print findings and exit
reap --dry-run            # interface, but deletion is simulated
reap -p ~/work -p ~/oss   # scan specific directories
reap --stale-days 90      # only call things stale after 90 days
reap --min-size 100MB     # hide small fry
reap --no-docker          # skip the Docker scan
reap --trash              # move paths to the Trash instead of deleting them
reap --no-cache           # re-measure every directory instead of reusing sizes
```

With no `--path`, reap looks in the usual places under `$HOME`: `repos`, `src`,
`Developer`, `Projects`, `code`, `dev`, `work`, `git`.

## Keys

| Key | |
|---|---|
| `↑ ↓` / `j k` | move |
| `← →` / `h l` | switch pane |
| `tab` | toggle pane |
| `enter` | expand / collapse a category |
| `space` | select item |
| `a` | select everything in view |
| `s` | select all except irreversible |
| `n` | clear the selection |
| `v` | start a range, then `v` again to select up to the cursor |
| `o` | cycle sort: size, age, name |
| `f` | cycle the risk filter: all, safe, rebuildable, irreversible |
| `/` | filter by text |
| `i` | reveal the highlighted path in Finder |
| `d` | reap the selection |
| `r` | rescan |
| `esc` | clear the filter, then the selection |
| `?` | help |
| `q` | quit |

`esc` deliberately does not quit — leaving a tool that deletes files should take a
specific keystroke.

## Reading the numbers

The header carries the three figures worth knowing at a glance:

```
 reap  914 items found                                        77.8 GB reclaimable
 ● 18.5 GB safe   ● 56.8 GB rebuildable   ▲ 2.56 GB irreversible
                                              164 GB free of 494 GB → 242 GB
```

The risk split answers "how much can I get back without thinking about it" — here,
18.5 GB is reclaimable with no judgement required. The disk line projects where free
space lands if you take everything, and the confirm dialog narrows that to just what
you have selected.

The tree opens on **Everything**, a single cross-category list sorted biggest-first,
so the largest wins are visible without picking a category. Each category then breaks
into groups carrying the reason its members are reapable, with a bar showing its share
of the total.

The highlighted row swaps its description for the exact command that will run, so
nothing is confirmed without its consequence being visible.

## What it looks for

**Git** — branches, worktrees, stashes, and repositories with enough loose objects
to be worth a `git gc`. Branches and worktrees are not merely listed: reap works out
what actually survives deleting them, and groups them by that answer.

| Group | Verdict | Risk |
|---|---|---|
| `merged branches` | reachable from the integration branch | safe |
| `squash-merged branches` | not an ancestor, but every patch is already upstream | safe |
| `pushed branches` | unmerged, but every commit is on a remote | rebuildable |
| `unpushed branches` | commits exist in this clone and nowhere else | **irreversible** |

The squash-merge case is why this matters. A squash-merged PR leaves a local branch
that `git branch --merged` reports as unmerged and whose upstream is gone — it looks
dangerous but every line of its work is already in `main`. reap settles it with
`git cherry`, which compares patch ids and so sees through the rewritten SHAs.
Conversely a branch whose upstream was deleted while it still holds local commits is
genuinely dangerous, and gets flagged as such rather than waved through as "probably
a squash merge".

Worktrees are judged the same way, on both axes that can lose work: uncommitted files
(`git status --porcelain`) and commits no remote can reach. A worktree is only
described as safe to prune when both are zero.

**Build artifacts** — `node_modules`, `target`, `bin`/`obj`, `dist`, `.next`, `.venv`,
`.gradle`, `Pods`, `__pycache__` and about twenty more. Each is only reported when a
sibling file proves what it is: a `target` next to a `Cargo.toml`, `bin` next to a
`.csproj`. A directory that merely happens to be called `build` is left alone.

**Docker** — images with no container, dangling images, stopped containers, unused
and anonymous volumes, reclaimable BuildKit cache, and dangling networks. Images are
sized by `UniqueSize`, the space that actually comes back when you remove them, rather
than the total that mostly consists of layers shared with images you are keeping.

**Caches** — npm, pnpm, yarn, bun, NuGet, Maven, Gradle, cargo, Go, pip, uv, Homebrew,
Xcode DerivedData and device support, Playwright and Puppeteer browsers, plus anything
over 200 MB in `~/Library/Caches` that isn't already named above.

## Safety

Every candidate carries a risk level, shown as a coloured dot and used by the
confirm dialog:

- **● safe** — regenerated automatically, nothing is lost
- **● rebuildable** — costs time to rebuild or re-download, nothing is unrecoverable
- **▲ irreversible** — may destroy work that exists nowhere else

Selecting anything irreversible locks the confirm button until you type `reap`.
Press `s` to select everything *except* irreversible items.

### `--trash`

With `--trash`, path removals are renamed into the volume's trash rather than
unlinked, which makes them recoverable from Finder. macOS keeps a separate trash
per volume — `~/.Trash` for the boot volume, `<mount>/.Trashes/<uid>` for the rest
— and a rename cannot cross filesystems, so reap picks the directory by device id.
Two APFS volumes sharing one container are still separate filesystems here.

The catch is that trashing frees nothing: the bytes sit in the trash until it is
emptied. reap says so rather than claiming a win it did not deliver, and the report
offers `e` to permanently delete **only the entries this run created**, leaving
anything you trashed yourself alone.

If a path cannot be trashed, reap reports the failure rather than falling back to an
unrecoverable delete.

### Estimated vs actual

The per-item figures are measured directory sizes, so the total is an estimate of
what should come back. The report also states what free space actually did, read
from the filesystem before and after. The two differ when items were trashed rather
than deleted, when something failed, or when sizes had drifted since the scan.

Beyond that:

- A recursive delete refuses any path fewer than three components deep, `$HOME`
  itself, and the system directories, whatever the scanners produce.
- Linked worktrees share one object store with their main worktree, so each set is
  collapsed to a single repository. Otherwise every branch, stash and `gc` gets
  reported once per checkout.
- `git gc` runs *without* `--prune=now`. reap also offers to delete branches, and
  pruning immediately would throw away the reflog that makes those recoverable.
- Stashes are dropped highest-index-first, because dropping `stash@{0}` renumbers
  everything below it.
- Overlapping selections are removed shallowest-first, and anything already taken by
  a parent is skipped rather than counted twice.
- The pnpm store is hard-linked into every `node_modules` on the machine, so it is
  handed to `pnpm store prune` instead of being deleted.
- A locked git worktree is never offered.

## Performance

Directory sizing is parallel at every level, and deletion runs the same way: the
overlap analysis already identifies which selected paths are pairwise disjoint, and
those are unlinked concurrently. Commands stay serial, because their order matters
and they touch shared state — a repository's ref store, the Docker daemon.

Measured sizes are cached in `~/.cache/reap/sizes.json` and reused while the
directory's own mtime is unchanged and the reading is under a week old. That mtime
moves when direct children are added or removed, but *not* when a file deep inside is
rewritten, so a cached figure can lag reality — hence the time limit, and `--no-cache`
to force a fresh measurement.

## Sizes

Sizes are SI — 1 GB is 1000³ bytes — matching macOS and `docker system df`, so the
figures can be compared against those directly. Note this differs from `du -h`, which
is 1024-based and will read about 7% smaller for the same bytes.

Directory sizes are the sum of file lengths, not allocated blocks.

The disk figure comes from the volume reap was launched in, and the projection adds
the whole reclaimable total to it. That is right when everything reap found lives in
one free-space pool — including several APFS volumes in a shared container, which
report a common figure. It overstates the gain if your scan roots sit on a genuinely
separate disk, since those bytes come back somewhere else.

## Development

```bash
cargo test
cargo test preview -- --ignored --nocapture   # print a rendered frame
```

The UI is tested through ratatui's `TestBackend` against the real cell buffer,
including tiny terminals. The deletion paths are covered directly: refusing broad
paths, dry-run leaving the disk alone, and overlapping selections counting their
bytes once.

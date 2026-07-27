<div align="center">

# reap

**Find and prune the stale things eating your disk — and know which ones you can actually afford to lose.**

[![Rust](https://img.shields.io/badge/rust-1.88%2B-b7410e?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![ratatui](https://img.shields.io/badge/tui-ratatui%200.30-7dd3fc)](https://ratatui.rs)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)](#platforms)
[![CI](https://github.com/woksin/reap/actions/workflows/ci.yml/badge.svg)](https://github.com/woksin/reap/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-green)](#license)

Git branches · worktrees · build artifacts · Docker · package manager caches

</div>

---

```
╭──────────────────────────────────────────────────────────────────────────────────────────────╮
│ reap  298 items found                                                    54.7 GB reclaimable │
│ ● 20.3 GB safe   ● 31.4 GB rebuildable   ▲ 2.98 GB irreversi… 162 GB free of 494 GB → 217 GB │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
╭─ Categories ─────────────────────╮╭─ Everything · biggest first (298) ───────────────────────╮
│   Everything (298)       54.7 GB ││ ◉ BuildKit cache (all reclaimable)      4d    19.1 GB  ● │
│  ─────────────────────────────── ││   $ docker builder prune --all --force                   │
│ ▸ Git (200)               285 MB ││ ○ NuGet global packages                 1d    8.87 GB  ● │
│  ─────────────────────────────── ││   ~/.nuget/packages · every restored .NET package — re-… │
│ ▸ Build artifacts (20)   12.0 GB ││ ○ JetBrains                             4d    3.27 GB  ● │
│  ███████──────────────────────── ││   ~/Library/Caches/JetBrains · rebuilt by the owning app │
│ ▸ Docker (70)            25.5 GB ││ ○ com.microsoft.VSCode.ShipIt           3d    1.57 GB  ● │
│  ██████████████───────────────── ││   ~/Library/Caches/com.microsoft.VSCode.ShipIt · rebuil… │
│ ▸ Caches (8)             16.8 GB ││ ○ mongo:latest                          3d    1.13 GB  ● │
│  ██████████───────────────────── ││   no containers · 1.27 GB total, 142 MB shared with oth… │
│                                  ││ ○ Core/bin                              5w    1.13 GB  ● │
│                                  ││   ~/src/repos/cratis/Studio/Source/Core · untouched 5w … │
╰──────────────────────────────────╯╰──────────────────────────────────────────────────────────╯
╭──────────────────────────────────────────────────────────────────────────────────────────────╮
│ ◉ 1 selected · frees 19.1 GB     space pick · a all · o sort:size · / find · d reap · ? help │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
```

## Why

Most disk cleaners tell you what is *big*. The hard question is what is **safe to
delete**, and that is where they stop and you start guessing.

reap answers it. A branch whose upstream was deleted might be a squash-merged PR
whose work is entirely in `main` — or it might be the only copy of three days of
work. Those look identical to `git branch --merged`. reap tells them apart.

## Install

```bash
cargo install --git https://github.com/woksin/reap
```

Or from a checkout:

```bash
git clone https://github.com/woksin/reap && cd reap
cargo install --path .
```

Needs Rust 1.88+. `git` and `docker` are used if present and skipped if not.

## Use

```bash
reap                      # scan and open the interface
reap --list               # print findings and exit
reap --dry-run            # interface, but deletion is simulated
reap --trash              # move paths to the Trash instead of deleting them
reap -p ~/work -p ~/oss   # scan specific directories
reap --stale-days 90      # only call things stale after 90 days
reap --min-size 100MB     # hide the small fry
reap --no-docker          # skip the Docker scan
reap --no-cache           # re-measure everything instead of reusing sizes
reap --ignore '*/vendor'  # skip anything matching, without editing the config
reap --write-config       # write a documented starter config
```

With no `--path`, reap looks in the usual places under `$HOME`: `repos`, `src`,
`Developer`, `Projects`, `code`, `dev`, `work`, `git`.

## The interesting part: git prunability

Branches and worktrees are not merely listed. reap works out **what actually
survives deleting them** and groups them by the answer.

```
╭─ Categories ─────────────────────╮╭─ Git › unpushed branches ────────────────────────────────╮
│   Everything (939)       80.6 GB ││ ○ Arc/blah                             4mo          —  ▲ │
│  ─────────────────────────────── ││   $ git branch -D blah                                   │
│ ▾ Git (200)               285 MB ││ ○ Arc/feat/chronicle-setup-diagnos…     3w          —  ▲ │
│  ─────────────────────────────── ││   1 commits exist only here — upstream origin/feat/chro… │
│     stale worktrees (1)   170 MB ││ ○ Arc/feature/command-scenario-uni…    3mo          —  ▲ │
│     repacking (5)         115 MB ││   1 commits exist only here — never pushed anywhere      │
│     merged branches (160)    0 B ││ ○ Arc/fix/proxy-and-readmodels         4mo          —  ▲ │
│     prunable worktrees (5)   0 B ││   4 commits exist only here — upstream origin/fix/proxy… │
│     pushed branches (15)     0 B ││ ○ Arc/fix/proxygen                     4mo          —  ▲ │
│     squash-merged branches … 0 B ││   2 commits exist only here — never pushed anywhere      │
│     unpushed branches (10)   0 B ││                                                          │
╰──────────────────────────────────╯╰──────────────────────────────────────────────────────────╯
```

| Group | Verdict | Risk |
|---|---|---|
| `merged branches` | reachable from the integration branch | 🟢 safe |
| `squash-merged branches` | not an ancestor, but every patch is already upstream | 🟢 safe |
| `pushed branches` | unmerged, but every commit is on a remote | 🟡 rebuildable |
| `unpushed branches` | commits exist in this clone and **nowhere else** | 🔴 irreversible |

> [!NOTE]
> The squash-merge case is why this matters. A squash-merged PR leaves a local branch
> that `git branch --merged` calls unmerged and whose upstream is gone — it *looks*
> dangerous while every line of its work is already in `main`. reap settles it with
> `git cherry`, which compares patch ids and so sees through the rewritten SHAs.
> Conversely, a branch whose upstream was deleted while it still holds local commits
> is genuinely dangerous, and gets flagged rather than waved through.

On a real machine this took 189 branches that all looked equally scary and reduced
them to **10** that actually needed a decision.

Worktrees are judged on both axes that lose work: uncommitted files, and commits no
remote can reach. One is only called safe to prune when both are zero.

## What else it finds

<details open>
<summary><b>Build artifacts</b> — with evidence, not guesswork</summary>

<br>

`node_modules`, `target`, `bin`/`obj`, `dist`, `.next`, `.venv`, `.gradle`, `Pods`,
`__pycache__` and ~20 more.

Each is reported only when a **sibling file proves what it is**: a `target` next to a
`Cargo.toml`, a `bin` next to a `.csproj`. A directory that merely happens to be
called `build` is left alone.

</details>

<details>
<summary><b>Docker</b> — sized by what you actually get back</summary>

<br>

Images with no container, dangling images, stopped containers, unused and anonymous
volumes, reclaimable BuildKit cache, dangling networks.

Images are sized by `UniqueSize` — the space that genuinely comes back — rather than
the total, which is mostly layers shared with images you are keeping.

Docker states its sizes as display strings, so this is the one scanner whose figures
reap repeats rather than measures. A string it cannot read is reported as
**unrecognised**, not as `0 B` — the item stays on the list, below the size floor or
not, and says why it has no number. Zero is a claim about your disk; not knowing is a
claim about reap, and only one of them is true when docker changes its output.

</details>

<details>
<summary><b>Caches</b> — the usual suspects, plus whatever else is large</summary>

<br>

npm, pnpm, yarn, bun, NuGet, Maven, Gradle, cargo, Go, pip, uv, Homebrew, Xcode
DerivedData and device support, Playwright and Puppeteer browsers — plus anything over
200 MB in `~/Library/Caches` not already named.

The pnpm store is hard-linked into every `node_modules` on the machine, so it is handed
to `pnpm store prune` rather than deleted out from under them.

</details>

## Safety

Every candidate carries a risk level, shown as a coloured dot and used by the confirm
dialog:

| | | |
|---|---|---|
| 🟢 | **safe** | regenerated automatically, nothing is lost |
| 🟡 | **rebuildable** | costs time to rebuild or re-download, nothing is unrecoverable |
| 🔴 | **irreversible** | may destroy work that exists nowhere else |

Selecting anything irreversible **locks the confirm button until you type `reap`**.
Press `s` to select everything *except* those.

```
╭ Confirm ─────────────────────────────────────────────────────╮
│  Reaping 268 items · frees 49.0 GB                           │
│                                                              │
│    ● safe          177 items     18.2 GB                     │
│    ● rebuildable    39 items     28.2 GB                     │
│    ▲ irreversible   52 items     2.56 GB                     │
│                                                              │
│    free space  164 GB  →  213 GB                             │
│                                                              │
│  ▲ Some selected items cannot be recovered.                  │
│    Type reap to confirm:  ▏                                  │
│                                                              │
│  enter confirm (locked)   esc cancel                         │
╰──────────────────────────────────────────────────────────────╯
```

Beyond that:

- A recursive delete **refuses** any path fewer than three components deep, `$HOME`
  itself, and the system directories — whatever the scanners produce.
- **Linked worktrees share one object store** with their main worktree, so each set is
  collapsed to a single repository. Otherwise every branch, stash and `gc` gets
  reported once per checkout.
- `git gc` runs **without** `--prune=now`. reap also deletes branches, and pruning
  immediately would throw away the reflog that makes those recoverable.
- **Stashes are dropped highest-index-first**, because dropping `stash@{0}` renumbers
  everything below it.
- Overlapping selections are removed **shallowest-first**, and anything already taken
  by a parent is skipped rather than counted twice.
- A **locked** git worktree is never offered.
- `esc` does not quit. Leaving a tool that deletes files should take a specific
  keystroke.

### `--trash`

With `--trash`, path removals are renamed into the volume's trash instead of unlinked,
making them recoverable from Finder. macOS keeps a separate trash per volume —
`~/.Trash` for the boot volume, `<mount>/.Trashes/<uid>` for the rest — and a rename
cannot cross filesystems, so reap picks the directory by device id. Two APFS volumes
sharing one container are still separate filesystems here.

> [!WARNING]
> Trashing frees **nothing** — the bytes sit there until the trash is emptied. reap
> says so rather than claiming a win it did not deliver, and the report offers `e` to
> permanently delete *only the entries that run created*, leaving anything you trashed
> yourself alone.

If a path cannot be trashed, reap reports the failure rather than silently falling back
to an unrecoverable delete.

### Estimated vs actual

Per-item figures are measured directory sizes, so the total is an *estimate*. The report
also states what free space **actually** did, read from the filesystem before and after.
The two differ when items were trashed, when something failed, or when sizes drifted
since the scan.


## Configuration

Nothing reap knows is baked in. Which directories count as build output, which
caches are worth offering, what never to descend into — all of it comes from
`~/.config/reap/config.toml` (or `$XDG_CONFIG_HOME`), seeded with the built-in
defaults.

```bash
reap --write-config     # documented starter file
```

Command-line flags override the config, which overrides the defaults.

### Ignoring things

Patterns match against a candidate's **path**, its **label**, and its
**`category/group`**. `*` matches any run of characters, and a pattern with no
wildcard also matches everything beneath it.

```toml
ignore = [
  "~/.nuget/packages",       # one cache, always
  "*/vendor",                # any vendor directory, anywhere
  "git/unpushed branches",   # a whole group
  "docker/unused volumes",
]
```

Pressing `x` on a candidate appends the right pattern and writes the file — a
path when there is one, so the rule survives a rename, otherwise the group.

### Adding rules

```toml
[[artifact]]
dir = "my-build-output"
evidence = ["Makefile"]       # sibling files proving what it is
regen = "make"
risk = "rebuildable"          # safe | rebuildable | irreversible

[[cache]]
path = "~/.cache/my-tool"
label = "my-tool cache"
detail = "re-downloaded on next run"
risk = "safe"
prune = ["my-tool", "cache", "clean"]   # run this instead of deleting
```

`evidence` is what keeps the artifact rules honest — without it, any directory
sharing the name would match. These entries **add** to the built-ins; set
`replace_builtin_artifacts` or `replace_builtin_caches` to use only your own.

> [!NOTE]
> A malformed config is a fatal error, not a warning. Silently falling back to
> defaults would quietly change which files this tool offers to delete.

## Platforms

macOS and Linux, tested on both in CI.

Rules naming a path a machine does not have simply do not apply, so one rule set
covers both — the Xcode entries are inert on Linux, the `~/.cache/*` ones on
macOS. The pieces that genuinely differ:

| | macOS | Linux |
|---|---|---|
| Trash | `~/.Trash`, `<mount>/.Trashes/<uid>` | freedesktop `Trash/files` + `.trashinfo`, `<mount>/.Trash-<uid>` |
| Unnamed caches | `~/Library/Caches` | `$XDG_CACHE_HOME`, else `~/.cache` |
| `i` reveals via | Finder | `xdg-open` |

Windows is not supported — the tool leans on `df`, POSIX device ids and unix
trash layouts throughout.

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
| `v` | start a range, `v` again to select up to the cursor |
| `o` | cycle sort: size, age, name |
| `f` | cycle risk filter: all, safe, rebuildable, irreversible |
| `/` | filter by text |
| `i` | reveal the highlighted path in Finder |
| `x` | never offer this again — appends to your config |
| `d` | reap the selection |
| `r` | rescan |
| `esc` | clear the filter, then the selection |
| `?` | help |
| `q` | quit |

## Reading the numbers

The header carries the three figures worth knowing at a glance:

```
│ reap  914 items found                                                          77.8 GB reclaimable │
│ ● 18.5 GB safe   ● 56.8 GB rebuildable   ▲ 2.56 GB irreversible     164 GB free of 494 GB → 242 GB │
```

The **risk split** answers "how much can I get back without thinking" — 18.5 GB here,
no judgement required. The **disk line** projects where free space lands if you take
everything; the confirm dialog narrows that to your actual selection.

The tree opens on **Everything**, one cross-category list sorted biggest-first, so the
largest wins are visible without picking a category. The highlighted row swaps its
description for the **exact command that will run**, so nothing is confirmed without its
consequence visible.

## Performance

A full scan of ~900 candidates across 5 repositories, 189 branches, 633 artifact
directories and Docker: **~3.4 s**.

Everything that can be parallel is. Directory sizing fans out at every level, and
deletion works the same way — the overlap analysis already identifies which selected
paths are pairwise disjoint, and those are unlinked concurrently. Commands stay serial,
because their order matters and they touch shared state.

The scan used to take 11.8 s. Profiling said the cost was not sizing at all but the
`git` process spawned per branch, run one repository after another; evaluating
repositories concurrently took it to 3.4 s and cut system time from 29 s to 6 s.

Measured sizes are cached in `~/.cache/reap/sizes.json` and reused while the directory's
own mtime is unchanged and the reading is under a week old.

> [!NOTE]
> That mtime moves when direct children are added or removed, but **not** when a file
> deep inside is rewritten — so a cached figure can lag reality. Hence the time limit,
> and `--no-cache` to force a fresh measurement.

## Sizes

Sizes are **SI** — 1 GB is 1000³ bytes — matching macOS and `docker system df`, so
figures can be compared against those directly. This differs from `du -h`, which is
1024-based and reads about 7% smaller for the same bytes.

Directory sizes are the sum of file lengths, not allocated blocks.

The disk figure comes from the volume reap was launched in, and the projection adds the
whole reclaimable total to it. That is right when everything found lives in one
free-space pool — including several APFS volumes in a shared container, which report a
common figure. It overstates the gain if your scan roots sit on a genuinely separate
disk.

## Development

```bash
cargo test
cargo test specs::                            # behavioural specifications only
cargo test preview -- --ignored --nocapture   # print a rendered frame

# check the figures against the docker daemon on this machine
cargo test daemon_on_this_machine -- --ignored --nocapture
```

### Specifications

Behaviour is specified separately from the unit tests, following the convention
used across the Cratis and Ada codebases: `for_<subject>` names what is under
specification, `when_<scenario>` names the situation, and each
`should_<expectation>` observes exactly one thing — so a failure reads as a
sentence and names precisely what broke.

```
for_branch_prunability
  when_a_branch_was_squash_merged
    should_recognise_the_work_is_already_upstream
    should_consider_it_safe_to_delete
    should_explain_that_the_content_is_upstream
    should_force_the_delete_since_git_still_calls_it_unmerged
  when_a_branchs_upstream_was_deleted_while_it_held_local_commits
    should_treat_it_as_irreversible_rather_than_assume_a_squash_merge
    should_say_the_upstream_was_deleted_rather_than_that_it_lacks_the_commits
```

Each scenario establishes its context through `given`, performs the act once in
`BECAUSE`, and only observes thereafter. The fixtures build **real git
repositories with real remotes** and real directory trees rather than mocking
them — whether a branch is recoverable turns on what a remote can actually
reach, so a mock would only assert that the fixture agrees with itself.

Docker is the exception, since a daemon cannot be built inside a test. Its
fixture is **output captured from a real one**, sanitised of names but verbatim
in shape and in every figure's spelling — which is the part that matters, as the
sizes are parsed from display strings. The capture can only prove reap still
reads the docker that produced it, so the cross-check above asks the live daemon
instead.

Where the unit tests beside the code check mechanics — path guards, ordering,
glob matching — the specifications drive the real scanners and assert on what a
user would be shown: the group, the risk, the wording, and the exact command.

### Tests

The UI is rendered through ratatui's `TestBackend` and asserted against the real
cell buffer, including terminals far too small to draw. The deletion paths are
covered directly: refusing broad paths, dry-run leaving the disk alone, trashing
keeping contents recoverable, emptying refusing anything outside a trash, and
overlapping selections counting their bytes once.

## License

MIT

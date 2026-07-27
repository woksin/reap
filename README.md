<div align="center">

<img src="assets/banner.svg" alt="reap — know what is safe to lose" width="820">

<br>

[![Rust](https://img.shields.io/badge/rust-1.88%2B-b7410e?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![ratatui](https://img.shields.io/badge/tui-ratatui%200.30-7dd3fc)](https://ratatui.rs)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)](#platforms)
[![CI](https://github.com/woksin/reap/actions/workflows/ci.yml/badge.svg)](https://github.com/woksin/reap/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/woksin/reap?color=86efac)](https://github.com/woksin/reap/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-green)](#license)

**Find and prune the stale things eating your disk — and know which ones you can actually afford to lose.**

<br>

<img src="assets/demo.gif" alt="reap scanning a machine, sorting 695 items into safe, rebuildable and irreversible, then reaping the safe tier" width="900">

<sub>One real run on one real machine: 60.7 GB reclaimable, sorted by what it costs to lose it.<br>
`f` narrows to the safe tier, `R` `1` takes all of it — 185 MB, none of it work.</sub>

</div>

---

## Built for the age of agentic programming

Agents changed how fast a machine fills up.

A branch per idea. A worktree per agent, so three of them can build at once without
tripping over each other. A container to check it against. A `node_modules` and a
`target` inside every one of those worktrees, because each is a real checkout. Work that
used to take a week of branches now takes an afternoon — and leaves the same debris
behind, at ten times the rate.

A month later there are two hundred branches, a dozen worktrees, and 80 GB you cannot
account for. Some of it is genuinely finished. Some of it is the only copy of an
afternoon's work. **They look identical**, and that is why the disk never gets cleaned:
the cost of guessing wrong is worse than the disk being full.

reap tells them apart.

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
╰──────────────────────────────────╯╰──────────────────────────────────────────────────────────╯
╭──────────────────────────────────────────────────────────────────────────────────────────────╮
│ ◉ 1 selected · frees 19.1 GB     R quick · space pick · a all · / find · d reap · ? help     │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
```

On a real machine this took **189 branches that all looked equally scary and reduced them
to 10** that actually needed a decision.

## Install

<table>
<tr><td width="150"><b>Homebrew</b><br><sub>macOS · Linux</sub></td><td>

```bash
brew tap woksin/reap
brew trust woksin/reap   # Homebrew 6 gates third-party taps
brew install reap
```

</td></tr>
<tr><td><b>Binary</b><br><sub>no toolchain</sub></td><td>

```bash
# macOS, Apple silicon
curl -fsSL https://github.com/woksin/reap/releases/latest/download/reap-macos-arm64.tar.gz | tar xz
# Linux, x86_64
curl -fsSL https://github.com/woksin/reap/releases/latest/download/reap-linux-x86_64.tar.gz | tar xz

sudo mv reap /usr/local/bin/
```

Also `reap-macos-x86_64` and `reap-linux-arm64` — swap the last part of the name. Every
release carries a `SHA256SUMS`.

</td></tr>
<tr><td><b>Windows</b><br><sub>no toolchain</sub></td><td>

```powershell
Invoke-WebRequest https://github.com/woksin/reap/releases/latest/download/reap-windows-x86_64.exe -OutFile reap.exe
```

Shipped as the executable itself — there is nothing to unpack. `reap-windows-x86.exe` is
the 32-bit build; Windows on ARM runs the x86_64 one under emulation. Put it anywhere on
your `PATH`.

</td></tr>
<tr><td><b>Cargo</b><br><sub>Rust 1.88+</sub></td><td>

```bash
cargo install --git https://github.com/woksin/reap
```

</td></tr>
</table>

> [!NOTE]
> The macOS binaries are unsigned. Fetched with `curl` they run as-is; downloaded through
> a browser, Gatekeeper quarantines them and `xattr -d com.apple.quarantine reap` clears
> it. Homebrew handles this for you.

`git` and `docker` are used if present and skipped if not.

### Learning it

```bash
reap guide
```

The same walkthrough `?` shows inside the interface: what the five categories are, what
the risk levels mean, how selection works, and what happens when you press `d`.

### Staying current

```bash
reap update
```

reap works out how it was installed from where its binary sits, and hands the job to
whoever put it there — `brew upgrade reap`, or `cargo install --force`. A binary you
placed by hand is left alone: it prints the two lines that would replace it rather than
overwriting something in `/usr/local/bin` behind a `sudo` you did not ask for.

The interface says so in the footer when a release is out. That check runs on its own
thread, gives up after five seconds and remembers the answer for a day, so it is never
the reason reap feels slow — and it stays quiet when there is no terminal to read it,
so it cannot end up in a cron log or in `--json`. `REAP_NO_UPDATE_CHECK=1` turns it off
entirely.

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
reap --no-personal        # skip downloads, installers and device backups
reap --no-cache           # re-measure everything instead of reusing sizes
reap --ignore '*/vendor'  # skip anything matching, without editing the config
reap --write-config       # write a documented starter config
```

With no `--path`, reap looks in the usual places under `$HOME`: `repos`, `src`,
`Developer`, `Projects`, `code`, `dev`, `work`, `git`, and the two the Windows tooling
picks by default — `source/repos` and `Documents/GitHub`.

Once you are in, `/` narrows the list as you type — across every category at once, so one
query reaches build output, docker images and repositories together:

<div align="center">
<img src="assets/find.gif" alt="pressing / and typing to narrow hundreds of findings down to one project's worth" width="860">
</div>

### Without the interface

```bash
reap --json                              # findings as JSON, for anything that decides for itself
reap --reap                              # print the plan and change nothing
reap --reap --yes                        # take everything safe
reap --reap --risk rebuildable --yes     # raise the ceiling
reap --reap --recipe d --yes             # whatever the `d` recipe covers
```

`--reap` defaults to `--risk safe` and does nothing at all without `--yes`. The
interface makes you look at the selection and type a word for the irreversible;
neither exists here, so the deliberate act is the flags — `--yes` to touch
anything, and a ceiling that has to be raised by hand before work can be lost.
Failures come back as a non-zero exit, so a cron job that half-worked says so.

```cron
0 4 * * 1  /usr/local/bin/reap --reap --recipe d --yes >> ~/.local/state/reap.log 2>&1
```

## `R` — one key for the decision you always make

After the first couple of runs, the ticking is the same ticking. `R` opens the recipes:

```
╭ Quick reap ────────────────────────────────────────────────────────────╮
│                                                                        │
│   1  Everything safe                            412    18.2 GB         │
│   2  Everything but the irreversible            451    46.4 GB         │
│   3  Absolutely everything                      503    49.0 GB         │
│ ▸ g  Git · branches already upstream            160     285 MB         │
│   w  Git · worktrees with nothing in them         5     170 MB         │
│   G  Git · everything it can spare              181     455 MB         │
│   b  Build artifacts                             20    12.0 GB         │
│   d  Docker · safe                               41    21.2 GB         │
│   D  Docker · everything but the volumes         66    25.5 GB         │
│   c  Caches                                       8    16.8 GB         │
│   a  Apps · what they will simply rebuild        34    9.28 GB         │
│   i  Installers you have already run              6    4.10 GB         │
│                                                                        │
│   merged and squash-merged · already in the integration branch         │
│   a key runs it · ↑↓ move · enter run · esc back                       │
╰────────────────────────────────────────────────────────────────────────╯
```

Every recipe shows what it would take **before** you press it, and the highlighted one
says what it leaves behind.

A recipe only *selects*. It drops you into the same confirm dialog as ticking by hand —
same risk split, same typed acknowledgement when anything irreversible is in there. One
key is a shortcut through the tedium, never through the safety.

And they are yours to define:

```toml
[[recipe]]
key = "n"
name = "Node · every node_modules"
detail = "pnpm install brings them all back"
match = ["build artifacts/node_modules"]
max_risk = "rebuildable"

[[recipe]]
key = "p"
name = "This project only"
match = ["~/work/big-monorepo/*"]
max_risk = "rebuildable"
```

`match` takes the same patterns as `ignore`, so a pattern learned in one place works in
the other. Reuse a built-in key and yours takes it over.

## The interesting part: git prunability

Branches and worktrees are not merely listed. reap works out **what actually survives
deleting them** and groups them by the answer.

<div align="center">
<img src="assets/git.gif" alt="expanding the Git category and stepping through stale worktrees, merged, pushed and unpushed branches" width="860">
</div>

<sub>Four groups, four different answers. The last one holds commits that exist in this
clone and nowhere else, and every entry says how many and where the upstream went.</sub>

| Group | Verdict | Risk |
|---|---|---|
| `merged branches` | reachable from the integration branch | 🟢 safe |
| `squash-merged branches` | not an ancestor, but every patch is already upstream | 🟢 safe |
| `pushed branches` | unmerged, but every commit is on a remote | 🟡 rebuildable |
| `unpushed branches` | commits exist in this clone and **nowhere else** | 🔴 irreversible |

> [!NOTE]
> The squash-merge case is why this matters, and it is the case a squash-merge workflow
> creates constantly. A squash-merged PR leaves a local branch that `git branch --merged`
> calls unmerged and whose upstream is gone — it *looks* dangerous while every line of its
> work is already in `main`. reap settles it with `git cherry`, which compares patch ids
> and so sees through the rewritten SHAs. Conversely, a branch whose upstream was deleted
> while it still holds local commits is genuinely dangerous, and gets flagged rather than
> waved through.

Worktrees are judged on both axes that lose work: uncommitted files, and commits no remote
can reach. One is only called safe to prune when both are zero.

## What else it finds

<details open>
<summary><b>Build artifacts</b> — with evidence, not guesswork</summary>

<br>

`node_modules`, `target`, `bin`/`obj`, `dist`, `.next`, `.venv`, `.gradle`, `Pods`,
`__pycache__` and ~20 more.

Each is reported only when a **sibling file proves what it is**: a `target` next to a
`Cargo.toml`, a `bin` next to a `.csproj`. A directory that merely happens to be called
`build` is left alone.

</details>

<details>
<summary><b>Docker</b> — sized by what you actually get back</summary>

<br>

Images with no container, dangling images, stopped containers, unused and anonymous
volumes, reclaimable BuildKit cache, dangling networks.

Images are sized by `UniqueSize` — the space that genuinely comes back — rather than the
total, which is mostly layers shared with images you are keeping.

Docker states its sizes as display strings, so this is the one scanner whose figures reap
repeats rather than measures. A string it cannot read is reported as **unrecognised**, not
as `0 B` — the item stays on the list and says why it has no number. Zero is a claim about
your disk; not knowing is a claim about reap, and only one of them is true when docker
changes its output.

</details>

<details>
<summary><b>Caches</b> — the usual suspects, plus whatever else is large</summary>

<br>

**Developer tools** — npm, pnpm, yarn, bun, NuGet, Maven, Gradle, cargo, Go, pip, uv,
Homebrew, Xcode DerivedData and device support, Playwright and Puppeteer browsers.

**Everything else on the machine** — Chrome, Firefox, Safari, Edge and Brave caches
(never a profile: you stay signed in); Adobe's media cache, waveform and Camera Raw
files, After Effects' disk cache, DaVinci Resolve's render cache, Blender; Spotify's
stream cache and its offline downloads; Steam shader caches and part-downloads; Windows
temporary files, crash dumps, shader caches and the Recycle Bin.

**Every Electron app at once.** Slack, Discord, Teams, VS Code, Figma, Notion, Postman —
each carries a Chromium, and each Chromium writes `Cache`, `Code Cache` and `GPUCache`
into the app's own data directory, where no platform's cache sweep ever looks. reap walks
`~/Library/Application Support`, `~/.config`, `%APPDATA%` and `%LOCALAPPDATA%` for those
names exactly, and reports them grouped by the app that owns them.

Plus anything over 200 MB in the platform's cache root that no rule already names — and
if a rule names something *inside* one of those directories, reap steps around it rather
than over it, so the same gigabytes are never offered twice under two different labels.

The pnpm store is hard-linked into every `node_modules` on the machine, so it is handed to
`pnpm store prune` rather than deleted out from under them. The Recycle Bin is emptied
through the shell rather than unlinked, because it is indexed.

</details>

<details>
<summary><b>Personal</b> — your own files, and an honest admission about them</summary>

<br>

Old downloads, installers, and phone backups.

This is the one category where reap has no proof to work from. A `target` beside a
`Cargo.toml` *is* build output; a branch whose patches are all in `main` *is* merged. But
a 4 GB file in Downloads is either an installer for something you already installed or
the only copy of a wedding video, and nothing in the filesystem tells those apart.

So reap does not guess. Anything announcing itself as an installer — `.dmg`, `.exe`,
`.pkg`, `.iso`, `.msi`, `.deb`, `.rpm` — is **rebuildable**, because the worst case is
downloading it again. **Everything else is irreversible**, and that is a mechanism rather
than a warning label: irreversible items are never taken by `s`, never by a safe recipe,
never by an unattended `--reap`, and never without the word `reap` being typed.

Device backups get the same treatment, and are named after the device rather than its
identifier — `Sara's iPhone`, not `00008030-001C4D...` — because the question you are
actually being asked is whether you still have that phone.

Only the top level of the download directory, only what is older than `stale_days`, and
only what is over `downloads_floor` (100 MB by default): every row here costs a judgement
someone has to make one at a time, so a list long enough to skim past is a list that gets
skipped whole.

`--no-personal`, or `personal = false` under `[scan]`, turns the whole category off.

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

<div align="center">
<img src="assets/gate.gif" alt="selecting absolutely everything; the confirm dialog turns red and refuses until the word reap is typed out" width="860">
</div>

<sub>`R` `3` selects absolutely everything. The dialog comes up red, `enter` reads **locked**,
and it stays that way until the word is typed out — then this one backs out with `esc`
instead. Recorded with `--dry-run`, which is why it says so.</sub>

Beyond that:

- A recursive delete **refuses** any path fewer than three components deep, `$HOME`
  itself, and the system directories — whatever the scanners produce.
- **Linked worktrees share one object store** with their main worktree, so each set is
  collapsed to a single repository. Otherwise every branch, stash and `gc` gets reported
  once per checkout.
- `git gc` runs **without** `--prune=now`. reap also deletes branches, and pruning
  immediately would throw away the reflog that makes those recoverable.
- **Stashes are dropped highest-index-first**, because dropping `stash@{0}` renumbers
  everything below it.
- Overlapping selections are removed **shallowest-first**, and anything already taken by a
  parent is skipped rather than counted twice.
- A **locked** git worktree is never offered.
- `esc` does not quit. Leaving a tool that deletes files should take a specific keystroke.

### `--trash`

With `--trash`, path removals are renamed into the volume's trash instead of unlinked,
making them recoverable from Finder. macOS keeps a separate trash per volume — `~/.Trash`
for the boot volume, `<mount>/.Trashes/<uid>` for the rest — and a rename cannot cross
filesystems, so reap picks the directory by device id. Two APFS volumes sharing one
container are still separate filesystems here.

> [!WARNING]
> Trashing frees **nothing** — the bytes sit there until the trash is emptied. reap says so
> rather than claiming a win it did not deliver, and the report offers `e` to permanently
> delete *only the entries that run created*, leaving anything you trashed yourself alone.

If a path cannot be trashed, reap reports the failure rather than silently falling back to
an unrecoverable delete.

### Estimated vs actual

Per-item figures are measured directory sizes, so the total is an *estimate*. The report
also states what free space **actually** did, read from the filesystem before and after.
The two differ when items were trashed, when something failed, or when sizes drifted since
the scan.

## Configuration

Nothing reap knows is baked in. Which directories count as build output, which caches are
worth offering, what never to descend into, which key reaps what — all of it comes from
`~/.config/reap/config.toml` (or `$XDG_CONFIG_HOME`), seeded with the built-in defaults.

```bash
reap --write-config     # documented starter file
```

Command-line flags override the config, which overrides the defaults.

### `C` — all of it, on screen

All of that was configurable and almost none of it was visible. Ninety cache rules, thirty
build rules, a dozen recipes and five thresholds decide what you are shown, and the only
way to read any of it was to open the source. `C` puts the whole lot on one screen:

```
╭─ Configuration ──────────────────────────────────────────────────────────────────────────────╮
│                                                                                              │
│  ▾ Where to look (1)              directories searched for repositories and build output     │
│     ~/work                                                                          yours    │
│     + a directory to search                                                                  │
│  ▾ Scanning (9)                   thresholds, and which scanners run at all                  │
│     stale after      90 days      days untouched before something counts as stale   yours    │
│     hide anything under  1MB      the floor under everything reap reports        built-in    │
│     scan your own files  off      downloads, installers, device backups             yours ✗  │
│  ▾ Caches (85)                    a path, what clears it, and what it costs to lose          │
│     my own cache                     ~/.cache/mine    rebuildable                   yours ✓  │
│     npm cache                    ~/.npm/_cacache      safe                       built-in ✓  │
│  ▸  Firefox cache        ~/Library/Caches/Firefox     safe                       built-in ✗  │
│     NuGet global packages          ~/.nuget/packages  safe ✎                     built-in ✓  │
│  ▸ Never offer (2)     ▸ Re-graded (1)     ▸ Quick reaps (12)                                │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
  x on/off · g re-grade · a add · L legend · esc back    every change is written as you make it
```

Every row says where it came from and whether it is on. `e` changes a path, pattern or
value; `n` renames a rule of yours; `a` adds one; `x` turns a rule off **and turns it back
on**; `g` re-grades what something costs you; `d` deletes something you added.

Changes are written as you make them, in the same shapes a hand-written config uses — an
`ignore`, an `[[override]]`, a `[[cache]]`. Nothing learned here stops being true at the
command line, and a file you wrote by hand is edited in place rather than replaced.

> [!NOTE]
> A built-in rule can be turned off and re-graded but never edited or deleted. Editing one
> in place would turn your config from an adjustment to reap's defaults into a replacement
> for them — and the next release correcting where a vendor hides its cache would silently
> stop reaching you. `x` and `g` cover the same ground without that cost.

This is also what makes `x` reversible. It used to write a line to a file nobody was
looking at; now there is a screen to take it back on.

### Ignoring things

Patterns match against a candidate's **path**, its **label**, and its
**`category/group`**. `*` matches any run of characters, and a pattern with no wildcard
also matches everything beneath it.

```toml
ignore = [
  "~/.nuget/packages",       # one cache, always
  "*/vendor",                # any vendor directory, anywhere
  "git/unpushed branches",   # a whole group
  "docker/unused volumes",
]
```

Pressing `x` on a candidate appends the right pattern and writes the file — a path when
there is one, so the rule survives a rename, otherwise the group.

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

`evidence` is what keeps the artifact rules honest — without it, any directory sharing the
name would match. These entries **add** to the built-ins; set `replace_builtin_artifacts`,
`replace_builtin_caches` or `replace_builtin_recipes` to use only your own.

### Disagreeing about what something costs

The built-in risk levels are one person's judgement. A cache you re-download over a fast
link is safe to you; a stopped container you are keeping to debug is not. Risk is what `s`
and the recipes select by, so correcting it is what makes those keys fit rather than nearly
fit.

```toml
[[override]]
match = ["caches/*"]           # same patterns as `ignore`
risk = "safe"

[[override]]
match = ["~/.cache/precious"]  # the last matching rule wins, so exceptions go below
risk = "irreversible"
```

Ignoring beats re-grading: something you said never to offer stays unoffered.

> [!NOTE]
> A malformed config is a fatal error, not a warning. Silently falling back to defaults
> would quietly change which files this tool offers to delete.

## Platforms

macOS, Linux and Windows, tested on all three in CI.

Rules naming a path a machine does not have simply do not apply, so one rule set covers
all of them — the Xcode entries are inert on Linux, the `~/.cache/*` ones on macOS, the
`%LOCALAPPDATA%` ones anywhere that is not Windows. A cache rule's `path` takes `~` for
your home directory and `%VARIABLE%` for an environment variable, which is the whole of
the branching. The pieces that genuinely differ:

| | macOS | Linux | Windows |
|---|---|---|---|
| Trash | `~/.Trash`, `<mount>/.Trashes/<uid>` | freedesktop `Trash/files` + `.trashinfo` | the shell's Recycle Bin |
| Unnamed caches | `~/Library/Caches` | `$XDG_CACHE_HOME`, else `~/.cache` | — |
| App data caches | `~/Library/Application Support` | `~/.config`, `~/.local/share` | `%APPDATA%`, `%LOCALAPPDATA%` |
| Free space via | `df` | `df` | `GetDiskFreeSpaceExW` |
| `i` reveals via | Finder | `xdg-open` | Explorer |

The guards that refuse to recursively delete a system directory are written per platform
rather than shared, because they are the one place a wrong answer is unrecoverable:
`/usr`, `/System` and friends on unix; `C:\Windows`, `Program Files`, `ProgramData` and
`$Recycle.Bin` on Windows, along with a depth floor that accounts for the drive letter
being a component of its own.

> [!NOTE]
> On Windows, `--trash` hands each path to the shell, so what lands in the Recycle Bin is
> restorable from it in the ordinary way. The shell does not report back where it put
> anything, so reap cannot offer to empty afterwards what it just put there — the `e` key
> on the report is macOS and Linux only.

## Keys

| Key | |
|---|---|
| `R` | **quick reap** — one key per standing decision |
| `C` | **configuration** — every rule reap is working from, and the means to change it |
| `L` | **legend** — what the marks mean, over whatever you are looking at |
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

Selecting a whole group is `a` with that group highlighted in the sidebar — the item list
is already narrowed to it.

## Reading the numbers

The header carries the three figures worth knowing at a glance:

```
│ reap  914 items found                                                          77.8 GB reclaimable │
│ ● 18.5 GB safe   ● 56.8 GB rebuildable   ▲ 2.56 GB irreversible     164 GB free of 494 GB → 242 GB │
```

The **risk split** answers "how much can I get back without thinking" — 18.5 GB here, no
judgement required. The **disk line** projects where free space lands if you take
everything; the confirm dialog narrows that to your actual selection.

The tree opens on **Everything**, one cross-category list sorted biggest-first, so the
largest wins are visible without picking a category. The highlighted row swaps its
description for the **exact command that will run**, so nothing is confirmed without its
consequence visible.

### Sizes

Sizes are **SI** — 1 GB is 1000³ bytes — matching macOS and `docker system df`, so figures
can be compared against those directly. This differs from `du -h`, which is 1024-based and
reads about 7% smaller for the same bytes.

Directory sizes are the sum of file lengths, not allocated blocks.

The disk figure comes from the volume reap was launched in, and the projection adds the
whole reclaimable total to it. That is right when everything found lives in one free-space
pool — including several APFS volumes in a shared container, which report a common figure.
It overstates the gain if your scan roots sit on a genuinely separate disk.

## Performance

A full scan of ~900 candidates across 5 repositories, 189 branches, 633 artifact
directories and Docker: **~3.4 s**.

Everything that can be parallel is. Directory sizing fans out at every level, and deletion
works the same way — the overlap analysis already identifies which selected paths are
pairwise disjoint, and those are unlinked concurrently. Commands stay serial, because
their order matters and they touch shared state.

The scan used to take 11.8 s. Profiling said the cost was not sizing at all but the `git`
process spawned per branch, run one repository after another; evaluating repositories
concurrently took it to 3.4 s and cut system time from 29 s to 6 s.

Measured sizes are cached in `~/.cache/reap/sizes.json` and reused while the directory's
own mtime is unchanged and the reading is under a week old.

> [!NOTE]
> That mtime moves when direct children are added or removed, but **not** when a file deep
> inside is rewritten — so a cached figure can lag reality. Hence the time limit, and
> `--no-cache` to force a fresh measurement.

## Development

```bash
cargo test
cargo test specs::                            # behavioural specifications only
cargo test preview -- --ignored --nocapture   # print a rendered frame

# check the figures against the docker daemon on this machine
cargo test daemon_on_this_machine -- --ignored --nocapture
```

The GIFs above are scripted, not screen-captured — every one of them re-renders from a
`.tape` file with [vhs](https://github.com/charmbracelet/vhs):

```bash
cargo build --release && vhs assets/demo.tape
```

[`assets/RECORDING.md`](assets/RECORDING.md) covers how they were made and why each one
earns its place.

### Specifications

Behaviour is specified separately from the unit tests, following the convention used
across the Cratis and Ada codebases: `for_<subject>` names what is under specification,
`when_<scenario>` names the situation, and each `should_<expectation>` observes exactly one
thing — so a failure reads as a sentence and names precisely what broke.

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

Each scenario establishes its context through `given`, performs the act once in `BECAUSE`,
and only observes thereafter. The fixtures build **real git repositories with real
remotes** and real directory trees rather than mocking them — whether a branch is
recoverable turns on what a remote can actually reach, so a mock would only assert that the
fixture agrees with itself.

Docker is the exception, since a daemon cannot be built inside a test. Its fixture is
**output captured from a real one**, sanitised of names but verbatim in shape and in every
figure's spelling — which is the part that matters, as the sizes are parsed from display
strings. The capture can only prove reap still reads the docker that produced it, so the
cross-check above asks the live daemon instead.

### Tests

The UI is rendered through ratatui's `TestBackend` and asserted against the real cell
buffer, including terminals far too small to draw. The deletion paths are covered directly:
refusing broad paths, dry-run leaving the disk alone, trashing keeping contents
recoverable, emptying refusing anything outside a trash, and overlapping selections
counting their bytes once.

### Releasing

There is no version to bump. Label a pull request `major`, `minor` or `patch`, and merging
it cuts the release: [cratis/release-action](https://github.com/cratis/release-action)
works out the next semantic version, tags it, and the
[release workflow](.github/workflows/release.yml) builds and attaches binaries for all four
targets and pushes the Homebrew formula. The version is stamped into `Cargo.toml` at build
time rather than committed, so `reap --version` reports the release it came from. A merge
with none of those labels releases nothing.

See [CHANGELOG.md](CHANGELOG.md).

## License

MIT

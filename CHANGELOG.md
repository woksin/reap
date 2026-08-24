# Changelog

Notable changes, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions are not set by hand. A merged pull request labelled `major`, `minor` or
`patch` decides the next one, and the
[release workflow](.github/workflows/release.yml) cuts the release from it — so
the tag, the GitHub release and the binaries all come from that label. This file is the readable summary of what changed; the release notes
on each tag are the generated list of pull requests.

## [Unreleased]

### Fixed

- **A checkout is no longer offered as a cache.** The unnamed cache sweep
  reported anything large it did not recognise as "rebuilt by the owning app",
  at the rebuildable tier — which a quick recipe takes without a typed
  confirmation. Tools that build in a scratch directory leave git worktrees
  under a cache root, and nothing rebuilds a commit that was never pushed. The
  sweep now declines a directory that is, or just contains, a checkout and
  leaves it to the Git category, which can prove whether the work in it is
  pushed. This shipped on Linux, where `~/.cache` was already the swept root.
- **`~/.cache` is swept on macOS too.** It was treated as the Linux answer and
  `~/Library/Caches` as the macOS one, as though a machine had only whichever
  its vendor chose. A great deal of developer tooling is cross-platform first
  and writes to `~/.cache` wherever it runs — and rules naming `~/.cache/...`
  were quietly matching there all along, while the sweep that catches
  everything else never looked. Worth about 1.5 GB on the machine this was
  found on.

### Added

- **A `Coding agents` category.** The tools that write the debris this project
  was built for were the one thing on the machine reap could not see: their
  data lives in dot-directories directly under `$HOME` — `~/.claude`,
  `~/.codex`, `~/.pi`, `~/.copilot`, `~/.cursor`, `~/.gemini`,
  `~/.local/share/opencode` — which is neither the platform cache root, nor an
  application-support directory, nor anywhere the repository walk goes.

  The split that matters is inside those directories rather than between them.
  Caches, logs and finished background-job output are graded safe; package and
  plugin trees cost a re-download; **session transcripts are graded
  irreversible**, on the same reasoning `Personal` uses — nothing regenerates a
  conversation, and the machine holds no second copy of one.

  Every rule therefore names the narrowest directory that holds one kind of
  thing, which is rarely the one with the tool's name on it: `plugins/cache`
  and not `plugins/`, whose `installed_plugins.json` is the record of what you
  had installed; `npm/node_modules` and not `npm/`, whose `package.json` is the
  list of your extensions and the thing that puts them back.

  Nothing is offered until it is provably finished, and finished is measured
  from **the newest file anywhere inside a store** rather than from the store's
  own timestamp. A directory's mtime moves when an entry is added and not when
  one is written to, and a transcript is appended to for as long as a session
  lasts — so a conversation running right now, or an old one resumed this
  morning, leaves every directory above it stamped with the day it was created,
  and reading that would offer a live session as months idle. A store written
  to within the last 24 hours is not offered at all: the one threshold here
  that `--stale-days 0` cannot lower, since a gap of hours is a claim about when
  reap happened to run rather than about whether the work is done. The same rule
  covers the safe tier, so a running background job protects its own log.

  History is offered **one project or one month at a time** rather than as a
  single all-or-nothing lump. Where a tool names its store after the project's
  path with the separators flattened (`-Volumes-sourcecode-reap`), reap
  resolves that back to a real directory against the filesystem — a dash is
  legal in a directory name, so the mapping is ambiguous and is searched rather
  than assumed — which surfaces the case actually worth acting on: **sessions
  for a project that is no longer on the machine**.

  That row states the observation and not a conclusion. reap saw that nothing of
  that name is there; it did not see a deletion, and from here an unmounted
  volume is indistinguishable from a removed directory — so the row offers both
  readings and leaves the conclusion to the reader. A name that is not a path at
  all, or a search that had to be abandoned, says only that reap cannot place
  it. A layout reap does not recognise is offered whole and says so, rather than
  being silently skipped.

  `A` in the quick-reap palette takes the caches and package trees and never a
  transcript. `--no-agents`, or `agents = false` under `[scan]`, turns the whole
  category off. `[[agent]]` entries in the configuration add tools reap does not
  ship knowing about, with `replace_builtin_agents` to use only your own; `risk`
  defaults to irreversible there rather than to rebuildable, since a directory
  under an agent's own tree is history until someone says otherwise. Aider's
  per-project `.aider.tags.cache.v3` is recognised as a build artifact, since
  its name alone is proof of what it is.
- **Caches no rule reached.** Yarn Berry keeps its store at `~/.yarn/berry`
  rather than in either of the two places Yarn Classic used, so neither rule
  covered it; `~/.npm/_npx` is a second npm cache beside `_cacache`; Pulumi's
  downloaded providers and logs; rustup's part-downloads and scratch files.
  pi-lens joins the coding-agent category with its logs, project indexes,
  downloaded language servers and tool packages.

  Also the runtimes the version managers download — rustup toolchains, nvm's
  node versions — which come back from the manifest in each project that pins
  them, and VS Code's cached extension archives.
- **`~/.vscode/extensions`, graded like a personal file.** Routinely the largest
  directory on a developer machine that no other rule reaches: 6.29 GB on the
  one this was surveyed on. Every other cache is safe to offer because the
  record of what to put back is a project file reap never touches — a
  `Cargo.toml`, a `package.json`. This one's record is `extensions.json` and it
  lives *inside* the directory, so there is no narrower thing to name and
  nothing left behind to reinstall from unless Settings Sync had it, which reap
  cannot see and does not claim to.

  Shown anyway, because hiding the biggest thing on the disk is how disks stay
  full, and graded **irreversible** so no recipe and no unattended `--reap`
  below the irreversible ceiling can take it. Someone who syncs can say so with
  an `[[override]]`. A rule whose own detail line admits it holds the only
  record of something is now held to that grading by a test.
- **Windows support.** Built and tested in CI alongside macOS and Linux, and
  published as the executable itself — `reap-windows-x86_64.exe` and
  `reap-windows-x86.exe` — since a fresh Windows has nothing that unpacks a
  tarball. Windows on ARM runs the x86_64 build under emulation. Free space
  comes from `GetDiskFreeSpaceExW` rather than `df`, `--trash` hands paths to
  the shell's Recycle Bin, `i` reveals through Explorer, and the guards that
  refuse to delete a system directory are written against `C:\Windows`,
  `Program Files`, `ProgramData` and `$Recycle.Bin` rather than against unix
  paths.
- **A `Personal` category** — old downloads, installers and phone backups.
  Nothing in it regenerates itself, so nothing in it is guessed at. Installer
  extensions (`.dmg`, `.exe`, `.pkg`, `.iso`, `.msi`, `.deb`, `.rpm`) make a
  useful review group but cannot prove another copy exists, so **everything is
  graded irreversible by default** — which keeps it out of `s`, out of every
  safe recipe, out of unattended reaping below the irreversible ceiling, and
  behind typed confirmation in the interface. Device backups are
  named after the device rather than its identifier. `--no-personal` or
  `personal = false` under `[scan]` turns it off.
- **Caches for the rest of the machine.** Chrome, Firefox, Safari, Edge and
  Brave (caches only, never a profile); Adobe's media cache, waveform and
  Camera Raw files, After Effects' disk cache, DaVinci Resolve's render cache,
  Blender; Spotify's stream cache and offline downloads; Steam shader caches
  and part-downloads; Windows temporary files, crash dumps, shader caches and
  the Recycle Bin, which is emptied through the shell rather than unlinked.
- **An Electron sweep.** Slack, Discord, Teams, VS Code, Figma and everything
  else built on Chromium write `Cache`, `Code Cache` and `GPUCache` into their
  own data directory, where no platform's cache sweep looks. reap now walks
  `~/Library/Application Support`, `~/.config`, `%APPDATA%` and
  `%LOCALAPPDATA%` for those names exactly, grouped by the app that owns them.
- **A settings screen, on `C`.** Every rule reap is working from — scan roots,
  thresholds, all built-in cache rules, the build rules, your ignores, your
  re-gradings and the recipes — with where each came from and whether it is on.
  `e` changes a path, pattern or value, `n` renames one of yours, `a` adds one,
  `x` turns a rule off *and back on*, `g` re-grades what something costs, `d`
  deletes something you added. Changes are written as you make them, in the
  same shapes a hand-written config uses, so nothing learned here stops being
  true at the command line. Built-in rules can be turned off and re-graded but
  never edited or deleted, so a later release correcting a vendor's cache path
  still reaches you. This is also what makes `x` reversible: it used to write a
  line to a file nobody was looking at.
- **A legend, on `L`**, drawn over whatever screen you are on and dismissed by
  any key — so what a triangle means can be settled without losing your place
  in a list of four hundred items. `reap guide` prints it too.
- A recipe for a machine that is not primarily a build machine: `a` for the
  caches applications simply rebuild.
- `%VARIABLE%` in a cache rule's `path`, alongside `~`. A variable this machine
  does not have expands to a path that is not there, so one rule table covers
  three operating systems the same way `~/Library/Caches/...` already covered
  two.
- `downloads_floor` under `[scan]`, defaulting to 100 MB.
- `reap update`, which works out how reap was installed from where its binary
  sits and hands the job to whoever put it there — Homebrew or cargo. A binary
  placed by hand gets instructions rather than being overwritten.
- A footer notice when a newer release exists, checked on its own thread with a
  five-second ceiling and remembered for a day. Silent when there is no
  terminal to read it, so it stays out of cron logs and `--json`.
  `REAP_NO_UPDATE_CHECK=1` disables it.
- `reap guide` and a scrollable `?` in the interface, both rendered from one
  source so the explanation cannot drift between them.

### Changed

- Release assets are named for the machine rather than the Rust target triple:
  `reap-macos-arm64.tar.gz` instead of `reap-aarch64-apple-darwin.tar.gz`.
- The unnamed-cache sweep steps *around* a rule rather than over it. Previously
  a rule naming `~/Library/Caches/Google/Chrome` did not stop the sweep from
  also offering `~/Library/Caches/Google` whole, so the same bytes appeared
  twice under two labels and were counted twice in the headline figure.
- `--min-size` now refuses a value it cannot read instead of quietly carrying
  on. `--min-size garbage` used to become zero, which removed the size floor
  altogether, and an unknown unit like `50XB` used to mean 50 bytes; both now
  stop the run and say so. A `library_cache_floor` or `downloads_floor` in the
  configuration file that cannot be read falls back to its documented default
  rather than to zero.

> [!IMPORTANT]
> The built-in `i` installer recipe has been removed because a filename cannot
> prove another copy exists. Scripts using `--recipe i` now fail instead of
> deleting installer-shaped downloads. Define an explicit custom recipe and
> risk override only for files you know can be downloaded again.

### Fixed

- Branches containing merge commits no longer receive a safe squash/rebase
  verdict from `git cherry`, which omits merge commits. Git and remote-check
  failures in the safety calculation now fail closed rather than becoming a
  zero-commit safe result.
- Worktree checks include ignored files. A clean worktree is removed without
  `--force`; any uncommitted or ignored content is graded irreversible.
- `stale_days` is now applied at the common scanner exit, including artifacts,
  caches and Docker objects with measurable ages. BuildKit pruning carries the
  same `until` filter as the displayed total so recent cache is not swept up.
- Installer-looking downloads are irreversible by default; the unsafe
  installer recipe has been removed.
- Linux downloads use the XDG user directory and Windows uses the Downloads
  Known Folder, with the conventional home-directory path as a fallback.
- Repository discovery honors the configured depth, recognizes bare and nested
  or hidden repositories, and no longer stops at an arbitrary depth of five.
- Artifact matching tries every same-named rule instead of allowing one rule to
  shadow another. Maven `target` and Swift `.build` are included explicitly.
- Docker's "about a minute ago" was parsed through a redundant float comparison
  that could not be relied on; the age of a Docker item is now settled by the
  wording alone, and a nonsensical count is rejected rather than rounded.

## [1.0.0]

First release.

### Added

- **Git prunability.** Branches and worktrees are evaluated rather than listed:
  merged, squash-merged (settled with `git cherry`, which compares patch ids and
  so sees through a rewritten SHA), pushed, or holding commits that exist in no
  other clone. Each verdict carries its own risk level. Worktrees are judged on
  both axes that lose work — uncommitted files and unpushed commits — and only
  called safe when both are zero.
- **Build artifacts, with evidence.** `node_modules`, `target`, `bin`/`obj`,
  `.next`, `.venv`, `Pods` and ~20 more, each reported only when a sibling file
  proves what produced it. A directory that merely happens to be called `build`
  is left alone.
- **Docker.** Unused and dangling images, stopped containers, unused and
  anonymous volumes, reclaimable BuildKit cache, dangling networks. Images are
  sized by `UniqueSize` — the space that actually comes back — rather than by a
  total that is mostly layers shared with images you are keeping.
- **Caches.** The package managers, Xcode DerivedData, Playwright and Puppeteer
  browsers, plus anything over 200 MB in the platform cache directory that is
  not already named. The pnpm store is handed to `pnpm store prune` rather than
  deleted out from under the `node_modules` trees hard-linked into it.
- **Three risk levels**, gating deletion. Selecting anything irreversible locks
  the confirm button until you type `reap`; `s` selects everything except those.
- **Quick-reap recipes**, on `R`. One key per standing decision — everything
  safe, the branches already upstream, worktrees with nothing in them, docker
  without the volumes — each showing what it would take before it is pressed. A
  recipe only selects, and lands in the same confirm dialog as ticking by hand.
  `[[recipe]]` in the config takes the same match patterns as `ignore`, and
  reusing a built-in key overrides it.
- **Headless operation** for cron and scripts: `--reap` with a `--risk`
  ceiling or a `--recipe` key, printing the plan and changing nothing without
  `--yes`, and exiting non-zero when anything failed. `--json` prints the same
  findings as `--list` in a machine-readable form.
- **Risk overrides.** `[[override]]` re-grades what reap thinks something costs,
  matched the same way `ignore` is — because risk is what `s` and the recipes
  select by, and the built-in judgement is only one person's. Ignoring still
  beats re-grading.
- **`--trash`**, which renames paths into the volume's trash instead of
  unlinking them — picking the right trash directory by device id, since a
  rename cannot cross filesystems. It reports a failure rather than falling back
  to an unrecoverable delete, and says plainly that trashing frees nothing until
  the trash is emptied.
- **Configuration for everything reap knows.** Artifact rules, cache rules,
  ignore patterns and never-descend directories all come from
  `~/.config/reap/config.toml`; `--write-config` writes a documented starter.
  Pressing `x` on a candidate appends the right pattern and saves the file.
- **A size cache** in `~/.cache/reap/sizes.json`, reused while a directory's
  mtime is unchanged and the reading is under a week old.
- macOS and Linux, tested on both in CI.

### Notes

- Sizes are SI — 1 GB is 1000³ bytes — so they can be compared directly against
  macOS and `docker system df`. `du -h` is 1024-based and reads about 7% smaller
  for the same bytes.
- A figure docker states in a form reap cannot read is reported as
  *unrecognised*, never as `0 B`.

[Unreleased]: https://github.com/woksin/reap/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/woksin/reap/releases/tag/v1.0.0

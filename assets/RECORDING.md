# Recording the README GIFs

Everything in this directory that ends in `.tape` is a script. Nothing was performed by
hand, nothing was screen-captured, and any of it can be re-rendered from a clean checkout:

```bash
cargo build --release
vhs assets/demo.tape          # or gate, git, find
```

That is the whole point. A hand-recorded screencast is a one-off you cannot fix a typo in;
a tape is source you can edit and re-run.

## The tools

[**vhs**](https://github.com/charmbracelet/vhs) turns a declarative `.tape` file into a
GIF. It spawns a real terminal, sends real keystrokes, and records what actually happened
— so the numbers in these GIFs are the numbers this machine really had.

```bash
brew install vhs      # pulls in ffmpeg and ttyd
```

**ffmpeg** comes along for the ride and does double duty: pulling frames out for
inspection, and trimming or re-timing the finished GIF.

## The loop

The order matters more than any individual setting.

**1. Read the keybindings before writing a single line of tape.** For reap that meant
`handle_key` in `src/main.rs`. This is not optional politeness — it caught a choreography
bug that would have wasted the one real recording. Pressing `1` in the quick-reap palette
does *not* just tick items and return you to the list; `apply_recipe` ends in
`begin_confirm()`, so it drops straight into the confirm dialog. The first tape assumed
otherwise and typed `f` and `d` into a dialog's text field instead of the browser. Frames
told the truth; the tape's intent did not.

**2. Rehearse in a mode that cannot hurt anything.** `reap --dry-run` runs the entire
interface and simulates the deletion. Three full rehearsals happened before anything real
was recorded. A real reap is a **one-shot take** — once the safe tier is gone it is gone
for days, and there is no second attempt at the same footage.

**3. Look at the frames. Actually look at them.**

```bash
ffmpeg -i out.gif -vf fps=2 frames/f%04d.png
```

Then open them. Every problem in this set was found by looking, never by reasoning about
what the tape should have done — the swallowed keystrokes, an `update available 1.0.0 →
1.2.0` banner sitting in the footer of a hero shot, a sidebar tour that had wandered into
the wrong category.

To find *where* something changed without scrubbing by eye, hash the frames and print only
the ones that differ from their predecessor:

```bash
ffmpeg -v error -i out.gif -vf fps=2 seq/f%04d.png
python3 -c "
import hashlib, glob
prev = None
for f in sorted(glob.glob('seq/f*.png')):
    h = hashlib.md5(open(f,'rb').read()).hexdigest()
    if h != prev: print(f'{(int(f[-8:-4])-1)/2.0:6.1f}s  {f}')
    prev = h
"
```

That produced the exact timeline of the real run — Enter at 20.0s, reaping done by 23.0s,
static report after — which is what made a precise trim possible.

**4. Only now record the real thing**, with sleeps sized for the slow case. Excess is easy
to cut later; a report that never made it on camera is not.

## The settings, and what each is actually for

All of them live in [`_style.tape`](_style.tape), which every other tape pulls in with
`Source assets/_style.tape`. Four recordings that drift apart in font size or theme read as
four screencasts; one shared file makes them read as one set.

| Setting | Why |
|---|---|
| `Set FontSize 16` + `1080x700` | ~110 columns. GitHub scales a README image to about 880px, so anything wider stops being legible. |
| `Set Theme {…}` | Hand-matched to reap's own palette (`#0d1117` background, `#7dd3fc` accent) so the chrome and the app agree. |
| `Set WindowBar Colorful`, `BorderRadius 10`, `Margin 24`, `MarginFill` | The polish. Costs nothing, and a bare rectangle of terminal looks unfinished next to it. |
| `Set Framerate 24` | 50 is the default and roughly doubles the file for no visible gain. |
| `Set TypingSpeed` | 55ms globally. Raised mid-tape where the typing *is* the content. |

The `Hide` / `Show` block is where the environment gets cleaned up before the camera rolls:

```tape
Hide
Type "export PATH=$PWD/target/release:$PATH" Enter
Type "export REAP_NO_UPDATE_CHECK=1" Enter
Type "export XDG_CONFIG_HOME=$PWD/assets/recording" Enter
Type `export PS1='\[\033[38;2;125;211;252m\]❯\[\033[0m\] '` Enter
Type "clear" Enter
Sleep 1s
Show
```

Four separate jobs: run the local build without a visible path, silence the app's own
update banner, point reap at a recording-only config (see below), and replace a personal
shell prompt with a single neutral glyph.

**Hunt for the environment's opt-out switches.** `REAP_NO_UPDATE_CHECK=1` is the difference
between a hero GIF that looks maintained and one advertising that it is two versions
behind. Every tool has something like it — an update check, a telemetry notice, a first-run
banner.

## vhs gotchas

- `Output /abs/path.gif` **fails to parse**. Quote it, or use a relative path.
- `Home` and `End` are not vhs commands. `Up 4` gets you back to the top of a list.
- `Set` works mid-tape, so `Set TypingSpeed 110ms` can slow down one stretch.
- `Escape`, not `Esc`.
- `vhs validate file.tape` catches all of this in a second. Run it before every render.

## What earns a GIF

The most useful discipline here was editorial, not technical.

**One hero, then short single-purpose clips.** The top of the README gets the full loop —
scan, browse, select, confirm, reap — at about 30 seconds. Everything after that is 15-20
seconds and shows exactly one thing, sitting next to the prose that explains it. This is
what lazygit, delta, atuin and yazi all converge on.

**A GIF has to show something text cannot.** That is the whole test.

- ✅ `gate.gif` — a dialog turning red and `enter confirm (locked)` releasing as the word
  is typed. A screenshot cannot show a lock releasing.
- ✅ `git.gif` — four branch groups, four different verdicts, dots changing colour.
- ✅ `find.gif` — 703 findings narrowing to 37 live, across categories.
- ❌ The quick-reap palette. It is a static list of ten recipes. The ASCII block already in
  the README says the same thing, is searchable, and loads instantly. It kept its place.

**Replace an ASCII mock only when the text survives elsewhere.** Two mocks were removed
here, and in both cases the table or prose directly underneath already carried the same
content — the risk-tier table under the confirm dialog, the verdict table under the git
browser. A mock deleted with nothing to replace it costs screen-reader users and anyone
reading the raw file.

**Watch the total weight.** Four GIFs, 3.8 MB. Short clips are what keep that reasonable.

## Keeping private work out of shot

A recording of a real machine is a recording of a real machine. The first cut of these GIFs
was published before anyone counted what was legible in them, and the answer was: two
private repositories by name, a client project on Azure DevOps, and ten branch names
describing work in progress.

Nothing leaked that was dangerous — reap renders directory names, sizes and an `rm -rf`
preview, never file contents, and `util::tilde()` masks `$HOME` so the username never
appears. But **project names and branch names are disclosure**, and git history makes a
committed GIF permanent: replacing the file in a later commit does not remove the old blob.
The decision has to be made before pushing, not after.

So: **do not guess what is private, ask the host.**

```bash
gh repo view "$org/$name" --json visibility -q .visibility
```

Run that across every repository the scan can reach, and treat anything that is not
`PUBLIC` — including non-GitHub remotes and repos with no remote at all — as out of shot.
It is worth the two minutes; the guesses here were wrong in both directions — one
repository whose name read like an internal codename turned out to be public, and another
sitting in the same open-source org turned out not to be. Note that this paragraph names
neither of them, which is the same discipline as gitignoring the config.

The exclusions then live in a config that vhs points at with `XDG_CONFIG_HOME`, so the
visible command stays a clean `reap`:

```toml
# assets/recording/reap/config.toml
ignore = ["*/some-private-org/*", "PrivateRepo*", "..."]
```

**That file is gitignored, and it has to be.** An exclusion list committed to a public repo
publishes exactly the names it exists to hide — the same mistake one level down. A clean
checkout finds no config there, falls back to a normal scan, and records that machine's own
findings, which is the correct behaviour for everybody else.

Verify by grepping the findings rather than trusting the patterns:

```bash
XDG_CONFIG_HOME=$PWD/assets/recording reap --json \
  | grep -iE 'PrivateRepo|other-private-name' | wc -l    # must be 0
```

## Recording something destructive

`gate.gif` is the only one that selects work which exists nowhere else, and it was recorded
with `--dry-run` on purpose. No README asset justifies a tape typo taking somebody's
unpushed branches. The cost is a visible `DRY RUN` badge, which the caption owns rather
than hides.

Before rendering anything real, audit the tape for the keystroke that commits:

```bash
grep -n "Enter\|Escape\|Type" assets/gate.tape
```

There should be exactly one `Enter` — the one that launched the command — and the sequence
should end on `Escape`. Then verify state afterwards. For these three, `reap --json` before
and after both reported 774 items and 10 unpushed branches, which is how "nothing was
deleted" became a checked fact instead of a hope.

## Post-processing

Sleeps are sized for the slow case, so every recording ends with dead air to cut:

```bash
ffmpeg -i raw.gif -filter_complex "\
  [0:v]trim=0:27.5,setpts=PTS-STARTPTS,fps=24[v];[v]split[v1][v2];\
  [v1]palettegen=stats_mode=diff[p];\
  [v2][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle" \
  -loop 0 assets/demo.gif
```

65 seconds became 27.5. The `palettegen`/`paletteuse` pair is not optional even for a plain
trim — re-encoding a GIF without it produces visible banding on exactly the kind of flat
dark UI a TUI is made of.

When a stretch is real but boring — a long build, a progress bar grinding through hundreds
of items — split the filter into three and put the middle through
`setpts=(PTS-STARTPTS)/2.5`, concatenated with `[a][b][c]concat=n=3:v=1`. An earlier cut of
the hero needed exactly that, when the reap took fifteen seconds instead of three. Speeding
up a progress bar is a demo convention, but it is still a change to the record, so the tape
says so in a comment when it happens.

---

## Doing this in another repo

Paste the following into Claude Code, from the root of the repo you want GIFs for.

````markdown
I want a set of README GIFs for this CLI, recorded with charmbracelet/vhs, in the style of
repos like lazygit, delta and yazi: one hero recording at the top, then a few short
single-purpose clips placed next to the prose that explains them.

Work in this order and do not skip ahead:

1. **Understand the tool first.** Read the CLI's argument parsing, its subcommands, and —
   if it is interactive — its key handling, from the source. Build it. Run it. Tell me what
   you found before you write any tape: what the notable commands are, what state they
   need, and anything that would surprise a recording (prompts, spinners, update banners,
   telemetry notices, anything writing to the terminal you would not want on camera).

2. **Propose the set before recording it.** Suggest a hero recording plus 2-4 feature
   clips. For each one, tell me which README section it goes in and — this is the test —
   what it shows that text or a screenshot could not. Anything that is just a static list
   or a wall of output does not earn a GIF; say so and leave it as text. Wait for me to
   agree before rendering.

3. **Set up a shared style.** Put every `Set` directive plus the environment cleanup in one
   `_style.tape` that the other tapes pull in with `Source`. Cleanup means: put the local
   build on PATH, set a neutral single-glyph PS1, silence any update-check or telemetry
   banner via whatever env var the tool honours, and `clear` — all inside `Hide`/`Show`.
   Match the terminal theme to the app's own colours. Target roughly 1080x700 at FontSize
   16; GitHub scales README images to about 880px and anything denser stops being legible.

4. **Work out what must not appear, before recording rather than after.** A recording of my
   machine shows my machine. Enumerate whatever the tool can reach — repositories,
   hostnames, database names, ticket ids, customer names — and check each against its host
   rather than guessing from the name (`gh repo view <org>/<repo> --json visibility`).
   Treat anything not demonstrably public, including non-GitHub remotes and anything with
   no remote, as out of shot. Put the exclusions in a recording-only config the tape points
   at through an env var, **gitignore that config** — an exclusion list committed to a
   public repo publishes the names it hides — and verify by grepping the tool's own output
   for each name until the count is zero. Do this before the first real render: a committed
   GIF is permanent, since replacing the file later leaves the old blob in history.

5. **Rehearse before you commit to anything.** If a recording would delete, publish, deploy
   or otherwise change real state, first render it against a dry-run flag, a scratch
   directory, or throwaway fixtures. Extract frames with
   `ffmpeg -i out.gif -vf fps=2 f%04d.png` and actually look at them. Iterate on the tape
   until the frames are right. Never let the first render of a destructive or one-shot
   action be the real one.

6. **Be careful with anything irreversible.** Prefer dry-run for any clip that selects or
   touches destructive paths, even if it means a visible "dry run" badge — caption it honestly
   rather than hiding it. Audit the tape for the keystroke that commits before rendering.
   Capture the relevant state before and after and confirm in your summary that nothing
   changed.

7. **Record, then trim.** Size the sleeps for the slow case, then cut the excess with
   ffmpeg. If a stretch is real but boring — a long build, a progress bar — speed up just
   that segment with `trim`/`setpts`/`concat`, and always re-encode through
   `palettegen`/`paletteuse` or you will get banding. Note any speed-up in a tape comment.
   Aim for a hero around 30s, clips at 15-20s, and keep the total under about 5 MB.

8. **Wire them into the README** in their sections, each with descriptive alt text and a
   one-or-two-line caption. If a GIF makes an existing ASCII mock or screenshot redundant,
   replace it — but only when the prose or table beside it already carries the same
   information as text. Never leave a section with no textual equivalent.

Commit the `.tape` files alongside the GIFs so the whole set can be re-rendered later, and
when you are done tell me plainly: what each GIF shows, anything real that appears in them
(paths, repo names, hostnames, customer data) that I should check before pushing, and
whether they were recorded against committed code or a dirty working tree.
````

Two things worth adapting before you send it:

- **If the CLI is not interactive**, the advice about key handling turns into command
  sequencing. A plain CLI records best as a short series of commands whose *output* tells a
  story — a status, a change, then the status again proving it changed. Ask for that
  explicitly.
- **If any command hits a real service**, say so in step 1. An agent cannot tell from the
  source that `deploy` talks to production.

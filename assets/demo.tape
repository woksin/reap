# The README demo. Rendered with charmbracelet/vhs:
#
#   cargo build --release && vhs assets/demo.tape
#
# It runs the real thing against whatever the machine actually has, and the
# reap at the end is real too — `1` in the quick-reap palette is the safe
# recipe, so nothing it takes is anything you cannot get back.
#
# How long the reap itself takes depends on the machine; the sleep below is
# sized for the slow case. The committed GIF then had its tail trimmed and its
# reaping phase run at 2.5x, so a run that deletes for fifteen seconds does not
# spend fifteen seconds of a README doing it:
#
#   ffmpeg -i out.gif -filter_complex "\
#     [0:v]trim=0:21,setpts=PTS-STARTPTS[a];\
#     [0:v]trim=21:36,setpts=(PTS-STARTPTS)/2.5[b];\
#     [0:v]trim=36:40.8,setpts=PTS-STARTPTS[c];\
#     [a][b][c]concat=n=3:v=1,fps=24[v];[v]split[v1][v2];\
#     [v1]palettegen=stats_mode=diff[p];[v2][p]paletteuse=dither=bayer" \
#     -loop 0 assets/demo.gif

Output assets/demo.gif

Source assets/_style.tape

Sleep 1200ms
Type "reap"
Sleep 700ms
Enter

# Scan: the spinner, then everything it found.
Sleep 4s

# What it found, broken down by what it is.
Down
Sleep 1100ms
Down
Sleep 1100ms
Down
Sleep 1100ms
Down
Sleep 1400ms
Up 4
Sleep 1s

# Narrow to the tier that costs nothing to lose.
Type "f"
Sleep 2800ms

# The standing decisions, one key each. `1` is everything safe, and a recipe
# goes straight to the confirm dialog rather than reaping on the keypress.
Type "R"
Sleep 2600ms
Type "1"
Sleep 2800ms
Enter

# Reaping, then what it freed. Long enough that a slow machine still gets its
# report on camera; the excess comes off in the trim.
Sleep 45s
Type "q"
Sleep 1s

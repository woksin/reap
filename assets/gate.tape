# The irreversible gate, for the Safety section.
#
#   cargo build --release && vhs assets/gate.tape
#
# Recorded with --dry-run on purpose. This is the one demo that selects work
# which exists nowhere else, and no README asset is worth a tape typo taking
# somebody's unpushed branches. Nothing below presses enter on the dialog; the
# shot ends on esc, because the point is the lock, not the deletion.

Output assets/gate.gif

Source assets/_style.tape

Sleep 1200ms
Type "reap --dry-run"
Sleep 600ms
Enter

Sleep 4s

# `3` is "absolutely everything" — the only recipe that reaches the top tier.
Type "R"
Sleep 2400ms
Type "3"

# The dialog comes up red and refuses: enter is dead until the word is typed.
Sleep 3500ms

# Typed a letter at a time, because the lock releasing is the whole point.
Type "r"
Sleep 450ms
Type "e"
Sleep 450ms
Type "a"
Sleep 450ms
Type "p"
Sleep 3s

# Backed out rather than confirmed.
Escape
Sleep 2s
Type "q"
Sleep 1s

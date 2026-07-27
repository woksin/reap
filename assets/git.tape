# Git prunability, for "The interesting part".
#
#   cargo build --release && vhs assets/git.tape
#
# Read-only: it expands the Git category and walks the four verdicts. No
# selection, no confirm dialog, nothing deleted.

Output assets/git.gif

Source assets/_style.tape

Sleep 1200ms
Type "reap"
Sleep 600ms
Enter

Sleep 4s

# Into Git, and open it up.
Down
Sleep 1200ms
Enter
Sleep 2200ms

# The four groups, in the order the sidebar lists them. Each one is a different
# answer to "what survives deleting this", and the dots change colour with it.
Down
Sleep 2s
Down
Sleep 2200ms
Down
Sleep 2200ms

# Unpushed last, and held: commits that exist in this clone and nowhere else.
Down
Sleep 4s

Type "q"
Sleep 1s

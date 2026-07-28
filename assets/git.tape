# Git prunability, for "The interesting part".
#
#   cargo build --release && vhs assets/git.tape
#
# Alone among these recordings, this one films a fixture rather than the machine
# it was made on — see assets/fixture.sh for why, and for how little of it is
# actually pretend. Building it is part of the tape, so this renders from a
# clean checkout with nothing set up first.
#
# Read-only: it expands a category and walks the four verdicts. No selection, no
# confirm dialog, nothing deleted.

Output assets/git.gif

Source assets/_style.tape

Hide
Type "./assets/fixture.sh /tmp/reap-demo" Enter
Sleep 25s
# Overrides the machine config the shared style points at.
Type "export XDG_CONFIG_HOME=$PWD/assets/fixture-config" Enter
Type "clear" Enter
Sleep 1s
Show

Sleep 1200ms
Type "reap"
Sleep 600ms
Enter

Sleep 3500ms

# Into Git — the categories sit in a fixed order, not by size — and open it up.
Down
Sleep 1400ms
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

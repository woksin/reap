# Search, for the Use section.
#
#   cargo build --release && vhs assets/find.tape
#
# Read-only: `/` narrows the list as you type, across every category at once.
# No selection, no confirm dialog, nothing deleted.

Output assets/find.gif

Source assets/_style.tape

Sleep 1200ms
Type "reap"
Sleep 600ms
Enter

Sleep 4s

# Narrowing hundreds of findings to one kind of thing.
Type "/"
Sleep 700ms
Set TypingSpeed 110ms
Type "node_modules"
Sleep 3s

# It is not a per-category filter — the same query reaches docker images and
# repositories too.
Escape
Sleep 1200ms
Type "/"
Sleep 500ms
Type "chronicle"
Sleep 3500ms

Escape
Sleep 1200ms
Type "q"
Sleep 1s

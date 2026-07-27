# Shared look for every recording in this directory, so the README's GIFs read
# as one set rather than four separate screencasts. Sourced, never rendered on
# its own — it deliberately has no Output.

Set Shell "bash"
Set FontSize 16
Set Width 1080
Set Height 700
Set Padding 20
Set Framerate 24
Set TypingSpeed 55ms
Set WindowBar Colorful
Set BorderRadius 10
Set Margin 24
Set MarginFill "#010409"
Set Theme { "name": "reap", "background": "#0d1117", "foreground": "#e6edf3", "cursor": "#7dd3fc", "selection": "#264f78", "black": "#484f58", "red": "#f87171", "green": "#86efac", "yellow": "#facc15", "blue": "#7dd3fc", "magenta": "#c4b5fd", "cyan": "#7dd3fc", "white": "#e6edf3", "brightBlack": "#6e7681", "brightRed": "#f87171", "brightGreen": "#86efac", "brightYellow": "#facc15", "brightBlue": "#7dd3fc", "brightMagenta": "#c4b5fd", "brightCyan": "#7dd3fc", "brightWhite": "#ffffff" }

Hide
Type "export PATH=$PWD/target/release:$PATH" Enter
# The update banner is for people, not for a recording of one.
Type "export REAP_NO_UPDATE_CHECK=1" Enter
Type `export PS1='\[\033[38;2;125;211;252m\]❯\[\033[0m\] '` Enter
Type "clear" Enter
Sleep 1s
Show

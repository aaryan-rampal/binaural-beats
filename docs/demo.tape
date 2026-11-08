# Records docs/demo.gif with https://github.com/charmbracelet/vhs: `vhs docs/demo.tape`, then
# `gifsicle -O3 --colors 32 --lossy=40 -b docs/demo.gif` to shrink it about 4x.
# The envelope only draws while sound plays, so this plays a faint 1% tone while recording.

Output docs/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 860
Set Height 520
Set Padding 12
Set Theme "Catppuccin Mocha"
Set Framerate 15

Hide
Type "cargo build --release -q && clear && target/release/binaural --beat 15 --volume 1"
Enter
Sleep 4s
Show

Sleep 2.5s
Type "3"
Sleep 2s
Type "5"
Sleep 2s
Type "1"
Sleep 2.5s
Space
Sleep 1.5s

Hide
Type "q"
Sleep 500ms

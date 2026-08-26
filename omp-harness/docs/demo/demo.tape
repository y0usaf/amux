Output docs/demo/pi-harness.gif
Output docs/demo/pi-harness.webm

Set Shell pi-harness-demo
Set Width 1200
Set Height 720
Set FontSize 18
Set FontFamily "DejaVu Sans Mono"
Set Theme { "name": "Pi Harness Demo", "black": "#1b1d23", "red": "#e06c75", "green": "#98c379", "yellow": "#e5c07b", "blue": "#61afef", "magenta": "#c678dd", "cyan": "#56b6c2", "white": "#abb2bf", "brightBlack": "#5c6370", "brightRed": "#e06c75", "brightGreen": "#98c379", "brightYellow": "#e5c07b", "brightBlue": "#61afef", "brightMagenta": "#c678dd", "brightCyan": "#56b6c2", "brightWhite": "#ffffff", "background": "#1b1d23", "foreground": "#abb2bf", "selection": "#3e4451", "cursor": "#528bff" }
Set TypingSpeed 35ms
Set Padding 20

Type "pi-harness-demo"
Enter
Wait+Screen /Acme CLI/
Sleep 2s

Type "Please inspect this project and suggest the next change."
Enter
Wait+Screen /Focused next step/
Sleep 5s

Ctrl+Q
Sleep 1s

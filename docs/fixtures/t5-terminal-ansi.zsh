#!/bin/zsh

# Deterministic product-mode terminal fixture for native evidence.
# It intentionally exercises SGR, OSC 8, Unicode, cursor visibility, and
# bracketed-paste mode while keeping the visible screen free of host data.
printf '\033[?1049h\033[2J\033[H'
printf '\033[1;38;5;45mHerdr IDE terminal\033[0m\r\n'
printf '\033[38;5;114mANSI colors\033[0m  \033[4munderline\033[0m  \033[3mitalic\033[0m\r\n'
printf '\033]8;;https://example.invalid/herdr\033\\OSC 8 hyperlink\033]8;;\033\\\r\n'
printf 'Korean UTF-8: 한글 입력 준비\r\n'
printf 'Cursor, selection, scrollback, mouse, resize, Meta, clipboard\r\n'
printf '\033[?2004h\033[?1000h\033[?1006h'
printf '\033[38;5;244mPress Return to leave the fixture\033[0m'
IFS= read -r _
printf '\033[?1006l\033[?1000l\033[?2004l\033[?1049l'

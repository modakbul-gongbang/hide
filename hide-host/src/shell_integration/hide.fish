# Hide's shell integration for fish. Hide starts a pane's shell with this
# file's data folder first in XDG_DATA_DIRS, so fish reads it with its vendor
# configuration, before the user's own. It gives XDG_DATA_DIRS and ZDOTDIR
# back as the user had them, which come in HIDE_SHELL_XDG_DATA_DIRS and
# HIDE_SHELL_ZDOTDIR, each absent when the user had none. At the first
# prompt, after the user's configuration and anything it asked, it tells
# Herdr the shell has finished starting, which is when Hide may type.
set -l __hide_data (string replace -r '/fish/vendor_conf\.d/[^/]*$' '' -- (status filename))
if string match -q -- "$__hide_data:*" "$XDG_DATA_DIRS"
    if set -q HIDE_SHELL_XDG_DATA_DIRS
        set -gx XDG_DATA_DIRS "$HIDE_SHELL_XDG_DATA_DIRS"
    else
        set -e XDG_DATA_DIRS
    end
end
if test "$ZDOTDIR" = (string replace -r '/fish-data$' '/zsh' -- $__hide_data)
    if set -q HIDE_SHELL_ZDOTDIR
        set -gx ZDOTDIR "$HIDE_SHELL_ZDOTDIR"
    else
        set -e ZDOTDIR
    end
end
set -e HIDE_SHELL_XDG_DATA_DIRS
set -e HIDE_SHELL_ZDOTDIR

status is-interactive; or exit

function __hide_first_prompt --on-event fish_prompt
    functions -e __hide_first_prompt
    if set -q HERDR_BIN_PATH; and set -q HERDR_PANE_ID
        command $HERDR_BIN_PATH pane report-metadata $HERDR_PANE_ID --source hide:shell --token hide_prompt=1 >/dev/null 2>&1
    end
end

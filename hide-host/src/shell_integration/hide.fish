# Hide's shell integration for fish. Hide starts a pane's shell with this
# file's data folder first in XDG_DATA_DIRS, so fish reads it with its vendor
# configuration; it gives XDG_DATA_DIRS back, and at the first prompt, after
# the user's own configuration and anything it asked, tells Herdr the shell
# has finished starting, which is when Hide may type.
set -l __hide_data (string replace -r '/fish/vendor_conf\.d/[^/]*$' '' -- (status filename))
if string match -q -- "$__hide_data:*" "$XDG_DATA_DIRS"
    set -gx XDG_DATA_DIRS (string replace -- "$__hide_data:" '' "$XDG_DATA_DIRS")
end
# A zsh started from here reads the user's own files again.
if test "$ZDOTDIR" = (string replace -r '/fish-data$' '/zsh' -- $__hide_data)
    set -e ZDOTDIR
end

status is-interactive; or exit

function __hide_first_prompt --on-event fish_prompt
    functions -e __hide_first_prompt
    if set -q HERDR_BIN_PATH; and set -q HERDR_PANE_ID
        command $HERDR_BIN_PATH pane report-metadata $HERDR_PANE_ID --source hide:shell --token hide_prompt=1 >/dev/null 2>&1
    end
end

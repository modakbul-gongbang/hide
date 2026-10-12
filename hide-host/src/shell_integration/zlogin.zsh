# Hide's shell integration for zsh: the user's .zlogin, read where zsh would;
# the last startup file of a login shell.
__hide_give
[[ -r ${ZDOTDIR:-$HOME}/.zlogin ]] && builtin source ${ZDOTDIR:-$HOME}/.zlogin
__hide_take
__hide_restore

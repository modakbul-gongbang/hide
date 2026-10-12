# Hide's shell integration for zsh: the user's .zprofile, read where zsh would.
__hide_give
[[ -r ${ZDOTDIR:-$HOME}/.zprofile ]] && builtin source ${ZDOTDIR:-$HOME}/.zprofile
__hide_take
[[ -o rcs ]] || __hide_restore

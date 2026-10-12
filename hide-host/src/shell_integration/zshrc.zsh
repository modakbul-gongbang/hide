# Hide's shell integration for zsh: the user's .zshrc, read where zsh would;
# the last startup file of a shell that is not a login shell.
__hide_give
# A global zshrc (macOS's /etc/zshrc) names the history file after ZDOTDIR,
# which was this folder when it ran.
if [[ ${HISTFILE-} == $__hide_zdotdir/* ]]; then
  HISTFILE=${ZDOTDIR:-$HOME}/${HISTFILE#$__hide_zdotdir/}
fi
[[ -r ${ZDOTDIR:-$HOME}/.zshrc ]] && builtin source ${ZDOTDIR:-$HOME}/.zshrc
__hide_take
if [[ ! -o rcs || ! -o login ]]; then
  __hide_restore
fi

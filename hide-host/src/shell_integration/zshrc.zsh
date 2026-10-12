# Hide's shell integration for zsh: the user's .zshrc, read where zsh would,
# then the hook that marks the first prompt.
ZDOTDIR=$__hide_user_zdotdir
# A global zshrc (macOS's /etc/zshrc) names the history file after ZDOTDIR,
# which was this folder when it ran.
if [[ ${HISTFILE-} == $__hide_zdotdir/* ]]; then
  HISTFILE=$ZDOTDIR/${HISTFILE#$__hide_zdotdir/}
fi
[[ -r $ZDOTDIR/.zshrc ]] && builtin source $ZDOTDIR/.zshrc
__hide_user_zdotdir=$ZDOTDIR
if [[ -o login ]]; then
  ZDOTDIR=$__hide_zdotdir
else
  __hide_restore
fi

# Runs before the first prompt, after every startup file and anything they
# asked: Herdr keeps the mark on the pane, where Hide reads it.
__hide_first_prompt() {
  builtin autoload -Uz add-zsh-hook
  add-zsh-hook -d precmd __hide_first_prompt
  builtin unfunction __hide_first_prompt
  if [[ -n ${HERDR_BIN_PATH-} && -n ${HERDR_PANE_ID-} ]]; then
    "$HERDR_BIN_PATH" pane report-metadata "$HERDR_PANE_ID" --source hide:shell --token hide_prompt=1 >/dev/null 2>&1
  fi
}
builtin autoload -Uz add-zsh-hook && add-zsh-hook precmd __hide_first_prompt

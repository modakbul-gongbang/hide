# Hide's shell integration for zsh: the user's .zlogin, read where zsh would;
# the last startup file of a login shell.
ZDOTDIR=$__hide_user_zdotdir
[[ -r $ZDOTDIR/.zlogin ]] && builtin source $ZDOTDIR/.zlogin
__hide_user_zdotdir=$ZDOTDIR
__hide_restore

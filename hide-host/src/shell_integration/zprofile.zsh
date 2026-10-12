# Hide's shell integration for zsh: the user's .zprofile, read where zsh would.
ZDOTDIR=$__hide_user_zdotdir
[[ -r $ZDOTDIR/.zprofile ]] && builtin source $ZDOTDIR/.zprofile
__hide_user_zdotdir=$ZDOTDIR
ZDOTDIR=$__hide_zdotdir

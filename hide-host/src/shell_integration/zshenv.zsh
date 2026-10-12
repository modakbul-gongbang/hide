# Hide's shell integration for zsh. Hide starts a pane's shell with ZDOTDIR
# naming this folder, so zsh reads its startup files from here. Each file here
# first reads the user's own file of the same name, from where zsh would have
# read it, and the last one of the run gives ZDOTDIR back. At the first prompt
# the shell tells Herdr it has finished starting, which is when Hide may type.
typeset -g __hide_zdotdir=$ZDOTDIR
# The user's own ZDOTDIR is set, if anywhere, by their .zshenv in HOME.
ZDOTDIR=$HOME
[[ -r $ZDOTDIR/.zshenv ]] && builtin source $ZDOTDIR/.zshenv
typeset -g __hide_user_zdotdir=$ZDOTDIR

# Puts back what Hide changed for the startup files, once none is left.
__hide_restore() {
  if [[ $__hide_user_zdotdir == $HOME ]]; then
    builtin unset ZDOTDIR
  else
    export ZDOTDIR=$__hide_user_zdotdir
  fi
  # Hide put its fish folder first in XDG_DATA_DIRS for a fish shell.
  if [[ ${XDG_DATA_DIRS-} == ${__hide_zdotdir:h}/fish-data:* ]]; then
    export XDG_DATA_DIRS=${XDG_DATA_DIRS#${__hide_zdotdir:h}/fish-data:}
  fi
  builtin unset __hide_zdotdir __hide_user_zdotdir
  builtin unfunction __hide_restore
}

if [[ -o interactive || -o login ]]; then
  ZDOTDIR=$__hide_zdotdir
else
  __hide_restore
fi

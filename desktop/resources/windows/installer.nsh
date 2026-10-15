; hide's additions to electron-builder's NSIS installer (PRD auto-update D-12).
;
; Removing hide from Settings > Apps stops its daemon, which runs from the
; build's copy under ~\.hide rather than this folder, so the installer's own
; stop of every process under $INSTDIR does not reach it. The Herdr server and
; its panes are left running: they hold the operator's agents, and
; docs/INSTALL.md gives the command that ends them. An update runs this
; uninstaller too (`--updated`), and then nothing is stopped: the daemon keeps
; answering until the new app replaces it.
!macro customUnInstall
  ${ifNot} ${isUpdated}
    nsExec::Exec '"$INSTDIR\resources\hide.exe" stop'
    Pop $0
  ${endIf}
!macroend

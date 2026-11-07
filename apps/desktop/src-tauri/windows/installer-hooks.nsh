!macro NSIS_HOOK_PREINSTALL
  ; Reuse the registered TokenScope directory for an in-place update.
  ReadRegStr $R8 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp /I "$R8" "$INSTDIR" tokenscope_install_dir_ready

  ; Do not install into a populated directory unless it is this app's registered location.
  StrCpy $R9 0
  FindFirst $R7 $R6 "$INSTDIR\*"
tokenscope_check_install_dir:
  StrCmp $R6 "" tokenscope_close_install_dir
  StrCmp $R6 "." tokenscope_next_install_dir_entry
  StrCmp $R6 ".." tokenscope_next_install_dir_entry
  StrCpy $R9 1
  Goto tokenscope_close_install_dir

tokenscope_next_install_dir_entry:
  FindNext $R7 $R6
  Goto tokenscope_check_install_dir

tokenscope_close_install_dir:
  FindClose $R7
  StrCmp $R9 0 tokenscope_install_dir_ready

  ; Use the first unused TokenScope child directory to preserve existing contents.
  StrCpy $R9 1
  StrCpy $R8 "$INSTDIR\TokenScope"
tokenscope_find_safe_install_dir:
  IfFileExists "$R8\." tokenscope_try_next_install_dir tokenscope_use_safe_install_dir

tokenscope_try_next_install_dir:
  IntOp $R9 $R9 + 1
  StrCpy $R8 "$INSTDIR\TokenScope ($R9)"
  Goto tokenscope_find_safe_install_dir

tokenscope_use_safe_install_dir:
  StrCpy $INSTDIR $R8
  SetOutPath $INSTDIR
  DetailPrint "Selected folder was not empty; installing TokenScope into $INSTDIR"
  IfSilent tokenscope_install_dir_ready
  MessageBox MB_OK|MB_ICONINFORMATION "The selected folder is not empty. TokenScope will be installed in:$\r$\n$INSTDIR"

tokenscope_install_dir_ready:
!macroend

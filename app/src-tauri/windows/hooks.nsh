; Hooks into Tauri's NSIS installer (bundle > windows > nsis > installerHooks).

; .asp files (bundle > fileAssociations, named "AutoScene plugin"). Every
; install, an update (/UPDATE) too, copies the type .asp has now into
; "AutoScene plugin_backup" before it makes .asp AutoScene's, and no uninstall
; runs before an update. So an update would copy AutoScene's own type over
; the one .asp had before AutoScene, and an uninstall after it would have
; nothing to put back. When .asp is AutoScene's already, the copy is kept
; aside here and put back once the install has overwritten it.
!macro NSIS_HOOK_PREINSTALL
  Push $R0
  ReadRegStr $R0 SHCTX "Software\Classes\.asp" ""
  StrCmp $R0 "AutoScene plugin" 0 asp_pre_done
  ClearErrors
  ReadRegStr $R0 SHCTX "Software\Classes\.asp" "AutoScene plugin_backup"
  IfErrors asp_pre_done
  WriteRegStr SHCTX "Software\Classes\.asp" "AutoScene plugin_first" $R0
  asp_pre_done:
  Pop $R0
!macroend

; Installers up to 1.0.1-BETA put the export-bindings dev tool beside the app.
; An update (/UPDATE, as the in-app updater runs it) never runs the old
; uninstaller, and the new one only deletes the files it installs itself, so
; the old copy would stay for good.
; The copy of .asp's earlier type kept aside before the install goes back,
; and Explorer is told that file types changed (Tauri's installer doesn't),
; so it shows .asp files with AutoScene's icon and verb at once.
!macro NSIS_HOOK_POSTINSTALL
  Delete "$INSTDIR\export-bindings.exe"
  Push $R0
  ReadRegStr $R0 SHCTX "Software\Classes\.asp" "AutoScene plugin_backup"
  StrCmp $R0 "AutoScene plugin" 0 asp_post_drop
  ClearErrors
  ReadRegStr $R0 SHCTX "Software\Classes\.asp" "AutoScene plugin_first"
  IfErrors asp_post_drop
  WriteRegStr SHCTX "Software\Classes\.asp" "AutoScene plugin_backup" $R0
  asp_post_drop:
  DeleteRegValue SHCTX "Software\Classes\.asp" "AutoScene plugin_first"
  Pop $R0
  !insertmacro UPDATEFILEASSOC
!macroend

; A copy the update couldn't delete. PRE, not POST: the uninstaller's
; RMDir "$INSTDIR" only removes an empty folder and runs before the POST hook.
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\export-bindings.exe"
!macroend

; Tauri's uninstaller puts back the type .asp had before as its default
; value, even when it had none: an empty value then hides the system's own
; .asp type (Windows reads the user's value first), and when the copy was
; AutoScene's own (an update by an installer without the hooks above) it puts
; back AutoScene's, whose type it just deleted. Either is removed, so .asp
; opens as it did before AutoScene, and so are the copies Tauri and the hooks
; above kept. Explorer is told that file types changed.
!macro NSIS_HOOK_POSTUNINSTALL
  ReadRegStr $R0 SHCTX "Software\Classes\.asp" ""
  StrCmp $R0 "" asp_forget
  StrCmp $R0 "AutoScene plugin" asp_forget asp_keep
  asp_forget:
    DeleteRegValue SHCTX "Software\Classes\.asp" ""
  asp_keep:
  DeleteRegValue SHCTX "Software\Classes\.asp" "AutoScene plugin_backup"
  DeleteRegValue SHCTX "Software\Classes\.asp" "AutoScene plugin_first"
  !insertmacro UPDATEFILEASSOC
!macroend

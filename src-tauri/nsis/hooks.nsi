; NSIS hook for the filer Windows installer.
; Tauri 2 calls these macros at the named install/uninstall phases.
; We use POSTUNINSTALL to remove the "用 filer 归档" right-click entry
; (HKCU, all files) that the app registers on first run — so uninstalling
; filer also cleans up the Explorer context menu.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\filer"
!macroend

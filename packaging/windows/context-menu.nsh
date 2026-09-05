; NSIS installer hooks for PDF Compressor — Windows Explorer context menu
; for .pdf files. Windows 资源管理器右键菜单集成（安装时写入，卸载时移除）。
;
; Wired into the bundle via tauri.conf.json:
;   bundle.windows.nsis.installerHooks = "packaging/windows/context-menu.nsh"
;
; The entries call the headless CLI bundled as a resource next to the app
; executable ($INSTDIR\binaries\pdf-compressor-cli.exe). "SystemFileAssociations"
; keeps the menu off the top-level .pdf shell key so it coexists with other
; PDF tools and never touches the default open verb.

!macro PDFCOMPRESSOR_WRITE_CONTEXT_MENU
  ; A nested "shell" structure turns the entry into a submenu (SubCommands
  ; enumerates children automatically on Windows 7+).
  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor" "MUIVerb" "Compress with PDF Compressor"
  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor" "Icon" "$INSTDIR\pdf-compressor.exe,0"
  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor" "SubCommands" ""

  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor\shell\settings" "MUIVerb" "App settings"
  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor\shell\settings\command" "" '"$INSTDIR\binaries\pdf-compressor-cli.exe" quick "%1"'

  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor\shell\maximum" "MUIVerb" "Maximum shrink"
  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor\shell\maximum\command" "" '"$INSTDIR\binaries\pdf-compressor-cli.exe" quick --preset maximum "%1"'

  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor\shell\fiveMb" "MUIVerb" "Under 5 MB"
  WriteRegStr SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor\shell\fiveMb\command" "" '"$INSTDIR\binaries\pdf-compressor-cli.exe" quick --target-size 5MB "%1"'
!macroend

; Tauri NSIS hook contract: runs after the files are in place.
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PDFCOMPRESSOR_WRITE_CONTEXT_MENU
!macroend

; Tauri NSIS hook contract: runs before files are removed.
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey SHCTX "SystemFileAssociations\.pdf\shell\PDFCompressor"
!macroend

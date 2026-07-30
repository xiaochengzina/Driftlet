; NSIS installer hooks (wired via tauri.conf.json: bundle.windows.nsis.installerHooks).
;
; Point the .dskin file type icon at the bundled dskin.ico instead of the app
; exe icon. The installer creates the association (including a DefaultIcon
; value of "$INSTDIR\Driftlet.exe,0") before NSIS_HOOK_POSTINSTALL runs, so
; this simply overwrites that value. Uninstall needs no registry hook:
; APP_UNASSOCIATE deletes the whole ProgID key.
;
; NOTE: the ProgID below is `bundle.fileAssociations[0].name` from
; tauri.conf.json ("Driftlet Skin Package") — keep the two in sync.
;
; UPDATEFILEASSOC (from FileAssociation.nsh, included by the tauri template
; before this file) calls SHChangeNotify(SHCNE_ASSOCCHANGED) so Explorer
; re-reads the association immediately — without it the icon only appears
; after a manual icon-cache refresh. The template itself never calls it,
; neither on install nor on uninstall, so both hooks must.
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\Driftlet Skin Package\DefaultIcon" "" "$\"$INSTDIR\dskin.ico$\",0"
  !insertmacro UPDATEFILEASSOC
  ; Persist the installer language (HKCU\${MANUPRODUCTKEY}\Installer Language)
  ; exactly as MUI_LANGDLL_DISPLAY would. The uninstaller reads it via
  ; MUI_UNGETLANGUAGE — without it, uninstalling shows a language-selection
  ; dialog because two languages are now bundled and nothing recorded the
  ; install-time choice.
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $LANGUAGE
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro UPDATEFILEASSOC
  ; Two kinds of leftovers the template never removes:
  ; 1) App data. resolve_portable_dir keeps config/skins NEXT TO THE EXE for
  ;    writable install locations (falling back to %APPDATA% for protected
  ;    ones like Program Files); WebView2 data sits in %LOCALAPPDATA%. The
  ;    template only deletes the files it installed itself, then does a
  ;    non-recursive RMDir on $INSTDIR — which fails while config/skins are
  ;    inside, leaving the whole install directory behind.
  ; 2) Registry keys written at install (${MANUPRODUCTKEY}, and our
  ;    "Installer Language" value).
  ; Remove them unconditionally (idempotent). The stock template gated the
  ; %APPDATA% part behind a "Delete app data" checkbox (default UNCHECKED,
  ; skipped on silent/passive uninstalls) — our custom installer.nsi drops
  ; that checkbox entirely, so this hook is the single deletion path.
  ; NEVER on updates ($UpdateMode = 1): the incoming version keeps the
  ; user's config and skins.
  ${If} $UpdateMode <> 1
    RmDir /r "$INSTDIR\config"
    RmDir /r "$INSTDIR\skins"
    ; Non-recursive on purpose: removes the install root only if nothing but
    ; our leftovers kept it non-empty; user files dropped there survive.
    RMDir "$INSTDIR"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}
!macroend

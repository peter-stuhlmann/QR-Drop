; QR Drop -- NSIS installer hooks (CLAUDE.md 5.4, 6.1)
;
; * Context menu "Per QR-Code senden" for files and folders (HKCU, no admin).
; * Start menu entries: send the clipboard text (--clipboard) and receive
;   from the phone (--receive).
; * Inbound firewall rule (TCP, private profile only) for the app. This
;   needs admin rights, so it runs elevated via "runas" (one UAC prompt),
;   and only when the rule is missing or points at another program path.
;   If the user declines, Windows shows its normal firewall dialog on first
;   use instead.
;
; Updates: the Tauri installer first runs the old uninstaller with /UPDATE
; ($UpdateMode = 1). The uninstall hook then keeps menu, shortcuts and the
; firewall rule, so an update never asks for UAC.
;
; Keep this file ASCII: hook files are read as ANSI. ${MAINBINARYNAME} is
; defined after this file is included, so it is only used inside macros.

!define QRDROP_FILE_KEY "Software\Classes\*\shell\QRDrop"
!define QRDROP_DIR_KEY "Software\Classes\Directory\shell\QRDrop"
!define QRDROP_RULE "QR Drop (LAN)"
; Name used by versions up to 0.1.x; removed on install and uninstall.
!define QRDROP_LEGACY_RULE "QR Drop"
!define QRDROP_NETSH "$SYSDIR\netsh.exe"
!define QRDROP_CLIP_DE "QR Drop - Zwischenablage senden"
!define QRDROP_CLIP_EN "QR Drop - Send clipboard"
!define QRDROP_RECV_DE "QR Drop - Vom Handy empfangen"
!define QRDROP_RECV_EN "QR Drop - Receive from phone"

!macro QRDROP_WRITE_MENU KEY LABEL
  WriteRegStr HKCU "${KEY}" "" "${LABEL}"
  WriteRegStr HKCU "${KEY}" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr HKCU "${KEY}" "MultiSelectModel" "Player"
  WriteRegStr HKCU "${KEY}\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
!macroend

; Runs a command line elevated in a hidden cmd.exe (one UAC prompt for all
; netsh calls in it). Failing parts (e.g. deleting a missing rule) are fine.
!macro QRDROP_ELEVATED COMMANDS
  ExecShellWait "runas" "$SYSDIR\cmd.exe" '/c "${COMMANDS}"' SW_HIDE
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; German installer language (1031) -> German texts, otherwise English. The
  ; app repairs the menu with the same rule ("Installer Language").
  ${If} $LANGUAGE == 1031
    !insertmacro QRDROP_WRITE_MENU "${QRDROP_FILE_KEY}" "Per QR-Code senden"
    !insertmacro QRDROP_WRITE_MENU "${QRDROP_DIR_KEY}" "Per QR-Code senden"
  ${Else}
    !insertmacro QRDROP_WRITE_MENU "${QRDROP_FILE_KEY}" "Send via QR code"
    !insertmacro QRDROP_WRITE_MENU "${QRDROP_DIR_KEY}" "Send via QR code"
  ${EndIf}
  ; Let Explorer pick up the new verb right away.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  ; Start menu entries (can be pinned to the taskbar). Shortcuts of the other
  ; language are removed in case an update switched it.
  ${If} $LANGUAGE == 1031
    Delete "$SMPROGRAMS\${QRDROP_CLIP_EN}.lnk"
    Delete "$SMPROGRAMS\${QRDROP_RECV_EN}.lnk"
    CreateShortcut "$SMPROGRAMS\${QRDROP_CLIP_DE}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "--clipboard" "$INSTDIR\${MAINBINARYNAME}.exe" 0
    CreateShortcut "$SMPROGRAMS\${QRDROP_RECV_DE}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "--receive" "$INSTDIR\${MAINBINARYNAME}.exe" 0
  ${Else}
    Delete "$SMPROGRAMS\${QRDROP_CLIP_DE}.lnk"
    Delete "$SMPROGRAMS\${QRDROP_RECV_DE}.lnk"
    CreateShortcut "$SMPROGRAMS\${QRDROP_CLIP_EN}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "--clipboard" "$INSTDIR\${MAINBINARYNAME}.exe" 0
    CreateShortcut "$SMPROGRAMS\${QRDROP_RECV_EN}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "--receive" "$INSTDIR\${MAINBINARYNAME}.exe" 0
  ${EndIf}

  ; Firewall: nothing to do if our rule exists for exactly this program
  ; ("find" exits with 0 if the program path is in the rule details).
  nsExec::Exec '"$SYSDIR\cmd.exe" /c ""${QRDROP_NETSH}" advfirewall firewall show rule name="${QRDROP_RULE}" verbose | "$SYSDIR\find.exe" /i "$INSTDIR\${MAINBINARYNAME}.exe""'
  Pop $0
  ${If} $0 != 0
    ; Missing or for another path: replace it (and the legacy rule) at once.
    !insertmacro QRDROP_ELEVATED '"${QRDROP_NETSH}" advfirewall firewall delete rule name="${QRDROP_RULE}" & "${QRDROP_NETSH}" advfirewall firewall delete rule name="${QRDROP_LEGACY_RULE}" & "${QRDROP_NETSH}" advfirewall firewall add rule name="${QRDROP_RULE}" dir=in action=allow program="$INSTDIR\${MAINBINARYNAME}.exe" protocol=TCP enable=yes profile=private'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; An update keeps everything (see the note at the top).
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "${QRDROP_FILE_KEY}"
    DeleteRegKey HKCU "${QRDROP_DIR_KEY}"
    Delete "$SMPROGRAMS\${QRDROP_CLIP_DE}.lnk"
    Delete "$SMPROGRAMS\${QRDROP_CLIP_EN}.lnk"
    Delete "$SMPROGRAMS\${QRDROP_RECV_DE}.lnk"
    Delete "$SMPROGRAMS\${QRDROP_RECV_EN}.lnk"
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

    ; Remove our rule and the legacy one, with a UAC prompt only if any exists.
    nsExec::Exec '"${QRDROP_NETSH}" advfirewall firewall show rule name="${QRDROP_RULE}"'
    Pop $0
    nsExec::Exec '"${QRDROP_NETSH}" advfirewall firewall show rule name="${QRDROP_LEGACY_RULE}"'
    Pop $1
    ${If} $0 == 0
    ${OrIf} $1 == 0
      !insertmacro QRDROP_ELEVATED '"${QRDROP_NETSH}" advfirewall firewall delete rule name="${QRDROP_RULE}" & "${QRDROP_NETSH}" advfirewall firewall delete rule name="${QRDROP_LEGACY_RULE}"'
    ${EndIf}
  ${EndIf}
!macroend

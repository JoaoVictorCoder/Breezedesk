; Breezedesk installer (NSIS). Built by scripts/package.sh:
;   makensis -DVERSION=1.2.3 -DEXE=path\to\breezedesk.exe -DOUTFILE=dist\Breezedesk-Setup.exe installer/breezedesk.nsi
;
; Installs for the current user only (no administrator rights needed) into
; %LOCALAPPDATA%\Programs\Breezedesk. Widgets live in %APPDATA%\Breezedesk\widgets and are
; never touched by installs or updates.
;
; Command line:
;   /S          silent install (used by the automatic updater)
;   /RELAUNCH   start Breezedesk when done (used by the automatic updater)

; 64-bit installer (Breezedesk itself is 64-bit only).
Target amd64-unicode
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef EXE
  !define EXE "../target/x86_64-pc-windows-gnu/release/breezedesk.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "Breezedesk-Setup.exe"
!endif

!define APP "Breezedesk"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Breezedesk"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Breezedesk"
RequestExecutionLevel user
BrandingText "${APP} ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "LegalCopyright" "MIT License"

!define MUI_ICON "breezedesk.ico"
!define MUI_UNICON "breezedesk.ico"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TEXT "This will install ${APP} ${VERSION}, lightweight desktop widgets for Windows.$\r$\n$\r$\nYour widgets are kept in %APPDATA%\Breezedesk\widgets and are never changed by updates."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\breezedesk.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP} now"
; The "show readme" checkbox is used for "start with Windows".
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Start ${APP} with Windows"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION EnableAutostart
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Asks a running Breezedesk to quit (it closes when its hidden tray window gets
; WM_CLOSE) and waits up to 10 seconds for it to exit.
!macro CloseBreezedesk UN
Function ${UN}CloseBreezedesk
  FindWindow $0 "BreezedeskWidget" "BreezedeskTray"
  ${If} $0 <> 0
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=2000
  ${EndIf}
  ; The same program under its old name (Plume).
  FindWindow $0 "PlumeWidget" "PlumeTray"
  ${If} $0 <> 0
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=2000
  ${EndIf}
  StrCpy $1 0
  ${Do}
    FindWindow $0 "BreezedeskWidget" ""
    FindWindow $2 "PlumeWidget" ""
    ${If} $0 = 0
    ${AndIf} $2 = 0
      ${Break}
    ${EndIf}
    Sleep 200
    IntOp $1 $1 + 1
  ${LoopUntil} $1 >= 50
  Sleep 300
FunctionEnd
!macroend
!insertmacro CloseBreezedesk ""
!insertmacro CloseBreezedesk "un."

Function EnableAutostart
  WriteRegStr HKCU "${RUN_KEY}" "${APP}" '"$INSTDIR\breezedesk.exe"'
FunctionEnd

Section "Install"
  ; Close a running Breezedesk so its files can be replaced.
  Call CloseBreezedesk

  ; ---- Coming from Plume (the project's old name) ----
  ; Widgets and settings move to the new folder, the old copy is removed.
  ${If} ${FileExists} "$APPDATA\Plume\widgets\*.*"
  ${AndIfNot} ${FileExists} "$APPDATA\Breezedesk\*.*"
    Rename "$APPDATA\Plume" "$APPDATA\Breezedesk"
  ${EndIf}
  ReadRegStr $1 HKCU "${RUN_KEY}" "Plume"
  ${If} $1 != ""
    DeleteRegValue HKCU "${RUN_KEY}" "Plume"
    WriteRegStr HKCU "${RUN_KEY}" "${APP}" '"$INSTDIR\breezedesk.exe"'
  ${EndIf}
  ${If} ${FileExists} "$LOCALAPPDATA\Programs\Plume\plume.exe"
    Delete "$LOCALAPPDATA\Programs\Plume\plume.exe"
    Delete "$LOCALAPPDATA\Programs\Plume\plume-editor.exe"
    Delete "$LOCALAPPDATA\Programs\Plume\plume.ico"
    Delete "$LOCALAPPDATA\Programs\Plume\README.md"
    Delete "$LOCALAPPDATA\Programs\Plume\LICENSE.txt"
    Delete "$LOCALAPPDATA\Programs\Plume\CHANGELOG.md"
    Delete "$LOCALAPPDATA\Programs\Plume\uninstall.exe"
    RMDir "$LOCALAPPDATA\Programs\Plume"
  ${EndIf}
  Delete "$SMPROGRAMS\Plume.lnk"
  Delete "$SMPROGRAMS\Plume Editor.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Plume"
  DeleteRegKey HKCU "Software\Classes\Plume.Package"

  SetOutPath "$INSTDIR"
  File "/oname=breezedesk.exe" "${EXE}"
  !ifdef EDITOR
    File "/oname=breezedesk-editor.exe" "${EDITOR}"
  !endif
  File "breezedesk.ico"
  File "/oname=README.md" "../README.md"
  File "/oname=LICENSE.txt" "../LICENSE"
  File "/oname=CHANGELOG.md" "../CHANGELOG.md"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\breezedesk.exe" "" "$INSTDIR\breezedesk.ico"
  !ifdef EDITOR
    CreateShortCut "$SMPROGRAMS\${APP} Editor.lnk" "$INSTDIR\breezedesk-editor.exe" "" "$INSTDIR\breezedesk.ico"
  !endif

  ; Double-clicking a widget package (.breeze, or .plume from before the rename) installs it.
  WriteRegStr HKCU "Software\Classes\.breeze" "" "Breezedesk.Package"
  WriteRegStr HKCU "Software\Classes\.plume" "" "Breezedesk.Package"
  WriteRegStr HKCU "Software\Classes\Breezedesk.Package" "" "Breezedesk widget package"
  WriteRegStr HKCU "Software\Classes\Breezedesk.Package\DefaultIcon" "" "$INSTDIR\breezedesk.ico"
  WriteRegStr HKCU "Software\Classes\Breezedesk.Package\shell\open\command" "" '"$INSTDIR\breezedesk.exe" --install-package "%1"'
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  ; "Apps & features" entry
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "JoaoVictorCoder"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\breezedesk.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/JoaoVictorCoder/Breezedesk"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0

  ; Automatic update: start the new version.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/RELAUNCH" $R1
  ${IfNot} ${Errors}
    Exec '"$INSTDIR\breezedesk.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  Call un.CloseBreezedesk

  Delete "$INSTDIR\breezedesk.exe"
  Delete "$INSTDIR\breezedesk-editor.exe"
  Delete "$INSTDIR\breezedesk.ico"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$SMPROGRAMS\${APP} Editor.lnk"
  DeleteRegKey HKCU "Software\Classes\.breeze"
  DeleteRegKey HKCU "Software\Classes\.plume"
  DeleteRegKey HKCU "Software\Classes\Breezedesk.Package"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegValue HKCU "${RUN_KEY}" "${APP}"
  DeleteRegKey HKCU "Software\Breezedesk"

  MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your widgets ($APPDATA\Breezedesk)?" /SD IDNO IDNO keep
    RMDir /r "$APPDATA\Breezedesk"
  keep:
SectionEnd

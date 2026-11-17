; Plume installer (NSIS). Built by scripts/package.sh:
;   makensis -DVERSION=1.2.3 -DEXE=path\to\plume.exe -DOUTFILE=dist\Plume-Setup.exe installer/plume.nsi
;
; Installs for the current user only (no administrator rights needed) into
; %LOCALAPPDATA%\Programs\Plume. Widgets live in %APPDATA%\Plume\widgets and are
; never touched by installs or updates.
;
; Command line:
;   /S          silent install (used by the automatic updater)
;   /RELAUNCH   start Plume when done (used by the automatic updater)

; 64-bit installer (Plume itself is 64-bit only).
Target amd64-unicode
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef EXE
  !define EXE "../target/x86_64-pc-windows-gnu/release/plume.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "Plume-Setup.exe"
!endif

!define APP "Plume"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Plume"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Plume"
RequestExecutionLevel user
BrandingText "${APP} ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "LegalCopyright" "MIT License"

!define MUI_ICON "plume.ico"
!define MUI_UNICON "plume.ico"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TEXT "This will install ${APP} ${VERSION}, lightweight desktop widgets for Windows.$\r$\n$\r$\nYour widgets are kept in %APPDATA%\Plume\widgets and are never changed by updates."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\plume.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP} now"
; The "show readme" checkbox is used for "start with Windows".
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Start ${APP} with Windows"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION EnableAutostart
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Asks a running Plume to quit (it closes when its hidden tray window gets
; WM_CLOSE) and waits up to 10 seconds for it to exit.
!macro ClosePlume UN
Function ${UN}ClosePlume
  FindWindow $0 "PlumeWidget" "PlumeTray"
  ${If} $0 <> 0
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=2000
  ${EndIf}
  StrCpy $1 0
  ${Do}
    FindWindow $0 "PlumeWidget" ""
    ${If} $0 = 0
      ${Break}
    ${EndIf}
    Sleep 200
    IntOp $1 $1 + 1
  ${LoopUntil} $1 >= 50
  Sleep 300
FunctionEnd
!macroend
!insertmacro ClosePlume ""
!insertmacro ClosePlume "un."

Function EnableAutostart
  WriteRegStr HKCU "${RUN_KEY}" "${APP}" '"$INSTDIR\plume.exe"'
FunctionEnd

Section "Install"
  ; Close a running Plume so its files can be replaced.
  Call ClosePlume

  SetOutPath "$INSTDIR"
  File "/oname=plume.exe" "${EXE}"
  File "plume.ico"
  File "/oname=README.md" "../README.md"
  File "/oname=LICENSE.txt" "../LICENSE"
  File "/oname=CHANGELOG.md" "../CHANGELOG.md"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\plume.exe" "" "$INSTDIR\plume.ico"

  ; "Apps & features" entry
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "JoaoVictorCoder"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\plume.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/JoaoVictorCoder/Plume"
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
    Exec '"$INSTDIR\plume.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  Call un.ClosePlume

  Delete "$INSTDIR\plume.exe"
  Delete "$INSTDIR\plume.ico"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegValue HKCU "${RUN_KEY}" "${APP}"
  DeleteRegKey HKCU "Software\Plume"

  MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your widgets ($APPDATA\Plume)?" /SD IDNO IDNO keep
    RMDir /r "$APPDATA\Plume"
  keep:
SectionEnd

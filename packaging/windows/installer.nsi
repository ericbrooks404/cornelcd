; NSIS installer for the Clawd / cornelcd daemon.
; Built by CI on a Windows runner; see .github/workflows/release.yml

!include "MUI2.nsh"

Name "Clawd (cornelcd)"
OutFile "cornelcd-setup.exe"
Unicode True

; Per-user install: no admin rights needed, and autostart is per-user anyway.
InstallDir "$LOCALAPPDATA\Programs\cornelcd"
InstallDirRegKey HKCU "Software\cornelcd" "InstallDir"
RequestExecutionLevel user

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Clawd" SecMain
    SectionIn RO
    SetOutPath "$INSTDIR"
    File "cornelcd.exe"
    File "cornelcdw.exe"
    File "..\..\README.md"
    File "..\..\LICENSE"

    WriteRegStr HKCU "Software\cornelcd" "InstallDir" "$INSTDIR"

    ; Run at login. HKCU\...\Run is the per-user autostart mechanism, and the
    ; daemon lives in the tray rather than showing a window.
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" \
        "Clawd" '"$INSTDIR\cornelcdw.exe"'

    ; Add/Remove Programs entry
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\cornelcd" \
        "DisplayName" "Clawd (cornelcd)"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\cornelcd" \
        "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\cornelcd" \
        "DisplayVersion" "0.1.1"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\cornelcd" \
        "Publisher" "Eric Brooks"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\cornelcd" \
        "NoModify" 1

    CreateShortcut "$SMPROGRAMS\Clawd.lnk" "$INSTDIR\cornelcdw.exe" ""
    WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Start now" SecStart
    Exec '"$INSTDIR\cornelcdw.exe"'
SectionEnd

Section "Uninstall"
    ; Stop it before removing files, or the exe stays locked.
    nsExec::Exec 'taskkill /F /IM cornelcdw.exe'
    nsExec::Exec 'taskkill /F /IM cornelcd.exe'

    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Clawd"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\cornelcd"
    DeleteRegKey HKCU "Software\cornelcd"

    Delete "$SMPROGRAMS\Clawd.lnk"
    Delete "$INSTDIR\cornelcd.exe"
    Delete "$INSTDIR\cornelcdw.exe"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
SectionEnd

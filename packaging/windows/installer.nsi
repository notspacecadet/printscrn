; Stillshot per-user installer. No admin prompt.
Unicode True
Name "Stillshot"
OutFile "dist\Stillshot-Setup.exe"
InstallDir "$LOCALAPPDATA\Stillshot"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "target\release\stillshot.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Stillshot"
  CreateShortcut "$SMPROGRAMS\Stillshot\Stillshot.lnk" "$INSTDIR\stillshot.exe"
  CreateShortcut "$SMPROGRAMS\Stillshot\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Stillshot" "DisplayName" "Stillshot"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Stillshot" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Stillshot" "DisplayVersion" "0.1.1"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Stillshot" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Stillshot" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\stillshot.exe"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\Stillshot\Stillshot.lnk"
  Delete "$SMPROGRAMS\Stillshot\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Stillshot"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Stillshot"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Stillshot"
SectionEnd

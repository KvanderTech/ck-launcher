Unicode true
!include "MUI2.nsh"
!ifndef PACKAGE
  !error "PACKAGE is required"
!endif
!ifndef OUTFILE
  !error "OUTFILE is required"
!endif
Name "ЦК Лаунчер"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\CKLauncher"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "ЦК Лаунчер"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\ck-launcher-qt.exe"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Russian"

Section "ЦК Лаунчер" Main
  SetOutPath "$INSTDIR"
  File /r "${PACKAGE}\*"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\ЦК Лаунчер"
  CreateShortcut "$SMPROGRAMS\ЦК Лаунчер\ЦК Лаунчер.lnk" "$INSTDIR\ck-launcher-qt.exe"
  CreateShortcut "$DESKTOP\ЦК Лаунчер.lnk" "$INSTDIR\ck-launcher-qt.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayName" "ЦК Лаунчер"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayVersion" "0.2.1-beta.1"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Classes\.mrpack" "" "CKLauncher.ModrinthPack"
  WriteRegStr HKCU "Software\Classes\CKLauncher.ModrinthPack\shell\open\command" "" '"$INSTDIR\ck-launcher-qt.exe" "%1"'
SectionEnd

Section "Uninstall"
  RMDir /r "$INSTDIR"
  Delete "$DESKTOP\ЦК Лаунчер.lnk"
  RMDir /r "$SMPROGRAMS\ЦК Лаунчер"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher"
  DeleteRegKey HKCU "Software\Classes\CKLauncher.ModrinthPack"
  DeleteRegValue HKCU "Software\Classes\.mrpack" ""
SectionEnd

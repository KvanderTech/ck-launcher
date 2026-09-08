Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!define CK_VERSION "0.2.3-beta.1"
!define CK_ICON "..\app\src-tauri\icons\icon.ico"
!define MUI_ICON "${CK_ICON}"
!define MUI_UNICON "${CK_ICON}"
!define MUI_BGCOLOR "081A2C"
!define MUI_TEXTCOLOR "F2F7FC"
!define MUI_INSTFILESPAGE_COLORS "D8E8F4 0B2035"
!define MUI_INSTFILESPAGE_PROGRESSBAR colored
!define MUI_PAGE_HEADER_TEXT "Установка ЦК Лаунчера"
!define MUI_PAGE_HEADER_SUBTEXT "Готовим всё для комфортной игры"
!define MUI_CUSTOMFUNCTION_GUIINIT StyleFrame
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.StyleFrame
!ifndef PACKAGE
  !error "PACKAGE is required"
!endif
!ifndef OUTFILE
  !error "OUTFILE is required"
!endif
!ifndef THEME_PLUGIN_DIR
  !error "THEME_PLUGIN_DIR is required (build scripts/installer-theme with -A Win32)"
!endif
!addplugindir /x86-unicode "${THEME_PLUGIN_DIR}"
Name "ЦК Лаунчер"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\CKLauncher"
RequestExecutionLevel user
ManifestDPIAware true
SetCompressor /SOLID lzma
BrandingText "ЦК Лаунчер"
SetFont "Segoe UI" 10
InstallColors 20BBEE 102E46
VIProductVersion "0.2.3.1"
VIAddVersionKey /LANG=1049 "ProductName" "ЦК Лаунчер"
VIAddVersionKey /LANG=1049 "FileDescription" "Установка ЦК Лаунчера"
VIAddVersionKey /LANG=1049 "FileVersion" "${CK_VERSION}"
VIAddVersionKey /LANG=1049 "ProductVersion" "${CK_VERSION}"
VIAddVersionKey /LANG=1049 "LegalCopyright" "KvanderTech"
Page custom WelcomePage WelcomeLeave
!define MUI_PAGE_CUSTOMFUNCTION_SHOW StyleProgress
!insertmacro MUI_PAGE_INSTFILES
Page custom FinishPage FinishLeave
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Russian"

Var Dialog
Var DirectoryInput
Var HeadingFont
Var RunCheck
!macro StyleFrame UN
Function ${UN}StyleFrame
  SetCtlColors $HWNDPARENT F2F7FC 081A2C
  FindWindow $0 "#32770" "" $HWNDPARENT
  SetCtlColors $0 F2F7FC 081A2C
  SetCtlColors $mui.Branding.Text /BRANDING 081A2C 081A2C
  SetCtlColors $mui.Branding.Background /BRANDING 081A2C 081A2C
  ShowWindow $mui.Branding.Text ${SW_HIDE}
  ShowWindow $mui.Branding.Background ${SW_HIDE}
  ShowWindow $mui.Line.Standard ${SW_HIDE}
  ShowWindow $mui.Line.FullWindow ${SW_HIDE}
  GetDlgItem $0 $HWNDPARENT 1036
  ShowWindow $0 ${SW_HIDE}
  SetCtlColors $mui.Button.Next FFFFFF 159CEC
  SetCtlColors $mui.Button.Cancel C0D8EA 102E46
  CKTheme::Apply /NOUNLOAD
FunctionEnd
!macroend
!insertmacro StyleFrame ""
!insertmacro StyleFrame "un."
Function StyleProgress
  CKTheme::Progress /NOUNLOAD
  Call StyleFrame
  SetCtlColors $mui.InstFilesPage F2F7FC 081A2C
  SetCtlColors $mui.InstFilesPage.Text D8E8F4 081A2C
FunctionEnd
Function .onInit
  SetAutoClose true
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "InstallLocation"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd
Function WelcomePage
  !insertmacro MUI_HEADER_TEXT "ЦК Лаунчер" "Minecraft начинается здесь"
  nsDialogs::Create 1018
  Pop $Dialog
  SetCtlColors $Dialog F2F7FC 081A2C
  Call StyleFrame
  ${NSD_CreateLabel} 6u 4u 94% 27u "Готовы к игре?"
  Pop $0
  CreateFont $HeadingFont "Segoe UI" 20 700
  SendMessage $0 ${WM_SETFONT} $HeadingFont 0
  SetCtlColors $0 F2F7FC 081A2C
  ${NSD_CreateLabel} 6u 37u 94% 30u "Сборки, моды, скины и обновления.$\r$\nВсё в одном лаунчере."
  Pop $0
  SetCtlColors $0 91B8D3 081A2C
  ${NSD_CreateLabel} 6u 73u 94% 14u "Папка установки"
  Pop $0
  SetCtlColors $0 80D9FF 081A2C
  ${NSD_CreateDirRequest} 6u 91u 225u 20u "$INSTDIR"
  Pop $DirectoryInput
  SetCtlColors $DirectoryInput F2F7FC 102E46
  ${NSD_CreateBrowseButton} 238u 91u 55u 20u "Обзор"
  Pop $0
  ${NSD_OnClick} $0 BrowseDirectory
  ${NSD_CreateLabel} 6u 120u 94% 26u "${CK_VERSION}$\r$\nВаши сборки и аккаунты сохранятся при обновлении."
  Pop $0
  SetCtlColors $0 91B8D3 081A2C
  GetDlgItem $0 $HWNDPARENT 1
  SendMessage $0 ${WM_SETTEXT} 0 "STR:Установить"
  CKTheme::Welcome /NOUNLOAD
  nsDialogs::Show
FunctionEnd
Function BrowseDirectory
  Pop $0
  nsDialogs::SelectFolderDialog "Папка ЦК Лаунчера" "$INSTDIR"
  Pop $0
  ${If} $0 != error
    ${NSD_SetText} $DirectoryInput $0
  ${EndIf}
FunctionEnd
Function WelcomeLeave
  ${NSD_GetText} $DirectoryInput $INSTDIR
  ${If} $INSTDIR == ""
    MessageBox MB_ICONEXCLAMATION "Выберите папку установки."
    Abort
  ${EndIf}
FunctionEnd
Function FinishPage
  !insertmacro MUI_HEADER_TEXT "Всё готово" "ЦК Лаунчер установлен"
  nsDialogs::Create 1018
  Pop $Dialog
  SetCtlColors $Dialog F2F7FC 081A2C
  Call StyleFrame
  ${NSD_CreateLabel} 6u 4u 94% 28u "Приятной игры!"
  Pop $0
  SendMessage $0 ${WM_SETFONT} $HeadingFont 0
  SetCtlColors $0 F2F7FC 081A2C
  ${NSD_CreateLabel} 6u 42u 94% 52u "Войдите через Microsoft, выберите сборку и нажмите «Играть».$\r$\nНовые версии доступны в настройках лаунчера."
  Pop $0
  SetCtlColors $0 91B8D3 081A2C
  ${NSD_CreateCheckbox} 6u 110u 278u 18u "Запустить ЦК Лаунчер"
  Pop $RunCheck
  ${NSD_Check} $RunCheck
  SetCtlColors $RunCheck 80D9FF 081A2C
  GetDlgItem $0 $HWNDPARENT 1
  SendMessage $0 ${WM_SETTEXT} 0 "STR:Готово"
  CKTheme::Finish /NOUNLOAD
  nsDialogs::Show
FunctionEnd
Function FinishLeave
!ifndef CK_INSTALLER_PREVIEW
  ${NSD_GetState} $RunCheck $0
  ${If} $0 == ${BST_CHECKED}
    Exec '"$INSTDIR\ck-launcher-qt.exe"'
  ${EndIf}
!endif
FunctionEnd

Section "ЦК Лаунчер" Main
!ifdef CK_INSTALLER_PREVIEW
  DetailPrint "Предпросмотр оформления — файлы и настройки не изменяются."
  Sleep 600
!else
  SetOutPath "$INSTDIR"
  File /r "${PACKAGE}\*"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\ЦК Лаунчер"
  CreateShortcut "$SMPROGRAMS\ЦК Лаунчер\ЦК Лаунчер.lnk" "$INSTDIR\ck-launcher-qt.exe"
  CreateShortcut "$DESKTOP\ЦК Лаунчер.lnk" "$INSTDIR\ck-launcher-qt.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayName" "ЦК Лаунчер"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayVersion" "${CK_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayIcon" "$INSTDIR\ck-launcher-qt.exe,0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Classes\.mrpack" "" "CKLauncher.ModrinthPack"
  WriteRegStr HKCU "Software\Classes\CKLauncher.ModrinthPack\shell\open\command" "" '"$INSTDIR\ck-launcher-qt.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\ck-launcher" "" "URL:CK Launcher"
  WriteRegStr HKCU "Software\Classes\ck-launcher" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\ck-launcher\DefaultIcon" "" "$INSTDIR\ck-launcher-qt.exe,0"
  WriteRegStr HKCU "Software\Classes\ck-launcher\shell\open\command" "" '"$INSTDIR\ck-launcher-qt.exe" --activate'
!endif
SectionEnd

Section "Uninstall"
  RMDir /r "$INSTDIR"
  Delete "$DESKTOP\ЦК Лаунчер.lnk"
  RMDir /r "$SMPROGRAMS\ЦК Лаунчер"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher"
  DeleteRegKey HKCU "Software\Classes\CKLauncher.ModrinthPack"
  DeleteRegKey HKCU "Software\Classes\ck-launcher"
  DeleteRegValue HKCU "Software\Classes\.mrpack" ""
SectionEnd

Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!define CK_VERSION "1.0.0"
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
InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "InstallLocation"
RequestExecutionLevel user
ManifestDPIAware true
SetCompressor /SOLID lzma
BrandingText "ЦК Лаунчер"
SetFont "Segoe UI" 10
InstallColors 20BBEE 102E46
VIProductVersion "1.0.0.0"
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
Var PathError
Var UninstallFailed
!ifndef CK_INSTALLER_PREVIEW
  !include "${PACKAGE}.uninstall.nsh"
!endif

; Return an error string for roots, launcher user data and every reparse-point
; ancestor. This check is shared by interactive/silent install and uninstall.
!macro ValidatePathSafety UN
Function ${UN}ValidatePathSafety
  StrCpy $PathError "Выберите отдельную локальную папку для программы. Сборки и аккаунты нельзя использовать как папку установки."
  StrCmp $INSTDIR "" unsafe
  StrCpy $0 $INSTDIR 1 1
  StrCmp $0 ":" 0 unsafe
  StrCpy $0 $INSTDIR 1 2
  StrCmp $0 "\" 0 unsafe
  System::Call 'kernel32::GetFullPathNameW(w "$INSTDIR", i ${NSIS_MAX_STRLEN}, w .r0, p 0) i.r1'
  IntCmp $1 0 unsafe
  IntCmp $1 ${NSIS_MAX_STRLEN} unsafe +1 unsafe
  StrCpy $INSTDIR $0
  System::Call 'kernel32::GetLongPathNameW(w "$INSTDIR", w .r0, i ${NSIS_MAX_STRLEN}) i.r1'
  ${If} $1 > 0
  ${AndIf} $1 < ${NSIS_MAX_STRLEN}
    StrCpy $INSTDIR $0
  ${EndIf}
  trim_separator:
    StrCpy $0 $INSTDIR 1 -1
    StrCmp $0 "\" 0 trimmed
    StrCpy $INSTDIR $INSTDIR -1
    Goto trim_separator
  trimmed:
  StrLen $0 $INSTDIR
  IntCmp $0 3 unsafe unsafe +1
  ${GetRoot} "$INSTDIR" $0
  StrCmp $INSTDIR "$0\" unsafe
  StrCmp $INSTDIR "$0" unsafe
  StrCmp $INSTDIR "$PROFILE" unsafe
  ReadEnvStr $5 "USERPROFILE"
  StrCmp $INSTDIR "$5" unsafe
  StrCmp $INSTDIR "$5\Desktop" unsafe
  StrCmp $INSTDIR "$5\Documents" unsafe
  StrCmp $INSTDIR "$DESKTOP" unsafe
  StrCmp $INSTDIR "$DOCUMENTS" unsafe
  StrCmp $INSTDIR "$APPDATA" unsafe
  StrCmp $INSTDIR "$LOCALAPPDATA" unsafe
  StrCmp $INSTDIR "$PROGRAMFILES" unsafe
  StrCmp $INSTDIR "$PROGRAMFILES64" unsafe
  StrCmp $INSTDIR "$COMMONFILES" unsafe
  StrCmp $INSTDIR "$COMMONFILES64" unsafe
  StrCpy $2 $INSTDIR
  path_loop:
    System::Call 'kernel32::GetLongPathNameW(w r2, w .r3, i ${NSIS_MAX_STRLEN}) i.r4'
    ${If} $4 > 0
    ${AndIf} $4 < ${NSIS_MAX_STRLEN}
      StrCpy $2 $3
    ${EndIf}
    StrCmp $2 "$APPDATA\CKLauncher" unsafe
    StrCmp $2 "$WINDIR" unsafe
    System::Call 'kernel32::GetFileAttributesW(w r2) i.r3'
    ${If} $3 != -1
      IntOp $4 $3 & 0x400
      StrCmp $4 0 +2
        Goto unsafe
    ${EndIf}
    ${GetParent} "$2" $3
    StrCmp $3 "" safe
    StrCmp $3 $2 unsafe
    StrCpy $2 $3
    Goto path_loop
  safe:
    StrCpy $PathError ""
  unsafe:
FunctionEnd
!macroend
!insertmacro ValidatePathSafety ""
!insertmacro ValidatePathSafety "un."

!macro CheckLauncherClosed UN
Function ${UN}CheckProgramFileWritable
  Pop $0
  IfFileExists "$INSTDIR\$0" 0 done
  StrCpy $0 "$INSTDIR\$0"
  System::Call 'kernel32::GetFileAttributesW(w r0) i.r1'
  IntOp $1 $1 & 0x400
  StrCmp $1 0 0 blocked
  ; Opening for write does not modify bytes, but Windows denies it for a
  ; running/mapped executable. Check all programs before replacing any file.
  System::Call 'kernel32::CreateFileW(w r0, i 0x40000000, i 7, p 0, i 3, i 0, p 0) p.r1'
  StrCmp $1 -1 blocked
  System::Call 'kernel32::CloseHandle(p r1)'
  Return
  blocked:
    StrCpy $PathError "Закройте ЦК Лаунчер перед установкой или удалением. Если он уже закрыт, проверьте права доступа к папке."
  done:
FunctionEnd
Function ${UN}CheckLauncherClosed
  Push "ck-launcher-qt.exe"
  Call ${UN}CheckProgramFileWritable
  Push "ck-launcher-service.exe"
  Call ${UN}CheckProgramFileWritable
  Push "ck-launcher-updater.exe"
  Call ${UN}CheckProgramFileWritable
FunctionEnd
!macroend
!insertmacro CheckLauncherClosed ""
!insertmacro CheckLauncherClosed "un."

Function ValidateInstallDirectory
  Call ValidatePathSafety
  StrCmp $PathError "" 0 done
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "InstallLocation"
  StrCmp $0 $INSTDIR 0 check_empty
  IfFileExists "$INSTDIR\ck-launcher-qt.exe" 0 check_empty
  IfFileExists "$INSTDIR\ck-launcher-service.exe" 0 check_empty
  IfFileExists "$INSTDIR\ck-launcher-updater.exe" 0 check_empty
  IfFileExists "$INSTDIR\SHA256SUMS.txt" done
  check_empty:
    FindFirst $0 $1 "$INSTDIR\*"
    StrCmp $0 "" done
    scan_directory:
      StrCmp $1 "" directory_empty
      StrCmp $1 "." next_entry
      StrCmp $1 ".." next_entry
      FindClose $0
      StrCpy $PathError "Папка не пуста и не является установленным ЦК Лаунчером. Выберите новую или пустую папку."
      Goto done
    next_entry:
      FindNext $0 $1
      Goto scan_directory
    directory_empty:
      FindClose $0
  done:
FunctionEnd

; The paths are fixed at package build time, but links created later must never
; redirect installation or removal outside it. Never recursively remove anything.
!macro CheckPackagePath UN
Function ${UN}CheckPackagePath
  StrCpy $2 $1
  loop:
    System::Call 'kernel32::GetFileAttributesW(w r2) i.r3'
    ${If} $3 != -1
      IntOp $4 $3 & 0x400
      StrCmp $4 0 +3
        StrCpy $0 0
        Return
    ${EndIf}
    StrCmp $2 $INSTDIR safe
    ${GetParent} "$2" $2
    StrCmp $2 "" unsafe
    Goto loop
  safe:
    StrCpy $0 1
    Return
  unsafe:
    StrCpy $0 0
FunctionEnd
!macroend
!insertmacro CheckPackagePath ""
!insertmacro CheckPackagePath "un."
Function CheckInstallPackagePath
  Pop $0
  StrCpy $1 "$INSTDIR\$0"
  Call CheckPackagePath
  StrCmp $0 1 done
  StrCpy $PathError "В папке программы найдена ссылка на другую папку или файл. Выберите новую пустую папку для безопасной установки."
  done:
FunctionEnd
Function ValidateInstallPackagePaths
!ifndef CK_INSTALLER_PREVIEW
  !insertmacro CK_VALIDATE_INSTALL_PACKAGE_PATHS
!endif
FunctionEnd
Function un.RemovePackageFile
  Pop $0
  StrCpy $1 "$INSTDIR\$0"
  Call un.CheckPackagePath
  StrCmp $0 1 0 done
  IfFileExists "$1" 0 done
  ClearErrors
  Delete "$1"
  IfErrors 0 done
    StrCpy $UninstallFailed 1
  done:
FunctionEnd
Function un.RemovePackageDirectory
  Pop $0
  StrCpy $1 "$INSTDIR\$0"
  Call un.CheckPackagePath
  StrCmp $0 1 0 done
  RMDir "$1"
  done:
FunctionEnd
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
!ifdef CK_PATH_CHECK
  ; CI-only validation probe. Does not install or write registry values.
  ${GetParameters} $0
  ${GetOptions} "$0" "/CKPATH=" $INSTDIR
  StrCmp $INSTDIR "" 0 +2
    StrCpy $INSTDIR "${PACKAGE}"
  ${GetOptions} "$0" "/CKPACKAGECHECK=" $1
  ${If} $1 == "1"
    Call ValidatePathSafety
    ${If} $PathError == ""
      Call ValidateInstallPackagePaths
    ${EndIf}
  ${Else}
    Call ValidateInstallDirectory
  ${EndIf}
  FileOpen $0 "${PACKAGE}\path-result.txt" w
  ${If} $PathError == ""
    FileWrite $0 "safe"
  ${Else}
    FileWrite $0 "unsafe"
  ${EndIf}
  FileWrite $0 "$\r$\n$INSTDIR$\r$\n$PROFILE$\r$\n$APPDATA"
  FileClose $0
  SetErrorLevel 0
  Quit
!else
!ifdef CK_UNINSTALL_EMIT
  ; CI-only executable: no install, no registry writes, no shortcuts or GUI.
  ; The output path is the compiler-provided isolated package directory.
  ClearErrors
  WriteUninstaller "${PACKAGE}\uninstall.exe"
  IfErrors emit_failed
  SetErrorLevel 0
  Quit
  emit_failed:
  SetErrorLevel 2
  Quit
!else
  SetAutoClose true
!endif
!endif
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
  Call ValidateInstallDirectory
  ${If} $PathError != ""
    MessageBox MB_ICONEXCLAMATION "$PathError"
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
!ifndef CK_UNINSTALL_EMIT
  Call ValidateInstallDirectory
  ${If} $PathError == ""
    Call ValidateInstallPackagePaths
  ${EndIf}
  ${If} $PathError == ""
    Call CheckLauncherClosed
  ${EndIf}
  ${If} $PathError != ""
    MessageBox MB_ICONEXCLAMATION "$PathError" /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  SetOutPath "$INSTDIR"
  File /r "${PACKAGE}\*"
  ; The emitter already bundled uninstall.exe and its checksum. Do not rewrite
  ; it here with a separately compiled binary that would invalidate the manifest.
  CreateDirectory "$SMPROGRAMS\ЦК Лаунчер"
  CreateShortcut "$SMPROGRAMS\ЦК Лаунчер\ЦК Лаунчер.lnk" "$INSTDIR\ck-launcher-qt.exe"
  CreateShortcut "$DESKTOP\ЦК Лаунчер.lnk" "$INSTDIR\ck-launcher-qt.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayName" "ЦК Лаунчер"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayVersion" "${CK_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "DisplayIcon" "$INSTDIR\ck-launcher-qt.exe,0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "UninstallString" '"$INSTDIR\uninstall.exe"'
  ReadRegStr $0 HKCU "Software\Classes\.mrpack" ""
  ${If} $0 != "CKLauncher.ModrinthPack"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "PreviousMrpackAssociation" "$0"
  ${EndIf}
  WriteRegStr HKCU "Software\Classes\.mrpack" "" "CKLauncher.ModrinthPack"
  WriteRegStr HKCU "Software\Classes\CKLauncher.ModrinthPack\shell\open\command" "" '"$INSTDIR\ck-launcher-qt.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\ck-launcher" "" "URL:CK Launcher"
  WriteRegStr HKCU "Software\Classes\ck-launcher" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\ck-launcher\DefaultIcon" "" "$INSTDIR\ck-launcher-qt.exe,0"
  WriteRegStr HKCU "Software\Classes\ck-launcher\shell\open\command" "" '"$INSTDIR\ck-launcher-qt.exe" --activate'
!endif
!endif
SectionEnd

Section "Uninstall"
!ifndef CK_INSTALLER_PREVIEW
  Call un.ValidatePathSafety
  ${If} $PathError == ""
    Call un.CheckLauncherClosed
  ${EndIf}
  ${If} $PathError != ""
    MessageBox MB_ICONEXCLAMATION "$PathError" /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  StrCpy $UninstallFailed 0
  !insertmacro CK_REMOVE_PACKAGE_FILES
  ${If} $UninstallFailed == 1
    MessageBox MB_ICONEXCLAMATION "Не удалось удалить некоторые файлы. Закройте лаунчер и повторите удаление. Личные файлы сохранены." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  !insertmacro CK_REMOVE_PACKAGE_DIRECTORIES
  ; Another installation may now own the shared shortcuts and associations.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "InstallLocation"
  ${If} $0 == $INSTDIR
    Delete "$DESKTOP\ЦК Лаунчер.lnk"
    Delete "$SMPROGRAMS\ЦК Лаунчер\ЦК Лаунчер.lnk"
    RMDir "$SMPROGRAMS\ЦК Лаунчер"
    ReadRegStr $1 HKCU "Software\Classes\.mrpack" ""
    ${If} $1 == "CKLauncher.ModrinthPack"
      ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher" "PreviousMrpackAssociation"
      ${If} $1 != ""
        WriteRegStr HKCU "Software\Classes\.mrpack" "" "$1"
      ${Else}
        DeleteRegValue HKCU "Software\Classes\.mrpack" ""
      ${EndIf}
    ${EndIf}
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher"
    DeleteRegKey HKCU "Software\Classes\CKLauncher.ModrinthPack"
    DeleteRegKey HKCU "Software\Classes\ck-launcher"
  ${EndIf}
  Push "uninstall.exe"
  Call un.RemovePackageFile
  RMDir "$INSTDIR"
!endif
SectionEnd

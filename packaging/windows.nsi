Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
Name "MusicCraft"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\MusicCraft"
InstallDirRegKey HKCU "Software\MusicCraft" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${STAGE}\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Italian"
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "MusicCraft"
VIAddVersionKey /LANG=1033 "FileDescription" "MusicCraft experimental installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "MusicCraft contributors"
!macro Extension EXT
  WriteRegStr HKCU "Software\Classes\.${EXT}\OpenWithProgids" "MusicCraft.Audio" ""
  WriteRegStr HKCU "Software\MusicCraft\Capabilities\FileAssociations" ".${EXT}" "MusicCraft.Audio"
!macroend
Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "MusicCraft requires 64-bit Windows 10 or newer."
    Abort
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "MusicCraft requires Windows 10 or newer."
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd
Function un.onInit
  SetRegView 64
  IfFileExists "$INSTDIR\musiccraft.exe" 0 un_ready
  un_check_running:
    System::Call 'kernel32::CreateFileW(w "$INSTDIR\musiccraft.exe", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
    ${If} $0 == -1
      MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "Close MusicCraft before uninstalling. Your library and pairing will be kept." IDRETRY un_check_running IDCANCEL un_stop
    ${Else}
      System::Call 'kernel32::CloseHandle(p r0)'
    ${EndIf}
    Goto un_ready
  un_stop:
    Abort
  un_ready:
FunctionEnd
Section "MusicCraft"
  SetShellVarContext current
  IfFileExists "$INSTDIR\musiccraft.exe" 0 copy_files
  check_running:
    System::Call 'kernel32::CreateFileW(w "$INSTDIR\musiccraft.exe", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
    ${If} $0 == -1
      MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "Close MusicCraft before updating. Your library and pairing will be kept." IDRETRY check_running IDCANCEL stop_install
    ${Else}
      System::Call 'kernel32::CloseHandle(p r0)'
    ${EndIf}
  copy_files:
    SetOutPath "$INSTDIR"
    File /r "${STAGE}\*"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    CreateDirectory "$SMPROGRAMS\MusicCraft"
    CreateShortcut "$SMPROGRAMS\MusicCraft\MusicCraft.lnk" "$INSTDIR\musiccraft.exe"
    WriteRegStr HKCU "Software\MusicCraft" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "Software\MusicCraft\Capabilities" "ApplicationName" "MusicCraft"
    WriteRegStr HKCU "Software\MusicCraft\Capabilities" "ApplicationDescription" "MusicCraft music player"
    WriteRegStr HKCU "Software\RegisteredApplications" "MusicCraft" "Software\MusicCraft\Capabilities"
    WriteRegStr HKCU "Software\Classes\MusicCraft.Audio" "" "MusicCraft audio"
    WriteRegStr HKCU "Software\Classes\MusicCraft.Audio\DefaultIcon" "" '$\"$INSTDIR\musiccraft.exe$\",0'
    WriteRegStr HKCU "Software\Classes\MusicCraft.Audio\shell\open\command" "" '$\"$INSTDIR\musiccraft.exe$\" $\"%1$\"'
    WriteRegStr HKCU "Software\Classes\Applications\musiccraft.exe\shell\open\command" "" '$\"$INSTDIR\musiccraft.exe$\" $\"%1$\"'
    !insertmacro Extension "flac"
    !insertmacro Extension "wav"
    !insertmacro Extension "aif"
    !insertmacro Extension "aiff"
    !insertmacro Extension "m4a"
    !insertmacro Extension "alac"
    !insertmacro Extension "mp3"
    !insertmacro Extension "aac"
    !insertmacro Extension "ogg"
    !insertmacro Extension "oga"
    !insertmacro Extension "mp4"
    !insertmacro Extension "dsf"
    !insertmacro Extension "dff"
    !insertmacro Extension "dsd"
    !insertmacro Extension "ape"
    !insertmacro Extension "wma"
    !insertmacro Extension "asf"
    !insertmacro Extension "wv"
    !insertmacro Extension "opus"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft" "DisplayName" "MusicCraft"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft" "Publisher" "MusicCraft contributors"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft" "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft" "NoRepair" 1
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
    Goto finished
  stop_install:
    Abort
  finished:
SectionEnd
!macro UnExtension EXT
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "MusicCraft.Audio"
!macroend
Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\MusicCraft\MusicCraft.lnk"
  RMDir "$SMPROGRAMS\MusicCraft"
  DeleteRegValue HKCU "Software\RegisteredApplications" "MusicCraft"
  DeleteRegKey HKCU "Software\MusicCraft"
  DeleteRegKey HKCU "Software\Classes\MusicCraft.Audio"
  DeleteRegKey HKCU "Software\Classes\Applications\musiccraft.exe"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MusicCraft"
  !insertmacro UnExtension "flac"
  !insertmacro UnExtension "wav"
  !insertmacro UnExtension "aif"
  !insertmacro UnExtension "aiff"
  !insertmacro UnExtension "m4a"
  !insertmacro UnExtension "alac"
  !insertmacro UnExtension "mp3"
  !insertmacro UnExtension "aac"
  !insertmacro UnExtension "ogg"
  !insertmacro UnExtension "oga"
  !insertmacro UnExtension "mp4"
  !insertmacro UnExtension "dsf"
  !insertmacro UnExtension "dff"
  !insertmacro UnExtension "dsd"
  !insertmacro UnExtension "ape"
  !insertmacro UnExtension "wma"
  !insertmacro UnExtension "asf"
  !insertmacro UnExtension "wv"
  !insertmacro UnExtension "opus"
  ; Only installed application payload; LOCALAPPDATA\MusicCraft and credentials remain.
  Delete "$INSTDIR\musiccraft.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY.md"
  Delete "$INSTDIR\ATTRIBUTION.md"
  Delete "$INSTDIR\README.txt"
  RMDir /r "$INSTDIR\codecs"
  RMDir /r "$INSTDIR\ThirdPartyLicenses"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

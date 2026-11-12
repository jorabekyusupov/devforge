!include LogicLib.nsh
LangString dfPrerequisites ${LANG_ENGLISH} "DevForge needs Windows 11 x64 and Microsoft Visual C++ v14 x64 Runtime. Install it from https://aka.ms/vc14/vc_redist.x64.exe then retry."
LangString dfPrerequisites ${LANG_RUSSIAN} "DevForge требует Windows 11 x64 и Microsoft Visual C++ v14 x64 Runtime. Установите https://aka.ms/vc14/vc_redist.x64.exe и повторите попытку."
LangString dfPrerequisites ${LANG_UZBEK} "DevForge uchun Windows 11 x64 va Microsoft Visual C++ v14 x64 Runtime kerak. https://aka.ms/vc14/vc_redist.x64.exe ni o'rnatib, qayta urining."
LangString dfQuit ${LANG_ENGLISH} "Quit DevForge from its tray menu and close DevForge CLI/terminal sessions before installing or uninstalling."
LangString dfQuit ${LANG_RUSSIAN} "Завершите DevForge через меню в трее и закройте его CLI/терминалы перед установкой или удалением."
LangString dfQuit ${LANG_UZBEK} "O'rnatish yoki o'chirishdan oldin DevForge tray menyusidan Chiqish amalini bajaring va uning CLI/terminal sessiyalarini yoping."
LangString dfCleanup ${LANG_ENGLISH} "System cleanup is required. Run devforge.exe distribution cleanup-plan with a new output directory, review the plans, then run the bundled devforge-helper.exe apply with each plan and digest as administrator. Retry uninstall. Projects and databases will be preserved."
LangString dfCleanup ${LANG_RUSSIAN} "Нужна очистка системных записей. Создайте планы через devforge.exe distribution cleanup-plan в новом каталоге, проверьте их и выполните devforge-helper.exe apply с каждым планом и digest от администратора. Повторите удаление. Проекты и базы сохраняются."
LangString dfCleanup ${LANG_UZBEK} "Tizim yozuvlarini tozalash kerak. devforge.exe distribution cleanup-plan bilan yangi katalogda rejalarni yarating, tekshiring va devforge-helper.exe apply orqali har bir reja hamda digestni administrator sifatida bajaring. Qayta o'chiring. Loyiha va bazalar saqlanadi."
!macro NSIS_HOOK_PREINSTALL
  SetRegView 64
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ReadRegDWORD $1 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
  ${IfNot} ${IsNativeAMD64}
    IfSilent +2
    MessageBox MB_ICONSTOP|MB_OK "$(dfPrerequisites)"
    SetErrorLevel 4
    Abort
  ${EndIf}
  ${If} $0 < 22000
  ${OrIf} $1 != 1
    IfSilent +2
    MessageBox MB_ICONSTOP|MB_OK "$(dfPrerequisites)"
    SetErrorLevel 4
    Abort
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\devforge.exe"
    nsExec::ExecToStack '"$INSTDIR\devforge.exe" distribution check-stopped'
    Pop $0
    Pop $1
    ${If} $0 != 0
      IfSilent +2
      MessageBox MB_ICONSTOP|MB_OK "$(dfQuit)"
      SetErrorLevel 3
      Abort
    ${EndIf}
  ${EndIf}
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToStack '"$INSTDIR\devforge.exe" distribution check-stopped'
  Pop $0
  Pop $1
  ${If} $0 != 0
    IfSilent +2
    MessageBox MB_ICONSTOP|MB_OK "$(dfQuit)"
    SetErrorLevel 3
    Abort
  ${EndIf}
  ; An in-place upgrade preserves system integration as well as user data.
  ${If} $UpdateMode <> 1
    nsExec::ExecToStack '"$INSTDIR\devforge.exe" distribution check-uninstall'
    Pop $0
    Pop $1
    ${If} $0 != 0
      IfSilent +2
      MessageBox MB_ICONSTOP|MB_OK "$(dfCleanup)"
      SetErrorLevel 3
      Abort
    ${EndIf}
  ${EndIf}
!macroend

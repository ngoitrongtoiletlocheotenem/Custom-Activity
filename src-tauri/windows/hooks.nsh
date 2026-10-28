; Only CurrentActivity's own opt-in per-user startup value is removed.
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "CurrentActivity"
!macroend

$os = Get-CimInstance Win32_OperatingSystem
'OS: ' + $os.Caption + ' build ' + $os.BuildNumber
$lnk = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Driftlet.lnk"
'lnk exists: ' + (Test-Path $lnk)
if (Test-Path $lnk) {
  $sh = New-Object -ComObject WScript.Shell
  'lnk target: ' + $sh.CreateShortcut($lnk).TargetPath
  # The shortcut's stamped AppUserModelID — MUST be "Driftlet" for toasts.
  # "com.driftlet.app" = stale NSIS-installer stamp (builds before the
  # self-heal fix skipped rewriting it and toasts silently never showed;
  # current builds rewrite it on next app launch).
  $shell = New-Object -ComObject Shell.Application
  $item = $shell.NameSpace((Split-Path $lnk)).ParseName((Split-Path $lnk -Leaf))
  'lnk AUMID: ' + $item.ExtendedProperty('System.AppUserModel.ID')
}
'--- notification settings ---'
$root = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings'
Get-ItemProperty $root -ErrorAction SilentlyContinue |
  Select-Object * -ExcludeProperty PS* | Format-List
Get-ItemProperty "$root\Driftlet" -ErrorAction SilentlyContinue |
  Select-Object * -ExcludeProperty PS* | Format-List

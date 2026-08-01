$os = Get-CimInstance Win32_OperatingSystem
'OS: ' + $os.Caption + ' build ' + $os.BuildNumber
$lnk = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Driftlet.lnk"
'lnk exists: ' + (Test-Path $lnk)
if (Test-Path $lnk) {
  $sh = New-Object -ComObject WScript.Shell
  'lnk target: ' + $sh.CreateShortcut($lnk).TargetPath
}
'--- notification settings ---'
$root = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings'
Get-ItemProperty $root -ErrorAction SilentlyContinue |
  Select-Object * -ExcludeProperty PS* | Format-List
Get-ItemProperty "$root\Driftlet" -ErrorAction SilentlyContinue |
  Select-Object * -ExcludeProperty PS* | Format-List

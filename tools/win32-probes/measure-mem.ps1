$drift = Get-CimInstance Win32_Process -Filter "Name='driftlet.exe'"
if (-not $drift) { 'driftlet.exe not running'; exit }
$driftPid = $drift.ProcessId
'all driftlet.exe PIDs: ' + (($drift | ForEach-Object ProcessId) -join ', ')

# WebView2 children of this instance: command line carries the app's user-data-folder
$wv = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
  Where-Object { $_.CommandLine -match 'driftlet' }
$rows = @()
foreach ($p in $wv) {
  $gp = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
  if ($gp) {
    $kind = if ($p.CommandLine -match '--type=renderer') {'renderer'}
            elseif ($p.CommandLine -match '--type=gpu-process') {'gpu'}
            elseif ($p.CommandLine -match '--type=utility') {'utility'}
            elseif ($p.CommandLine -match '--type=') { ($Matches[0] -replace '--type=','') }
            else {'browser'}
    $rows += [pscustomobject]@{ Id=$gp.Id; Kind=$kind; WS_MB=[math]::Round($gp.WorkingSet64/1MB,1); Private_MB=[math]::Round($gp.PrivateMemorySize64/1MB,1) }
  }
}
$rows | Sort-Object Kind | Format-Table -AutoSize
$dp = Get-Process -Id $driftPid
'driftlet.exe WS_MB: ' + [math]::Round($dp.WorkingSet64/1MB,1) + '  Private_MB: ' + [math]::Round($dp.PrivateMemorySize64/1MB,1)
'webview2 tree count: ' + $rows.Count
'webview2 tree WS_MB: ' + [math]::Round(($rows | Measure-Object WS_MB -Sum).Sum,1)
'TOTAL (driftlet + its webview2) WS_MB: ' + [math]::Round(($rows | Measure-Object WS_MB -Sum).Sum + $dp.WorkingSet64/1MB,1)





param(
  [int]$Samples = 6,
  [int]$IntervalSeconds = 10,
  [string]$ProcessName = 'talo'
)

$ErrorActionPreference = 'Stop'
$roots = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
if (-not $roots.Count) { Write-Output "No '$ProcessName' process found. Start Talo first."; exit 1 }


$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
$ids = New-Object System.Collections.Generic.HashSet[uint32]
foreach ($root in $roots) { [void]$ids.Add([uint32]$root.Id) }
$changed = $true
while ($changed) {
  $changed = $false
  foreach ($p in $all) {
    if ($ids.Contains([uint32]$p.ParentProcessId) -and -not $ids.Contains([uint32]$p.ProcessId)) {
      [void]$ids.Add([uint32]$p.ProcessId); $changed = $true
    }
  }
}
$idList = @($ids | ForEach-Object { [int]$_ })
$names = ($all | Where-Object { $ids.Contains([uint32]$_.ProcessId) } | Group-Object Name | Sort-Object Count -Descending |
  ForEach-Object { "$($_.Name) x$($_.Count)" }) -join ', '
Write-Output ("Tracking {0} process(es): {1}" -f $idList.Count, $names)
Write-Output ("{0,-7} {1,10} {2,12} {3,14}" -f 'Sample', 'CPU %/core', 'WorkingSetMB', 'PrivateMB')

$report = @()
for ($i = 1; $i -le $Samples; $i++) {
  $before = @{}
  foreach ($p in Get-Process -Id $idList -ErrorAction SilentlyContinue) {
    $before[$p.Id] = $p.TotalProcessorTime.TotalSeconds
  }
  Start-Sleep -Seconds $IntervalSeconds
  $cpuDelta = 0.0; $working = 0; $private = 0; $count = 0
  foreach ($p in Get-Process -Id $idList -ErrorAction SilentlyContinue) {
    $beforeSeconds = $before[$p.Id]; if ($null -eq $beforeSeconds) { continue }
    $cpuDelta += $p.TotalProcessorTime.TotalSeconds - $beforeSeconds
    $working += [int]($p.WorkingSet64 / 1MB); $private += [int]($p.PrivateMemorySize64 / 1MB); $count++
  }
  $cpuPerCore = if ($IntervalSeconds -gt 0) { [math]::Round(100 * $cpuDelta / $IntervalSeconds, 2) } else { 0 }
  $report += [pscustomobject]@{ Cpu = $cpuPerCore; Working = $working; Private = $private; Count = $count }
  Write-Output ("{0,-7} {1,10} {2,12} {3,14}" -f $i, "$cpuPerCore%", "$($report[-1].Working) MB", "$($report[-1].Private) MB")
}

$avg = [math]::Round(($report | Measure-Object -Property Cpu -Average).Average, 2)
$workingAvg = [math]::Round(($report | Measure-Object -Property Working -Average).Average, 1)
$privateAvg = [math]::Round(($report | Measure-Object -Property Private -Average).Average, 1)
Write-Output ("Average: {0}% of one core, {1} MB working set, {2} MB private across {3} processes." -f $avg, $workingAvg, $privateAvg, $report[-1].Count)
Write-Output "Record the machine model, Windows build, power plan and whether the window or only the tray was active."
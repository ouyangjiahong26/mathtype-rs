#Requires -Version 5.1
<#
.SYNOPSIS
    Bounded probe: ask Word to embed one ProgID's OLE object and report what can be observed.

.DESCRIPTION
    A host that cannot activate an embedded object does not return an error. It sits on a dialog for
    fifteen minutes and then reports that the program is not installed or is not responding, which
    is a quarter of an hour per attempt by hand. This probe performs the same insertion from a
    separate process, waits a bounded number of seconds, and then reports the four things that tell
    the possibilities apart:

      1. what the ProgID resolves to and in which hives its class is registered, read-only
      2. whether the insertion returned, with the class Word saw and the shape it made
      3. what the server's own log gained during this run, which says whether the call arrived
      4. whether a server or an editor process started, and what DCOM wrote to the System log

    Word honours machine-wide registration only, so a class registered in HKCU alone is the failure
    of this step and the probe says so without starting Word at all.

    Word is driven in a background job that never calls `$word.Quit()`, because a modal dialog makes
    quitting block too; the probe instead stops the WINWORD processes this run started. Word
    processes that were already running are left alone.

.PARAMETER ProgId
    The programmatic identifier to embed, for example MathTypeRs.Equation.1 or Equation.DSMT4.

.PARAMETER TimeoutSeconds
    How long the insertion may take before it counts as blocked. Word waits 900 s or more for an
    object that never answers, so anything near the default is conclusive.

.PARAMETER Activate
    Also send the double-click verb (`$shape.OLEFormat.DoVerb(-2)`) to the inserted shape.

.EXAMPLE
    ./ole-word-probe.ps1 -ProgId Equation.DSMT4 -TimeoutSeconds 60

.EXAMPLE
    ./ole-word-probe.ps1 -ProgId MathTypeRs.Equation.1 -TimeoutSeconds 120 -Activate
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string] $ProgId,
    [int] $TimeoutSeconds = 90,
    [switch] $Activate
)

$ErrorActionPreference = 'Stop'

$tempDir = Join-Path $env:TEMP 'mathtype-ole-server'
$log     = Join-Path $tempDir 'server.log'
$started = Get-Date

function Ok([string]$m)   { Write-Host "[ok]   $m" -ForegroundColor Green }
function Note([string]$m) { Write-Host "[note] $m" -ForegroundColor Yellow }
function Fail([string]$m) { Write-Host "[FAIL] $m" -ForegroundColor Red }
function Step([string]$m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }

# `reg.exe` reports a missing key on standard error, which PowerShell 5.1 turns into a terminating
# error while `$ErrorActionPreference` is Stop. This runs it without letting that end the probe.
function Invoke-Reg([string[]]$Arguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & reg @Arguments 2>&1
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    [pscustomobject]@{ Output = @($output); ExitCode = $code }
}

function Get-Pids([string]$Name) {
    @(Get-Process -Name $Name -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
}

# ---------------------------------------------------------------------------------------------
Step "1. what $ProgId resolves to (read-only)"

$machineClsid = $null
$machineKey   = "HKLM:\Software\Classes\$ProgId\CLSID"
if (Test-Path -LiteralPath $machineKey) { $machineClsid = (Get-ItemProperty -LiteralPath $machineKey).'(default)' }

$userClsid = $null
$userKey   = "HKCU:\Software\Classes\$ProgId\CLSID"
if (Test-Path -LiteralPath $userKey) { $userClsid = (Get-ItemProperty -LiteralPath $userKey).'(default)' }

# The 32-bit view of HKLM, which is the one a 32-bit Word reads.
$wow32Clsid = $null
$wow32 = Invoke-Reg @('query', "HKLM\SOFTWARE\Classes\$ProgId\CLSID", '/ve', '/reg:32')
if ($wow32.ExitCode -eq 0) {
    $line = @($wow32.Output | Where-Object { $_ -match 'REG_SZ' }) | Select-Object -First 1
    if ($null -ne $line) { $wow32Clsid = ($line -replace '.*REG_SZ\s+', '').Trim() }
}

Write-Host "  HKLM\Software\Classes\$ProgId\CLSID        : $machineClsid"
Write-Host "  HKLM 32-bit view                          : $wow32Clsid"
Write-Host "  HKCU\Software\Classes\$ProgId\CLSID        : $userClsid"

$classId = @($machineClsid, $wow32Clsid, $userClsid) | Where-Object { $_ } | Select-Object -First 1
if ($null -eq $classId) {
    Fail "$ProgId resolves to no CLSID in HKLM or HKCU, so no host can create it"
    exit 1
}

$classKeys = [ordered]@{
    'HKLM'        = "HKLM:\Software\Classes\CLSID\$classId"
    'HKLM 32-bit' = "HKLM:\Software\Classes\WOW6432Node\CLSID\$classId"
    'HKCU'        = "HKCU:\Software\Classes\CLSID\$classId"
}
$present = [ordered]@{}
foreach ($view in $classKeys.Keys) { $present[$view] = Test-Path -LiteralPath $classKeys[$view] }
Write-Host "  CLSID $classId"
foreach ($view in $classKeys.Keys) {
    Write-Host ("  class key in {0,-12} : {1}" -f $view, $present[$view])
}

if (-not ($present['HKLM'] -or $present['HKLM 32-bit'])) {
    Fail "$classId is not registered machine-wide: Word honours only HKLM when it embeds an object, so this is the failure"
    if ($present['HKCU']) {
        Note 'HKCU alone is enough for COM itself, which is why a script can create the object where Word cannot'
    }
    exit 1
}
Ok "$classId is registered machine-wide"

# ---------------------------------------------------------------------------------------------
Step "2. baseline"

$logBefore = if (Test-Path -LiteralPath $log) { @(Get-Content -LiteralPath $log).Count } else { 0 }
$wordBefore   = Get-Pids 'WINWORD'
$serverBefore = Get-Pids 'mathtype-ole-server'
$editorBefore = Get-Pids 'mathtype'
Write-Host "  server.log lines : $logBefore"
Write-Host "  WINWORD          : $($wordBefore -join ', ')"
Write-Host "  mathtype-ole-server : $($serverBefore -join ', ')"
Write-Host "  mathtype         : $($editorBefore -join ', ')"

# ---------------------------------------------------------------------------------------------
Step "3. ask Word to embed it (job, $TimeoutSeconds s at most)"

$job = Start-Job -ScriptBlock {
    param($progId, $activate)
    $ErrorActionPreference = 'Stop'
    try {
        $word = New-Object -ComObject Word.Application
        $word.Visible = $false
        $word.DisplayAlerts = 0
        $doc = $word.Documents.Add()
        $shape = $doc.InlineShapes.AddOLEObject($progId)
        if ($null -eq $shape) {
            [pscustomobject]@{ Inserted = $false; ClassType = ''; ShapeProg = ''; Width = $null; Height = $null; Note = 'AddOLEObject returned nothing' }
        } else {
            $verbNote = ''
            if ($activate) {
                try { $shape.OLEFormat.DoVerb(-2) } catch { $verbNote = "DoVerb(-2) said: $($_.Exception.Message)" }
            }
            [pscustomobject]@{
                Inserted  = $true
                ClassType = [string]$shape.OLEFormat.ClassType
                ShapeProg = [string]$shape.OLEFormat.ProgID
                Width     = [math]::Round($shape.Width, 1)
                Height    = [math]::Round($shape.Height, 1)
                Note      = $verbNote
            }
        }
    } catch {
        [pscustomobject]@{ Inserted = $false; ClassType = ''; ShapeProg = ''; Width = $null; Height = $null; Note = $_.Exception.Message }
    }
} -ArgumentList $ProgId, $Activate.IsPresent

$finished = Wait-Job -Job $job -Timeout $TimeoutSeconds
$result = $null
$blocked = $null -eq $finished
if ($blocked) {
    Fail "Word did not return within $TimeoutSeconds seconds, which is what a host does when the object never answers (Word waits 900 s or more)"
    Stop-Job -Job $job
} else {
    $received = @(Receive-Job -Job $job -ErrorAction SilentlyContinue)
    $result = $received | Where-Object { $_.PSObject.Properties.Name -contains 'Inserted' } | Select-Object -Last 1
    foreach ($record in @($received | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] })) {
        Note "the job reported: $($record.Exception.Message)"
    }
    if ($null -eq $result) { Note "the job returned nothing (state $($job.State))" }
}

# ---------------------------------------------------------------------------------------------
Step '4. evidence'

if ($null -ne $result) {
    Write-Host "  Inserted  : $($result.Inserted)"
    Write-Host "  ClassType : $($result.ClassType)"
    Write-Host "  ShapeProg : $($result.ShapeProg)"
    Write-Host "  Width/Height : $($result.Width) x $($result.Height)"
    if ($result.Note) { Write-Host "  note      : $($result.Note)" }
}

$lines = if (Test-Path -LiteralPath $log) { @(Get-Content -LiteralPath $log) } else { @() }
$new = if ($lines.Count -gt $logBefore) { $lines[$logBefore..($lines.Count - 1)] } else { @() }
if ($new.Count -gt 0) {
    Write-Host "  --- $($new.Count) new line(s) in $log ---" -ForegroundColor DarkGray
    foreach ($line in $new) { Write-Host "    $line" -ForegroundColor DarkGray }
} else {
    Note "the server log gained nothing; last 40 lines of $log follow"
    foreach ($line in @($lines | Select-Object -Last 40)) { Write-Host "    $line" -ForegroundColor DarkGray }
}

$wordAfter   = Get-Pids 'WINWORD'
$serverAfter = Get-Pids 'mathtype-ole-server'
$editorAfter = Get-Pids 'mathtype'
$newServers = @($serverAfter | Where-Object { $serverBefore -notcontains $_ })
$newEditors = @($editorAfter | Where-Object { $editorBefore -notcontains $_ })
if ($newServers.Count -gt 0) {
    Ok "this run started the equation server: pid $($newServers -join ', ')"
} else {
    Note 'no mathtype-ole-server process appeared, so nothing asked COM to start one'
}
if ($newEditors.Count -gt 0) {
    Ok "this run started the editor: mathtype pid $($newEditors -join ', ')"
} else {
    Note 'no mathtype editor process appeared'
}

$events = @()
try {
    $bare = $classId.Trim('{', '}')
    $events = @(Get-WinEvent -FilterHashtable @{
        LogName      = 'System'
        ProviderName = 'Microsoft-Windows-DistributedCOM'
        StartTime    = $started
    } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -like "*$classId*" -or $_.Message -like "*$bare*" } |
        Select-Object -First 10)
} catch {
    Note "reading the System log failed: $($_.Exception.Message)"
}
if ($events.Count -gt 0) {
    Write-Host "  --- DCOM in the System log mentioning $classId ---" -ForegroundColor DarkGray
    foreach ($event in $events) {
        Write-Host "    [$($event.TimeCreated)] id=$($event.Id) $($event.LevelDisplayName)" -ForegroundColor DarkGray
        Write-Host "      $(($event.Message -split "`n")[0])" -ForegroundColor DarkGray
    }
    Note 'ids 10001, 10010 and 10016 are the ones worth reading'
} else {
    Note "DCOM wrote nothing to the System log about $classId during this run"
}

# ---------------------------------------------------------------------------------------------
Step '5. cleanup'
Stop-Job -Job $job -ErrorAction SilentlyContinue | Out-Null
Remove-Job -Job $job -Force

foreach ($id in @($wordAfter | Where-Object { $wordBefore -notcontains $_ })) {
    Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    Write-Host "  stopped WINWORD pid $id (started by this run)"
}

# ---------------------------------------------------------------------------------------------
if ($null -ne $result -and $result.Inserted) {
    Write-Host ''
    Ok "inserted $ProgId as $($result.ClassType)"
    exit 0
}
Write-Host ''
Fail "$ProgId was not inserted"
exit 1

#Requires -Version 5.1
<#
.SYNOPSIS
    Acceptance test for the MathTypeRs WPS 文字 add-in.

.DESCRIPTION
    One command that installs the add-in, checks the installed files and
    manifests, starts WPS 文字 as a plain process (no COM/VBA), waits for it to
    load add-ins, and proves the add-in actually ran by finding the health probe
    it issues on start-up in the conversion bridge log. Then it closes WPS and
    uninstalls.

    Prints PASS or FAIL, and on FAIL prints the exact step that blocked.

    The bridge log comes from the MATHTYPE_BRIDGE_LOG environment variable or an explicit
    -BridgeLog <path>; there is no default path, so the script never inspects a log it was not
    pointed at.
#>
[CmdletBinding()]
param(
    # The conversion bridge log to watch; MATHTYPE_BRIDGE_LOG fills the default.
    [string]$BridgeLog  = $env:MATHTYPE_BRIDGE_LOG,
    [int]$LaunchTimeout = 150,
    [string]$AddinName  = 'MathTypeRs',
    [string]$Version    = '0.1.0'
)

$ErrorActionPreference = 'Stop'
if (-not $BridgeLog) {
    Write-Host '[FAIL] no bridge log given: pass -BridgeLog <path> or set the MATHTYPE_BRIDGE_LOG environment variable' -ForegroundColor Red
    exit 1
}
$RealName    = "${AddinName}_$Version"
$LinkName    = "${AddinName}_"
$JsAddonsDir = Join-Path $env:APPDATA 'kingsoft\wps\jsaddons'
$TargetDir   = Join-Path $JsAddonsDir $RealName
$LinkDir     = Join-Path $JsAddonsDir $LinkName
$SourceDir   = $PSScriptRoot

$script:Failures = New-Object System.Collections.Generic.List[string]
function Fail([string]$m) { $script:Failures.Add($m) | Out-Null; Write-Host "[FAIL] $m" -ForegroundColor Red }
function Ok([string]$m)   { Write-Host "[ok]   $m" -ForegroundColor Green }

function CountLike($lines, $pat) { @($lines | Where-Object { $_ -match $pat }).Count }

Write-Host '=== 1. install ===' -ForegroundColor Cyan
& (Join-Path $SourceDir 'install.ps1') -AddinName $AddinName -Version $Version

Write-Host '=== 2. files and manifests ===' -ForegroundColor Cyan

$ribbon = Join-Path $TargetDir 'ribbon.xml'
$mainjs = Join-Path $TargetDir 'main.js'
$jsmain = Join-Path $TargetDir 'js\main.js'
foreach ($f in @($ribbon, $mainjs, $jsmain)) {
    if (Test-Path -LiteralPath $f) { Ok "exists: $f" } else { Fail "missing: $f" }
}
if (Test-Path -LiteralPath $LinkDir) { Ok "add-in link exists: $LinkDir" } else { Fail "missing add-in link: $LinkDir" }

foreach ($x in @($ribbon, (Join-Path $JsAddonsDir 'jsplugins.xml'))) {
    if (Test-Path -LiteralPath $x) {
        try { $null = [xml](Get-Content -Raw -Encoding UTF8 -LiteralPath $x); Ok "parses as XML: $x" }
        catch { Fail "not valid XML: $x ($($_.Exception.Message))" }
    } else {
        Fail "missing manifest: $x"
    }
}

# jsplugins.xml must actually list this add-in with name, version and url.
$jsp = Join-Path $JsAddonsDir 'jsplugins.xml'
if (Test-Path -LiteralPath $jsp) {
    try {
        $doc = [xml](Get-Content -Raw -Encoding UTF8 -LiteralPath $jsp)
        $entry = $doc.jsplugins.jsplugin | Where-Object { $_.name -eq $AddinName } | Select-Object -First 1
        if ($entry -and $entry.version -eq $Version -and $entry.url) {
            Ok "jsplugins.xml lists $AddinName v$Version -> $($entry.url)"
        } else {
            Fail "jsplugins.xml entry for $AddinName is missing or wrong (name/version/url)"
        }
    } catch { Fail "could not read jsplugins.xml entry: $($_.Exception.Message)" }
}

$node = Get-Command node -ErrorAction SilentlyContinue
if ($node) {
    & $node.Source --check $mainjs
    if ($LASTEXITCODE -eq 0) { Ok 'main.js passes node --check' }
    else { Fail 'main.js fails node --check' }
} else {
    Write-Warning 'node not found; skipping JavaScript syntax check'
}

Write-Host '=== 3. start WPS and wait for the add-in probe ===' -ForegroundColor Cyan

# A fresh WPS process is required: add-ins only load at start-up.
$running = @(Get-Process -Name wps -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    Write-Host "[verify] closing $($running.Count) running WPS process(es) so add-ins reload"
    $running | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 4
}

$lines = @(Get-Content -LiteralPath $BridgeLog -ErrorAction SilentlyContinue)
if ($lines.Count -eq 0) { Fail "bridge log missing or empty: $BridgeLog" }
$baseListen = CountLike $lines 'listening on'
# The add-in probes /health?src=wps-addin, so the marker is unambiguous and other
# clients hitting the bridge cannot produce a false pass.
$baseHealth = CountLike $lines 'src=wps-addin'
Write-Host "[verify] bridge log baseline: listen=$baseListen health=$baseHealth"

$exe = Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA 'Kingsoft\WPS Office') -Recurse -Filter 'wps.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $exe) { Fail 'could not find wps.exe under %LOCALAPPDATA%\Kingsoft\WPS Office' }
$started = $false
if ($exe) {
    Start-Process -FilePath $exe.FullName | Out-Null
    $started = $true
    Ok "started WPS via $($exe.FullName) (plain process, no COM)"
}

$matched = $null
if ($started) {
    $iters = [Math]::Ceiling($LaunchTimeout / 2)
    for ($i = 0; $i -lt $iters; $i++) {
        Start-Sleep -Seconds 2
        $lines = @(Get-Content -LiteralPath $BridgeLog -ErrorAction SilentlyContinue)
        $l = CountLike $lines 'listening on'
        $h = CountLike $lines 'src=wps-addin'
        if ($l -gt $baseListen) { $baseListen = $l; $baseHealth = $h }  # log rotated; rebaseline
        if ($h -gt $baseHealth) {
            $matched = ($lines | Where-Object { $_ -match 'src=wps-addin' } | Select-Object -Last 1)
            break
        }
    }
    if ($matched) {
        Ok "add-in health probe seen in bridge log: $matched"
        Write-Host "       matched line: $matched" -ForegroundColor Yellow
    } else {
        Fail ("no new '/health?src=wps-addin' line in $BridgeLog within $LaunchTimeout s: WPS did not execute the add-in. " +
              "On WPS personal builds the add-in must be confirmed in the WPS UI before it loads (and local jsplugins loading is restricted from 12.1.0.16910); " +
              "confirm the add-in in WPS once, then re-run.")
    }
}

Write-Host '=== 4. close WPS and uninstall ===' -ForegroundColor Cyan
Get-Process -Name wps -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

& (Join-Path $SourceDir 'uninstall.ps1') -AddinName $AddinName -Version $Version

if (Test-Path -LiteralPath $TargetDir) { Fail "add-in directory still present after uninstall: $TargetDir" }
else { Ok 'add-in directory removed' }

Write-Host ''
if ($script:Failures.Count -eq 0) {
    Write-Host 'PASS' -ForegroundColor Green
    exit 0
} else {
    Write-Host 'FAIL' -ForegroundColor Red
    Write-Host "blocked at: $($script:Failures[0])"
    exit 1
}

#Requires -Version 5.1
<#
.SYNOPSIS
    Remove the MathTypeRs WPS 文字 add-in installed by install.ps1.

.DESCRIPTION
    Deletes the add-in directory (and the "<name>_" junction) and removes this
    add-in's entry from the local add-in manifests (jsplugins.xml, publish.xml,
    authaddin.json). Entries for other add-ins are left untouched; a manifest
    that ends up with no entries is removed altogether.
#>
[CmdletBinding()]
param(
    [string]$AddinName = 'MathTypeRs',
    [string]$Version   = '0.1.0'
)

$ErrorActionPreference = 'Stop'

$RealName    = "${AddinName}_$Version"
$LinkName    = "${AddinName}_"
$JsAddonsDir = Join-Path $env:APPDATA 'kingsoft\wps\jsaddons'
$TargetDir   = Join-Path $JsAddonsDir $RealName
$LinkDir     = Join-Path $JsAddonsDir $LinkName

function Test-ReparsePoint([string]$p) {
    $item = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
    return $item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
}

# --- 1. remove the junction first (link only), then the directory ------------
if (Test-Path -LiteralPath $LinkDir) {
    if (Test-ReparsePoint $LinkDir) {
        [IO.Directory]::Delete($LinkDir, $false)
    } else {
        Remove-Item -LiteralPath $LinkDir -Recurse -Force
    }
    Write-Host "[uninstall] removed $LinkDir"
}
if (Test-Path -LiteralPath $TargetDir) {
    Remove-Item -LiteralPath $TargetDir -Recurse -Force
    Write-Host "[uninstall] removed $TargetDir"
}

# --- 2. remove manifest entries ---------------------------------------------
function Remove-PluginEntry([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    try {
        $doc = New-Object System.Xml.XmlDocument
        $doc.Load($Path)
    } catch {
        Write-Warning "[uninstall] could not parse ${Path}: $($_.Exception.Message)"
        return
    }
    $root = $doc.DocumentElement
    $removed = 0
    foreach ($n in @($root.SelectNodes("jsplugin[@name='$AddinName']"))) {
        $null = $root.RemoveChild($n)
        $removed++
    }
    if ($root.SelectNodes('jsplugin').Count -eq 0) {
        Remove-Item -LiteralPath $Path -Force
        Write-Host "[uninstall] removed empty manifest $Path"
    } else {
        $doc.Save($Path)
        Write-Host "[uninstall] removed $removed entry(ies) from $Path"
    }
}

Remove-PluginEntry (Join-Path $JsAddonsDir 'jsplugins.xml')
Remove-PluginEntry (Join-Path $JsAddonsDir 'publish.xml')

# --- 3. remove authaddin.json entry -----------------------------------------
$AuthPath = Join-Path $JsAddonsDir 'authaddin.json'
if (Test-Path -LiteralPath $AuthPath) {
    try {
        $raw = Get-Content -Raw -Encoding UTF8 -LiteralPath $AuthPath
        $authData = $raw -replace ('^' + [char]0xFEFF), '' | ConvertFrom-Json
        if ($authData.PSObject.Properties['wps']) {
            foreach ($p in @($authData.wps.PSObject.Properties)) {
                if ($p.Name -eq 'namelist') { continue }
                if ($p.Value -is [pscustomobject] -and $p.Value.name -eq $AddinName) {
                    $authData.wps.PSObject.Properties.Remove($p.Name)
                    Write-Host "[uninstall] removed authaddin entry '$($p.Name)'"
                }
            }
            $authData | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $AuthPath -Encoding UTF8
        }
    } catch {
        Write-Warning "[uninstall] could not update authaddin.json: $($_.Exception.Message)"
    }
}

Write-Host '[uninstall] done'

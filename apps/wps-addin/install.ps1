#Requires -Version 5.1
<#
.SYNOPSIS
    Install the MathTypeRs WPS 文字 add-in for local (no server) deployment.

.DESCRIPTION
    Copies the add-in into the per-user WPS add-in directory and registers it in
    the local add-in manifests that WPS reads on start-up:

      * <jsaddons>\MathTypeRs_0.1.0\  - the versioned add-in directory
      * <jsaddons>\MathTypeRs_\       - the "<name>_" directory WPS resolves for name "MathTypeRs"
      * <jsaddons>\jsplugins.xml      - local deployment list (name/version/url)
      * <jsaddons>\publish.xml        - manifest used by newer WPS builds

    Each add-in directory contains ribbon.xml, main.js, js/main.js and an
    index.html that loads js/main.js (WPS generates the same index.html when it
    loads an add-in that has none).

    Existing entries for other add-ins are preserved; only the entry for this
    add-in is added or replaced. A manifest that exists but cannot be parsed is
    never overwritten — the script stops and asks you to fix or move it, so the
    other add-ins it lists are not lost.
#>
[CmdletBinding()]
param(
    [string]$AddinName = 'MathTypeRs',
    [string]$Version   = '0.1.0',
    [string]$AddinType = 'wps'
)

$ErrorActionPreference = 'Stop'

$SourceDir   = $PSScriptRoot
$RealName    = "${AddinName}_$Version"
$LinkName    = "${AddinName}_"
$JsAddonsDir = Join-Path $env:APPDATA 'kingsoft\wps\jsaddons'

$IndexHtml = "<!DOCTYPE html>`n<html>`n<meta charset='utf-8'>`n" +
             "<script type='text/javascript' src='js/main.js'></script>`n</html>"

# --- helpers -----------------------------------------------------------------
function Test-ReparsePoint([string]$p) {
    $item = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
    return $item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
}

function Remove-AnyDir([string]$p) {
    if (-not (Test-Path -LiteralPath $p)) { return }
    if (Test-ReparsePoint $p) { [IO.Directory]::Delete($p, $false) }
    else { Remove-Item -LiteralPath $p -Recurse -Force }
}

# --- 1. copy the add-in files ------------------------------------------------
foreach ($dirName in @($RealName, $LinkName)) {
    $dir = Join-Path $JsAddonsDir $dirName
    Remove-AnyDir $dir
    New-Item -ItemType Directory -Force -Path (Join-Path $dir 'js') | Out-Null
    foreach ($f in @('ribbon.xml', 'main.js')) {
        $src = Join-Path $SourceDir $f
        if (-not (Test-Path -LiteralPath $src)) { throw "missing add-in file: $src" }
        Copy-Item -LiteralPath $src -Destination (Join-Path $dir $f) -Force
    }
    # WPS's generated index.html loads js/main.js.
    Copy-Item -LiteralPath (Join-Path $SourceDir 'main.js') -Destination (Join-Path $dir 'js\main.js') -Force
    [IO.File]::WriteAllText((Join-Path $dir 'index.html'), $IndexHtml)
    Write-Host "[install] copied add-in into $dir"
}

# --- 2. jsplugins.xml / publish.xml ------------------------------------------
function Get-OrCreatePluginXml([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        try {
            $doc = New-Object System.Xml.XmlDocument
            $doc.Load($Path)
        } catch {
            # The file exists but cannot be read. Rebuilding it would silently throw away the
            # entries of every other add-in it lists, so stop and say so instead.
            throw "refusing to overwrite the manifest ${Path}: it is not valid XML ($($_.Exception.Message)). Fix or move the file, then run this script again."
        }
        if (-not ($doc.DocumentElement -and $doc.DocumentElement.Name -eq 'jsplugins')) {
            $rootName = if ($doc.DocumentElement) { $doc.DocumentElement.Name } else { '<none>' }
            throw "refusing to overwrite the manifest ${Path}: its root element is '$rootName', not 'jsplugins'. Fix or move the file, then run this script again."
        }
        return $doc
    }
    $doc = New-Object System.Xml.XmlDocument
    $decl = $doc.CreateXmlDeclaration('1.0', 'UTF-8', $null)
    $null = $doc.AppendChild($decl)
    $root = $doc.CreateElement('jsplugins')
    $null = $doc.AppendChild($root)
    return $doc
}

function Set-PluginEntry([string]$Path, [hashtable]$Attrs) {
    $doc = Get-OrCreatePluginXml $Path
    $root = $doc.DocumentElement
    $before = @($root.SelectNodes("jsplugin[@name='$AddinName']")).Count
    foreach ($n in @($root.SelectNodes("jsplugin[@name='$AddinName']"))) {
        $null = $root.RemoveChild($n)
    }
    $el = $doc.CreateElement('jsplugin')
    foreach ($k in $Attrs.Keys) { $el.SetAttribute($k, [string]$Attrs[$k]) }
    $null = $root.AppendChild($el)
    $doc.Save($Path)
    # Self-check: the other add-ins' entries have to still be there after this write.
    Write-Host "[install] registered '$AddinName' in $Path (replaced $before own entry(ies); the file now lists $(@($root.SelectNodes('jsplugin')).Count) add-in entry(ies))"
}

Set-PluginEntry (Join-Path $JsAddonsDir 'jsplugins.xml') @{
    type    = $AddinType
    enable  = 'true'
    name    = $AddinName
    version = $Version
    url     = $RealName
}

Set-PluginEntry (Join-Path $JsAddonsDir 'publish.xml') @{
    enable = 'enable_dev'
    name   = $AddinName
    url    = $RealName
    type   = $AddinType
}

Write-Host '[install] done'

<#
.SYNOPSIS
    Removes everything install.ps1 registered for the MathType Rs Word add-in.

.DESCRIPTION
    Only the three keys install.ps1 writes are removed, and only for the current user. The built
    add-in itself is left alone.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$progId = 'MathTypeRs.Addin'
$classId = '{8F6C2B41-5A7D-4E93-B0C1-2D4E6F8A9B3C}'

foreach ($key in @(
        "HKCU:\Software\Microsoft\Office\Word\Addins\$progId",
        "HKCU:\Software\Classes\CLSID\$classId",
        "HKCU:\Software\Classes\$progId"
    )) {
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key -Recurse -Force
        Write-Host "removed $key"
    }
}

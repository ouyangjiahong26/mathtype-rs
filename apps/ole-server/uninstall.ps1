#Requires -Version 5.1
<#
.SYNOPSIS
    Removes the MathType Rs equation object's registration.

.DESCRIPTION
    Deletes exactly what install.ps1 wrote, and stops a server process left over from a host that
    is no longer running. The hand-off files and the log under
    %TEMP%\mathtype-ole-server are left alone: they are the evidence a failed verification needs.
    Use -Scope Machine (default, needs an elevated shell) or -Scope User to match how it was
    registered.
#>
[CmdletBinding()]
param(
    [ValidateSet('Machine', 'User')]
    [string] $Scope = 'Machine'
)

$ErrorActionPreference = 'Stop'

$progId      = 'MathTypeRs.Equation.1'
$unversioned = 'MathTypeRs.Equation'
$classId     = '{7C3E9A54-1B62-4E7D-9F03-5A8C2D6E1B47}'
$classes     = if ($Scope -eq 'Machine') { 'HKLM:\Software\Classes' } else { 'HKCU:\Software\Classes' }
$isAdmin     = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if ($Scope -eq 'Machine' -and -not $isAdmin) {
    throw "removing the machine-wide registration needs an elevated shell"
}

foreach ($key in @(
    (Join-Path $classes "CLSID\$classId"),
    (Join-Path $classes $progId),
    (Join-Path $classes $unversioned)
)) {
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key -Recurse -Force
        Write-Host "removed $key"
    }
}

# A server whose host died can otherwise sit in its idle grace for a while after the class is gone.
$running = @(Get-Process -Name 'mathtype-ole-server' -ErrorAction SilentlyContinue)
foreach ($process in $running) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
}
if ($running.Count -gt 0) {
    Write-Host "stopped $($running.Count) running server process(es)"
}

Write-Host "unregistered $progId"

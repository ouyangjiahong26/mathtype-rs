#Requires -Version 5.1
<#
.SYNOPSIS
    Registers the MathType Rs equation object for the current user.

.DESCRIPTION
    Everything is written under HKCU\Software\Classes, so no administrator rights are needed, and
    both Word and WPS 文字 read per-user registration from there:

      HKCU\Software\Classes\CLSID\{clsid}
          (default)                 "MathType Rs equation"
          ProgID                    MathTypeRs.Equation.1
          LocalServer32             "<server.exe>" -Embedding
          DefaultIcon               "<server.exe>",0
          MiscStatus                513   (recompose on resize | device independent rendering)
          Insertable                (empty: the class appears in the hosts' insert-object dialogs)
          AuxUserType\2 / \3        short name and application name
          Verb\0                    the primary verb, "Edit"
      HKCU\Software\Classes\MathTypeRs.Equation.1\CLSID     the class id
      HKCU\Software\Classes\MathTypeRs.Equation\CLSID       the class id, for the unversioned name
      HKCU\Software\Classes\MathTypeRs.Equation\CurVer      MathTypeRs.Equation.1

    The class id is the one in src/clsid.rs; rather than trusting the two to agree, this script
    asks the binary itself with `--registration` and stops if they differ.

.PARAMETER Configuration
    Which build of the server to register; debug by default.

.PARAMETER Scope
    Machine (default) writes to HKLM and therefore needs an elevated shell; User writes the same
    keys to HKCU, which is enough for COM itself (a script can create the object) but is NOT
    honoured by Word when it embeds an object — measured: the same server registered per-user
    makes Word report "the program is not installed or is not responding", while the same server
    registered machine-wide inserts fine. Use User only for experiments.
#>
[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string] $Configuration = 'debug',

    [ValidateSet('Machine', 'User')]
    [string] $Scope = 'Machine'
)

$ErrorActionPreference = 'Stop'

$progId              = 'MathTypeRs.Equation.1'
$unversioned         = 'MathTypeRs.Equation'
$classId             = '{7C3E9A54-1B62-4E7D-9F03-5A8C2D6E1B47}'
$miscStatus          = 0      # the value working equation objects use; 0x10 (16) below is for the 16-bit context
$root                = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$server              = Join-Path $root "target\$Configuration\mathtype-ole-server.exe"
$editor              = Join-Path $root "target\$Configuration\mathtype.exe"

if (-not (Test-Path -LiteralPath $server)) {
    throw "the server has not been built: $server"
}

# The binary is the authority on its own identity.
$reported = (& $server --registration | Where-Object { $_ -like 'clsid=*' }) -replace '^clsid=', ''
if ($reported -ne $classId) {
    throw "the server reports $reported but this script registers $classId"
}

$classes  = if ($Scope -eq 'Machine') { 'HKLM:\Software\Classes' } else { 'HKCU:\Software\Classes' }
$isAdmin  = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if ($Scope -eq 'Machine' -and -not $isAdmin) {
    throw "machine-wide registration needs an elevated shell: rerun this script from an administrator PowerShell (or pass -Scope User purely for experiments, which Word ignores when embedding an object)"
}
$classKey = Join-Path $classes "CLSID\$classId"

# --- the class itself ---------------------------------------------------------
New-Item -Path $classKey -Force | Out-Null
Set-ItemProperty -Path $classKey -Name '(default)' -Value 'MathType Rs equation'

New-Item -Path "$classKey\LocalServer32" -Force | Out-Null
Set-ItemProperty -Path "$classKey\LocalServer32" -Name '(default)' -Value "`"$server`" -Embedding"

New-Item -Path "$classKey\DefaultIcon" -Force | Out-Null
Set-ItemProperty -Path "$classKey\DefaultIcon" -Name '(default)' -Value "`"$server`",0"

# An empty `Insertable` key is what puts the class in the host's own "insert object" list.
New-Item -Path "$classKey\Insertable" -Force | Out-Null

# Clients load a handler into their own process and forward to us through it. Without this the
# host can end up waiting for an object that never answers (Word reports "not responding").
Set-ItemProperty -Path $classKey -Name 'InProcHandler32' -Value 'ole32.dll'
Set-ItemProperty -Path $classKey -Name 'InProcHandler' -Value 'ole2.dll'

Set-ItemProperty -Path $classKey -Name 'ProgID' -Value $progId
Set-ItemProperty -Path $classKey -Name 'VersionIndependentProgID' -Value $unversioned
Set-ItemProperty -Path $classKey -Name 'MiscStatus' -Value "$miscStatus"
New-Item -Path "$classKey\MiscStatus\16" -Force | Out-Null
Set-ItemProperty -Path "$classKey\MiscStatus\16" -Name '(default)' -Value '1'

# The automation surface is reachable late-bound, which is how the hosts and scripts drive it.
New-Item -Path "$classKey\Programmable" -Force | Out-Null

# Formats this object exchanges with the clipboard and with a host, in the conventional order:
# the document, the source, our own data, and a bitmap for display.
$formats = @(
    'Document,1,1,3',
    'Embed_Source,1,8,1',
    '1,1,1,3',
    '3,1,32,1'
)
for ($index = 0; $index -lt $formats.Count; $index++) {
    $key = Join-Path $classKey "DataFormats\GetSet\$index"
    New-Item -Path $key -Force | Out-Null
    Set-ItemProperty -Path $key -Name '(default)' -Value $formats[$index]
}

New-Item -Path "$classKey\AuxUserType\2" -Force | Out-Null
Set-ItemProperty -Path "$classKey\AuxUserType\2" -Name '(default)' -Value 'Equation'
New-Item -Path "$classKey\AuxUserType\3" -Force | Out-Null
Set-ItemProperty -Path "$classKey\AuxUserType\3" -Name '(default)' -Value 'MathType Rs'

# Verb 0 is what a double-click picks; both it and the explicit Edit verb open the editor.
New-Item -Path "$classKey\Verb\0" -Force | Out-Null
Set-ItemProperty -Path "$classKey\Verb\0" -Name '(default)' -Value '&Open,0,2'
New-Item -Path "$classKey\Verb\1" -Force | Out-Null
Set-ItemProperty -Path "$classKey\Verb\1" -Name '(default)' -Value '&Edit,0,2'

# --- the programmatic identifiers ---------------------------------------------
foreach ($name in @($progId, $unversioned)) {
    $key = Join-Path $classes $name
    New-Item -Path $key -Force | Out-Null
    Set-ItemProperty -Path $key -Name '(default)' -Value 'MathType Rs equation'
    New-Item -Path "$key\CLSID" -Force | Out-Null
    Set-ItemProperty -Path "$key\CLSID" -Name '(default)' -Value $classId
}
Set-ItemProperty -Path (Join-Path $classes $unversioned) -Name 'CurVer' -Value $progId

Write-Host "registered $progId -> $classId"
Write-Host "server: $server -Embedding"
Write-Host "editor: $editor"

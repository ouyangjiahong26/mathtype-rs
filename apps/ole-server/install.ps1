#Requires -Version 5.1
<#
.SYNOPSIS
    Registers the MathType Rs equation object under the hive named by -Scope.

.DESCRIPTION
    The keys are written under the `CLSID` and ProgID paths of one hive, chosen by -Scope:
    Machine (the default) writes to HKLM\Software\Classes and therefore needs an elevated shell;
    User writes the same keys to HKCU\Software\Classes without elevation. Word only honours a
    machine-wide registration when it embeds an object, so Machine is the default (see the Scope
    parameter for what was measured):

      HKLM\Software\Classes\CLSID\{clsid}
          (default)                 "MathType Rs equation"
          ProgID                    MathTypeRs.Equation.1
          LocalServer32\            (default) "<server.exe>" -Embedding
          DefaultIcon\              (default) "<server.exe>",0
          InProcHandler32\          (default) "ole32.dll", the handler a host loads for a local server
          InProcHandler\            (default) "ole2.dll", the same declaration for an OLE 1 host
          MiscStatus\              (default) 0
          MiscStatus\16\            (default) 1
          Insertable                (empty: the class appears in the hosts' insert-object dialogs)
          Programmable              (empty: the automation surface is reachable late-bound)
          AuxUserType\2 / \3        short name and application name
          Verb\0                    "&Open,0,2", the verb a double-click picks
          Verb\1                    "&Edit,0,2"
      HKLM\Software\Classes\MathTypeRs.Equation.1\CLSID     the class id
      HKLM\Software\Classes\MathTypeRs.Equation\CLSID       the class id, for the unversioned name
      HKLM\Software\Classes\MathTypeRs.Equation\CurVer      MathTypeRs.Equation.1

    The class key mirrors the one MathType 7.0 Equation registers under
    {0002CE03-0000-0000-C000-000000000046}, which is the control for what Word expects when it
    embeds an equation object, with one deliberate difference: `DataFormats\GetSet` lists the two
    formats this object actually serves (the enhanced metafile and the bitmap, see src/formats.rs)
    rather than the control's four, which name formats it does not serve. A host asks for what is
    advertised there, so advertising more than the object answers is what makes an insertion fail.
    Two parts of that control are left out for the same reason: `Conversion` and `LocalServer` name
    a converter component and a 16-bit host this server does not have.

    Under -Scope User every HKLM above is HKCU instead. The keys are otherwise identical, so
    uninstall.ps1 takes the same -Scope and removes exactly what was written.

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
$miscStatus          = 0      # the value the control registers for the default aspect
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

# Registrations written before this change carried the handler and the status flags as values of
# the class key. Nothing reads them there, so they go, including on a machine that was registered
# with an earlier version of this script.
foreach ($stale in @('InProcHandler32', 'InProcHandler', 'MiscStatus')) {
    Remove-ItemProperty -Path $classKey -Name $stale -ErrorAction SilentlyContinue
}

New-Item -Path "$classKey\LocalServer32" -Force | Out-Null
Set-ItemProperty -Path "$classKey\LocalServer32" -Name '(default)' -Value "`"$server`" -Embedding"

New-Item -Path "$classKey\DefaultIcon" -Force | Out-Null
Set-ItemProperty -Path "$classKey\DefaultIcon" -Name '(default)' -Value "`"$server`",0"

# An empty `Insertable` key is what puts the class in the host's own "insert object" list.
New-Item -Path "$classKey\Insertable" -Force | Out-Null

# The stock OLE handler is what a host loads into its own process for a local server and forwards
# to us through; without it the host can end up waiting for an object that never answers (Word
# reports "not responding"). Both are subkeys whose default value names the handler DLL, which is
# the shape MathType's own equation class uses. The same names as values on the class key are inert.
New-Item -Path "$classKey\InProcHandler32" -Force | Out-Null
Set-ItemProperty -Path "$classKey\InProcHandler32" -Name '(default)' -Value 'ole32.dll'
New-Item -Path "$classKey\InProcHandler" -Force | Out-Null
Set-ItemProperty -Path "$classKey\InProcHandler" -Name '(default)' -Value 'ole2.dll'

Set-ItemProperty -Path $classKey -Name 'ProgID' -Value $progId
Set-ItemProperty -Path $classKey -Name 'VersionIndependentProgID' -Value $unversioned
# The flags for the default aspect go in the default value of the `MiscStatus` subkey. All 164
# classes under HKLM\Software\Classes\CLSID that carry a `MiscStatus` use the subkey this way.
New-Item -Path "$classKey\MiscStatus" -Force | Out-Null
Set-ItemProperty -Path "$classKey\MiscStatus" -Name '(default)' -Value "$miscStatus"
# The control carries this subkey with the same value, so it stays. 16 is not a documented
# DVASPECT, but nothing in the comparison says removing it helps.
New-Item -Path "$classKey\MiscStatus\16" -Force | Out-Null
Set-ItemProperty -Path "$classKey\MiscStatus\16" -Name '(default)' -Value '1'

# The automation surface is reachable late-bound, which is how the hosts and scripts drive it.
New-Item -Path "$classKey\Programmable" -Force | Out-Null

# The formats a host reads to ask this object for its presentation, each written the way the CLSID
# key documents it: `<clipboard format>, <aspect>, <medium>, <direction>`. These are the three the
# object answers `GetData` for (`src/formats.rs`), in the order it prefers them, and nothing else:
# a host that asks for what is advertised here gets a picture, and a host that asks for anything
# else gets `DV_E_FORMATETC`, which is the failure that stops Word's insertion.
$formats = @(
    '3,1,32,1',    # CF_METAFILEPICT, DVASPECT_CONTENT, TYMED_MFPICT, DATADIR_GET
    '14,1,64,1',   # CF_ENHMETAFILE, DVASPECT_CONTENT, TYMED_ENHMF, DATADIR_GET
    '8,1,1,1'      # CF_DIB, DVASPECT_CONTENT, TYMED_HGLOBAL, DATADIR_GET
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

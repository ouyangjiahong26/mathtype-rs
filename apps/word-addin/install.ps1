<#
.SYNOPSIS
    Registers the MathType Rs Word add-in for the current user.

.DESCRIPTION
    Everything is written under HKCU, so no administrator rights are needed:
      HKCU\Software\Classes\<ProgID>\CLSID
      HKCU\Software\Classes\CLSID\<CLSID>\InprocServer32
      HKCU\Software\Microsoft\Office\Word\Addins\<ProgID>

    The class id must match CLSID_ADDIN in src/lib.rs.

.PARAMETER Configuration
    Which build of the add-in to register; release by default.
#>
[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string] $Configuration = 'release'
)

$ErrorActionPreference = 'Stop'

$progId = 'MathTypeRs.Addin'
$classId = '{8F6C2B41-5A7D-4E93-B0C1-2D4E6F8A9B3C}'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$library = Join-Path $root "target\$Configuration\mathtype_word_addin.dll"

if (-not (Test-Path -LiteralPath $library)) {
    throw "the add-in has not been built: $library"
}

# The ProgID, and the class id it stands for.
$progIdKey = "HKCU:\Software\Classes\$progId"
New-Item -Path $progIdKey -Force | Out-Null
Set-ItemProperty -Path $progIdKey -Name '(default)' -Value 'MathType Rs equation add-in for Word'
New-Item -Path "$progIdKey\CLSID" -Force | Out-Null
Set-ItemProperty -Path "$progIdKey\CLSID" -Name '(default)' -Value $classId

# The class itself: an in-process server that can be used from any apartment.
$classKey = "HKCU:\Software\Classes\CLSID\$classId"
New-Item -Path $classKey -Force | Out-Null
Set-ItemProperty -Path $classKey -Name '(default)' -Value $progId
Set-ItemProperty -Path $classKey -Name 'ProgID' -Value $progId
New-Item -Path "$classKey\InprocServer32" -Force | Out-Null
Set-ItemProperty -Path "$classKey\InprocServer32" -Name '(default)' -Value $library
Set-ItemProperty -Path "$classKey\InprocServer32" -Name 'ThreadingModel' -Value 'Both'

# Word's own list of add-ins; load behaviour 3 means "load at startup and connect".
$addinKey = "HKCU:\Software\Microsoft\Office\Word\Addins\$progId"
New-Item -Path $addinKey -Force | Out-Null
Set-ItemProperty -Path $addinKey -Name 'FriendlyName' -Value 'MathType Rs'
Set-ItemProperty -Path $addinKey -Name 'Description' -Value 'Converts between LaTeX and Word equations with the MathType Rs core.'
Set-ItemProperty -Path $addinKey -Name 'LoadBehavior' -Value 3 -Type DWord

# Word keeps a list of add-ins that made it crash and refuses to load those again. An entry that
# names this add-in is a leftover from an earlier failed load, so it is cleared.
$resiliency = 'HKCU:\Software\Microsoft\Office\16.0\Word\Resiliency\DisabledItems'
if (Test-Path -LiteralPath $resiliency) {
    $key = Get-Item -LiteralPath $resiliency
    foreach ($name in $key.GetValueNames()) {
        $value = $key.GetValue($name)
        if ($value -isnot [byte[]]) { continue }
        $text = [Text.Encoding]::Unicode.GetString($value)
        if ($text.Contains($progId) -or $text.Contains($classId)) {
            Remove-ItemProperty -LiteralPath $resiliency -Name $name
            Write-Host "cleared a disabled add-in entry: $name"
        }
    }
}

Write-Host "registered $progId -> $classId"
Write-Host "server: $library"

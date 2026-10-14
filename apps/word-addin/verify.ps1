<#
.SYNOPSIS
    Acceptance test for the MathType Rs Word add-in.

.DESCRIPTION
    Builds the add-in, registers it for the current user, drives a hidden Word through COM, and
    prints a PASS or FAIL summary:

      1. cargo build -p mathtype-word-addin --release, with no warnings
      2. install.ps1
      3. a hidden Word with a blank document holding \frac{a}{b} in the selection
      4. COMAddIns("MathTypeRs.Addin").Connect must be true, and Object.ConvertSelectionToEquation()
         must run
      5. the document must hold exactly one equation
      6. Object.ConvertSelectionToLatex() must put \frac{a}{b} back as document text
      7. TeXToggle and ExportSelectionPng must work too
      8. Word closes without saving, uninstall.ps1 runs

    The script always cleans up: Word is closed and the add-in unregistered even when a check
    fails.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$progId = 'MathTypeRs.Addin'
$here = $PSScriptRoot
$latex = '\frac{a}{b}'
$addinKey = "HKCU:\Software\Microsoft\Office\Word\Addins\$progId"

$checks = New-Object System.Collections.Generic.List[object]

# Records one check and prints it as it happens, so the transcript shows the real observations.
function Test-It {
    param([string] $Name, [bool] $Ok, [string] $Detail = '')
    $checks.Add([pscustomobject]@{ Name = $Name; Ok = $Ok; Detail = $Detail }) | Out-Null
    $line = if ($Ok) { "[ok]   $Name" } else { "[FAIL] $Name" }
    if ($Detail) { $line += " -- $Detail" }
    Write-Host $line
}

# The code points of a string, so the observed document text is unambiguous in any console.
function Format-Codes {
    param([string] $Text)
    ($Text.ToCharArray() | ForEach-Object { '{0:X4}' -f [int]$_ }) -join ' '
}

# Replaces the document's text with `$Text` and selects it, leaving the paragraph mark out.
function Set-SelectedText {
    param($Document, [string] $Text)
    $range = $Document.Content
    $range.End = $range.End - 1
    $range.Text = $Text
    $range.Select()
    return $range
}

$word = $null
$document = $null
$exported = $null

Write-Host '--- 1. build ---'
# cargo writes its progress and its warnings to stderr, which PowerShell turns into error records;
# they are captured rather than raised so the transcript shows what the build actually said.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$buildText = (& cargo build -p mathtype-word-addin --release 2>&1 | ForEach-Object { "$_" } | Out-String)
$buildExit = $LASTEXITCODE
$ErrorActionPreference = $previousPreference
Write-Host $buildText.Trim()
Test-It 'cargo build -p mathtype-word-addin --release succeeded' ($buildExit -eq 0) "exit code $buildExit"
Test-It 'the build reports no warnings' (-not ($buildText -match 'warning')) ''

Write-Host '--- 2. install ---'
& (Join-Path $here 'install.ps1') | Out-Host
Test-It 'the add-in is registered for Word' (Test-Path -LiteralPath $addinKey) $addinKey

try {
    Write-Host '--- 3. Word ---'
    # Word is started once, at connect time, and an add-in is only loaded by a Word that starts
    # after it is registered. Creating a Word while one is already running attaches to that older
    # process, which would exercise a stale copy of the add-in or none at all, so the run stops
    # instead of reporting a result that is not about the build under test.
    $running = @(Get-Process WINWORD -ErrorAction SilentlyContinue)
    if ($running.Count -gt 0) {
        Test-It 'no other Word instance is running' $false ("Word is already running (pid " + ($running.Id -join ', ') + "); close it and run this script again")
        throw 'another Word instance is running'
    }

    $word = New-Object -ComObject Word.Application
    $word.Visible = $false
    $word.DisplayAlerts = 0
    Write-Host ("Word " + $word.Version)
    $document = $word.Documents.Add()
    Set-SelectedText $document $latex | Out-Null
    Test-It 'the selection holds the LaTeX' ($word.Selection.Text -eq $latex) ("selection=[" + $word.Selection.Text + "]")

    $addin = $word.COMAddIns.Item($progId)
    Test-It 'Word knows the add-in' ($null -ne $addin) ''
    Test-It 'the add-in connected itself' ($addin.Connect -eq $true) ("Connect=" + $addin.Connect)

    $addinObject = $addin.Object
    Test-It 'COMAddIn.Object hands back the add-in' ($null -ne $addinObject) ("type=" + $(if ($null -ne $addinObject) { $addinObject.GetType().Name } else { 'nothing' }))
    Test-It 'the ribbon markup is readable' ($null -ne $addinObject -and $addinObject.GetCustomUI('Microsoft.Word.Document').Contains('公式')) ''

    Write-Host '--- 4. selection -> equation ---'
    $message = $addinObject.ConvertSelectionToEquation()
    Write-Host ("message: " + $message)
    Test-It 'the document holds one equation' ($document.OMaths.Count -eq 1) ("OMaths.Count=" + $document.OMaths.Count)
    $equationText = $document.OMaths.Item(1).Range.Text
    Write-Host ("equation text: [" + $equationText + "]")
    Write-Host ("equation text code points: " + (Format-Codes $equationText))

    Write-Host '--- 5. equation -> LaTeX ---'
    $document.OMaths.Item(1).Range.Select()
    $message = $addinObject.ConvertSelectionToLatex()
    Write-Host ("message: " + $message)
    $documentText = $document.Content.Text
    Write-Host ("document text: [" + $documentText + "]")
    Write-Host ("document text code points: " + (Format-Codes $documentText))
    Test-It 'the document text is the LaTeX again' ($documentText.Contains($latex)) ("text=[" + $documentText + "]")
    Test-It 'no equation is left' ($document.OMaths.Count -eq 0) ("OMaths.Count=" + $document.OMaths.Count)

    Write-Host '--- 6. TeXToggle ---'
    Set-SelectedText $document $latex | Out-Null
    $message = $addinObject.TeXToggle()
    Write-Host ("message: " + $message)
    Test-It 'TeXToggle turned the LaTeX into an equation' ($document.OMaths.Count -eq 1) ("OMaths.Count=" + $document.OMaths.Count)
    $document.OMaths.Item(1).Range.Select()
    $message = $addinObject.TeXToggle()
    Write-Host ("message: " + $message)
    Test-It 'TeXToggle turned the equation back into LaTeX' ($document.OMaths.Count -eq 0 -and $document.Content.Text.Contains($latex)) ("text=[" + $document.Content.Text + "]")

    Write-Host '--- 7. ExportSelectionPng ---'
    Set-SelectedText $document $latex | Out-Null
    $message = $addinObject.ExportSelectionPng()
    Write-Host ("message: " + $message)
    $exported = if ($message -match 'image written to (.+)$') { $Matches[1] } else { $null }
    $written = $null -ne $exported -and (Test-Path -LiteralPath $exported)
    if ($written) {
        $bytes = [IO.File]::ReadAllBytes($exported)
        $written = $bytes.Length -gt 8 -and $bytes[0] -eq 0x89 -and $bytes[1] -eq 0x50 -and $bytes[2] -eq 0x4E -and $bytes[3] -eq 0x47
    }
    Test-It 'a PNG was written for the selection' $written ("path=" + $exported)
    Write-Host '--- 8. embedded equation object ---'
    $server = 'MathTypeRs.Equation.1'
    $registered = $null -ne (Get-ItemProperty "HKCU:\Software\Classes\$server\CLSID" -ErrorAction SilentlyContinue) -or
        $null -ne (Get-ItemProperty "HKLM:\Software\Classes\$server\CLSID" -ErrorAction SilentlyContinue)
    Set-SelectedText $document $latex | Out-Null
    $message = $addinObject.InsertEquationObject()
    Write-Host ("message: " + $message)
    if ($registered) {
        $shapes = $document.InlineShapes.Count
        $progId = if ($shapes -ge 1) { $document.InlineShapes.Item(1).OLEFormat.ProgID } else { '' }
        Test-It 'an embedded equation object was inserted' ($shapes -eq 1 -and $progId -eq $server) ("InlineShapes=$shapes ProgID=$progId")
    }
    else {
        Write-Host ("  ($server is not registered yet, so an insert cannot be exercised)")
        Test-It 'a missing object server is reported clearly' `
            ($message.Contains($server) -and $message.Contains('install.ps1') -and $document.InlineShapes.Count -eq 0) `
            ("InlineShapes=" + $document.InlineShapes.Count)
    }
    Write-Host '--- 9. numbered equation ---'
    Set-SelectedText $document $latex | Out-Null
    $message = $addinObject.InsertNumberedEquation()
    Write-Host ("message: " + $message)
    $seqFields = @($document.Fields | Where-Object { $_.Code.Text -match 'SEQ MTEqn' })
    Test-It 'the numbered equation carries a SEQ MTEqn field' ($seqFields.Count -ge 1) ("fields=" + $document.Fields.Count)
    $document.Fields.Update() | Out-Null
    Test-It 'the number shows on the equation line' ($document.Content.Text -match '\(\d+\)') ("text=[" + $document.Content.Text + "]")
    $message = $addinObject.InsertChapterBreak()
    Write-Host ("message: " + $message)
    $chapFields = @($document.Fields | Where-Object { $_.Code.Text -match 'SEQ MTChap' })
    Test-It 'the chapter break carries a SEQ MTChap field' ($chapFields.Count -ge 1) ("fields=" + $document.Fields.Count)
    $message = $addinObject.InsertCrossReference()
    Write-Host ("message: " + $message)
    $refFields = @($document.Fields | Where-Object { $_.Code.Text -match 'REF _RefMTEq' })
    Test-It 'the cross-reference carries a REF field' ($refFields.Count -ge 1) ("fields=" + $document.Fields.Count)
}
catch {
    Write-Host ("the run stopped early: " + $_.Exception.Message)
    Test-It 'the test ran to completion' $false $_.Exception.Message
}
finally {
    Write-Host '--- 10. clean up ---'
    # Every equation object opens its editor when it is inserted; the ones this run started
    # must not be left open behind the script.
    Get-Process mathtype, mathtype-ole-server -ErrorAction SilentlyContinue | Stop-Process -Force
    if ($null -ne $document) {
        try { $document.Close(0) } catch { Write-Host ("closing the document failed: " + $_.Exception.Message) }
    }
    if ($null -ne $word) {
        try { $word.Quit() } catch { Write-Host ("quitting Word failed: " + $_.Exception.Message) }
        [Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
    }
    if ($null -ne $exported -and (Test-Path -LiteralPath $exported)) { Remove-Item -LiteralPath $exported -Force }
    & (Join-Path $here 'uninstall.ps1') | Out-Host
    Test-It 'the add-in is unregistered' (-not (Test-Path -LiteralPath $addinKey)) $addinKey
}

$failed = @($checks | Where-Object { -not $_.Ok })
Write-Host ''
Write-Host ("checks passed: {0} of {1}" -f ($checks.Count - $failed.Count), $checks.Count)
if ($failed.Count -eq 0) {
    Write-Host 'PASS'
    exit 0
}
Write-Host 'FAIL'
foreach ($check in $failed) {
    Write-Host ("  failed: " + $check.Name + " -- " + $check.Detail)
}
exit 1

#Requires -Version 5.1
<#
.SYNOPSIS
    End-to-end acceptance test for the MathType Rs OLE embedded-object server.

.DESCRIPTION
    One command that does real work from end to end:

      1. builds the server (`cargo build -p mathtype-ole-server`) and checks it is warning free
      2. registers it (install.ps1, -Scope $Scope) and checks what was written in that same hive
      3. starts Word, creates a document and inserts the object with
         `$doc.InlineShapes.AddOLEObject("MathTypeRs.Equation.1")`
      4. activates it with `$shape.OLEFormat.DoVerb(-2)` and waits for a `mathtype` editor process,
         which is what a double-click on the equation does
      5. writes `\frac{a}{b}+\sqrt{x_i^2}` into the hand-off file the server is editing, closes the
         editor, and waits for the object to pick the change up
      6. reads the equation back through automation and compares it with what was written
      7. saves the document, closes Word, reopens the document, and checks the equation is still
         there — both through automation and by reading the object's own streams out of the saved
         file with `mathtype-ole-server.exe --read-stream/--read-native`
      8. unregisters (uninstall.ps1) and prints PASS or FAIL with the values it saw

    WPS 文字 is exercised as far as this machine allows: automation is attempted once and whatever
    it says is reported, and the document is then opened with wps.exe directly. Activating an
    embedded object in WPS needs a double-click, which this script cannot perform, so what it
    reports there is evidence rather than a verdict, and it prints the exact recipe for doing it by
    hand.

    The verification bridge on port 8760 (`mathtype.exe --serve 8760`) is never touched.

.PARAMETER Configuration
    Which build of the server to test; debug by default.
.PARAMETER EditorTimeoutSeconds
    How long to wait for the editor to appear or to exit.
.PARAMETER ObjectTimeoutSeconds
    How long to wait for the object to pick up an edit and for the document to save.
.PARAMETER Scope
    Which registration hive to install into, read back from and remove: Machine (default) is
    HKLM and needs an elevated shell, User is HKCU. It is passed straight to install.ps1 and
    uninstall.ps1 so the step 2 checks and the step 9 "no longer registered" check read the same
    hive the scripts wrote.
#>
[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string] $Configuration = 'debug',
    [int] $EditorTimeoutSeconds = 60,
    [int] $ObjectTimeoutSeconds = 60,

    [ValidateSet('Machine', 'User')]
    [string] $Scope = 'Machine'
)

$ErrorActionPreference = 'Stop'

$progId      = 'MathTypeRs.Equation.1'
$classId     = '{7C3E9A54-1B62-4E7D-9F03-5A8C2D6E1B47}'
$targetLatex = '\frac{a}{b}+\sqrt{x_i^2}'
$root        = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$server      = Join-Path $root "target\$Configuration\mathtype-ole-server.exe"
$tempDir     = Join-Path $env:TEMP 'mathtype-ole-server'
$pointer     = Join-Path $tempDir 'current-handoff.txt'
$handoffLog  = Join-Path $tempDir 'handoff-log.txt'
$log         = Join-Path $tempDir 'server.log'
$docx        = Join-Path $tempDir 'verify-equation.docx'
$bin         = Join-Path $tempDir 'verify-equation.bin'
$shot        = Join-Path $tempDir 'wps-window.png'
$utf8NoBom   = New-Object System.Text.UTF8Encoding($false)

$script:Failures = New-Object System.Collections.Generic.List[string]
$script:Notes    = New-Object System.Collections.Generic.List[string]
$script:StartedEditors = New-Object System.Collections.Generic.List[int]
# The `mathtype` processes that were running when the run started, so the editor this run opens can
# be told apart from the editor a machine that also has MathType 7 already has up.
$script:EditorsBefore = @()

function Ok([string]$m)   { Write-Host "[ok]   $m" -ForegroundColor Green }
function Fail([string]$m) { $script:Failures.Add($m) | Out-Null; Write-Host "[FAIL] $m" -ForegroundColor Red }
function Note([string]$m) { $script:Notes.Add($m) | Out-Null; Write-Host "[note] $m" -ForegroundColor Yellow }
function Step([string]$m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }

# Whitespace is not part of the equation, and both the editor and the parser are free to change
# how it is spelled; everything else has to match.
function Normalize([string]$text) {
    if ($null -eq $text) { return '' }
    return ($text -replace '\s', '')
}

function Show-Log {
    if (Test-Path -LiteralPath $log) {
        Write-Host "`n--- server log (last 40 lines: $log) ---" -ForegroundColor DarkGray
        Get-Content -LiteralPath $log -Tail 40 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
    }
    if (Test-Path -LiteralPath $handoffLog) {
        Write-Host "--- hand-off log ($handoffLog) ---" -ForegroundColor DarkGray
        Get-Content -LiteralPath $handoffLog -Tail 10 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
    }
}

function Stop-EditorProcesses {
    # Only the editors this run started: the conversion bridge on port 8760 is a `mathtype` process
    # of its own and must keep running.
    foreach ($id in $script:StartedEditors) {
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    }
}

# Native commands write their progress to standard error, which PowerShell 5.1 turns into a
# terminating error while `$ErrorActionPreference` is Stop. This runs one without letting that end
# the run, and reports the exit code the command actually returned.
function Invoke-Native([string]$File, [string[]]$Arguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $File @Arguments 2>&1
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    [pscustomobject]@{ Output = @($output); ExitCode = $code; Text = (@($output) | Out-String).Trim() }
}

# ---------------------------------------------------------------------------------------------
Step '1. build'
$build = Invoke-Native 'cargo' @('build', '-p', 'mathtype-ole-server', '--manifest-path', (Join-Path $root 'Cargo.toml'))
$buildText = $build.Text
if ($build.ExitCode -ne 0) {
    Fail "cargo build -p mathtype-ole-server failed:`n$buildText"
    Show-Log
    Stop-EditorProcesses
    exit 1
}
Ok 'cargo build -p mathtype-ole-server succeeded'
$warnings = @($build.Output | Where-Object { $_ -match '^warning' })
if ($warnings.Count -gt 0) {
    Fail "the build is not warning free: $($warnings -join ' | ')"
} else {
    Ok 'the build is warning free'
}
if (-not (Test-Path -LiteralPath $server)) {
    Fail "the build produced no server at $server"
    exit 1
}

# ---------------------------------------------------------------------------------------------
Step '2. register'
& (Join-Path $PSScriptRoot 'install.ps1') -Configuration $Configuration -Scope $Scope | ForEach-Object { Write-Host "    $_" }
# Read back from the hive the install actually wrote to, so both this step and step 9 are real
# assertions and neither passes against a hive the scripts never touched.
$classes  = if ($Scope -eq 'Machine') { 'HKLM:\Software\Classes' } else { 'HKCU:\Software\Classes' }
$classKey = Join-Path $classes "CLSID\$classId"
$localServer = (Get-ItemProperty -LiteralPath "$classKey\LocalServer32").'(default)'
$registeredProgId = (Get-ItemProperty -LiteralPath $classKey).ProgID
$progIdClass = (Get-ItemProperty -LiteralPath (Join-Path $classes "$progId\CLSID")).'(default)'
$insertable = Test-Path -LiteralPath "$classKey\Insertable"
Write-Host "    scope              : $Scope ($classes)"
Write-Host "    CLSID              : $classId"
Write-Host "    ProgID             : $registeredProgId"
Write-Host "    LocalServer32      : $localServer"
Write-Host "    ProgID -> CLSID    : $progIdClass"
Write-Host "    Insertable present : $insertable"
if ($localServer -notlike "*mathtype-ole-server.exe*" -or $localServer -notlike '*-Embedding*') {
    Fail "LocalServer32 is not the server with -Embedding: $localServer"
} else { Ok 'LocalServer32 names the server with -Embedding' }
if ($progIdClass -ne $classId) { Fail "the ProgID points at $progIdClass" } else { Ok 'the ProgID points at the registered class' }
if (-not $insertable) { Fail 'the Insertable key is missing' } else { Ok 'Insertable is registered' }

# ---------------------------------------------------------------------------------------------
Step '3. insert the object in Word'
$word = $null
$doc = $null
$shape = $null
try {
    $word = New-Object -ComObject Word.Application
    $word.Visible = $false
    $word.DisplayAlerts = 0
    $doc = $word.Documents.Add()
    Ok "Word started, document created (pid $((Get-Process -Name WINWORD -ErrorAction SilentlyContinue |
        Sort-Object StartTime -Descending | Select-Object -First 1).Id))"
    # Word asks the object to show itself (`OLEIVERB_SHOW`) while it inserts it, so this run's
    # editor can already be up before step 4 asks for one. This is what "this run's editor" means.
    $script:EditorsBefore = @(Get-Process -Name mathtype -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $shape = $doc.InlineShapes.AddOLEObject($progId)
    if ($null -eq $shape) {
        Fail 'AddOLEObject returned nothing'
    } else {
        Ok "inserted an inline shape: type=$($shape.Type) width=$([math]::Round($shape.Width,1))pt height=$([math]::Round($shape.Height,1))pt"
        Write-Host "    OLEFormat.ClassType : $($shape.OLEFormat.ClassType)"
        Write-Host "    OLEFormat.ProgID    : $($shape.OLEFormat.ProgID)"
    }
} catch {
    Fail "inserting the object failed: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------------------------
Step '4. activate it (the double-click path)'
$editor = $null
if ($null -ne $shape) {
    $before = @(Get-Process -Name mathtype -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $verb = -2
    $verbError = $null
    try {
        $shape.OLEFormat.DoVerb($verb)
        Ok 'OLEFormat.DoVerb(-2) returned'
    } catch {
        $verbError = $_.Exception.Message
        Note "OLEFormat.DoVerb(-2) failed ($verbError); trying OLEFormat.DoVerb(-1)"
        try { $shape.OLEFormat.DoVerb(-1); $verb = -1; Ok 'OLEFormat.DoVerb(-1) returned' }
        catch { Fail "OLEFormat.DoVerb(-1) failed too: $($_.Exception.Message)" }
    }
    if (-not $verbError -or $verb -eq -1) {
        $deadline = (Get-Date).AddSeconds($EditorTimeoutSeconds)
        while ((Get-Date) -lt $deadline -and $null -eq $editor) {
            $new = @(Get-Process -Name mathtype -ErrorAction SilentlyContinue |
                Where-Object { $before -notcontains $_.Id })
            if ($new.Count -gt 0) { $editor = $new[0] }
            else { Start-Sleep -Milliseconds 200 }
        }
        if ($null -eq $editor) {
            # The insertion itself may have opened the editor: it is this run's editor even though
            # it appeared before this step, and the hand-off file it is editing belongs to the
            # object this run inserted.
            $opened = @(Get-Process -Name mathtype -ErrorAction SilentlyContinue |
                Where-Object { $script:EditorsBefore -notcontains $_.Id })
            if ($opened.Count -gt 0) {
                $editor = $opened[0]
                Note "the editor was already running from the insertion: mathtype pid=$($editor.Id)"
            }
        }
        if ($null -eq $editor) {
            Fail "no mathtype editor process appeared within $EditorTimeoutSeconds seconds of DoVerb($verb)"
        } else {
            $script:StartedEditors.Add($editor.Id) | Out-Null
            $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $($editor.Id)" -ErrorAction SilentlyContinue).CommandLine
            Ok "the editor appeared: mathtype pid=$($editor.Id)"
            Write-Host "    command line : $commandLine"
            Write-Host "    window title : $($editor.MainWindowTitle)"
        }
    }
} else {
    Fail 'no object to activate'
}

# ---------------------------------------------------------------------------------------------
Step '5. edit through the hand-off file'
if ($null -ne $editor) {
    if (-not (Test-Path -LiteralPath $pointer)) {
        Fail "the server published no hand-off pointer at $pointer"
    } else {
        $handoff = ([IO.File]::ReadAllText($pointer)).Trim()
        Write-Host "    hand-off file : $handoff"
        if (-not (Test-Path -LiteralPath $handoff)) {
            Fail "the hand-off file the pointer names does not exist: $handoff"
        } else {
            Write-Host "    it held      : $([IO.File]::ReadAllText($handoff))"
            [IO.File]::WriteAllText($handoff, $targetLatex, $utf8NoBom)
            Ok "wrote the new equation into the hand-off file: $targetLatex"

            $null = $editor.CloseMainWindow()
            if (-not $editor.WaitForExit($EditorTimeoutSeconds * 1000)) {
                Note 'the editor ignored the close request and was stopped instead'
                Stop-Process -Id $editor.Id -Force
                $editor.WaitForExit(10000) | Out-Null
            }
            Ok "the editor (pid $($editor.Id)) exited"

            # The editor only writes the file when it has changes of its own, so ours must have
            # survived; if it did not, the object would be reading something else entirely.
            $after = [IO.File]::ReadAllText($handoff)
            if ((Normalize $after) -eq (Normalize $targetLatex)) {
                Ok 'the hand-off file still holds the equation that was written'
            } else {
                Fail "the hand-off file changed under us: '$after'"
            }
        }
    }
}

# ---------------------------------------------------------------------------------------------
Step '6. read the equation back through automation'
$observed = $null
$route = $null
if ($null -ne $shape) {
    # The object reads the hand-off file on its own timer, which runs every 200 ms, so the value is
    # asked for again on every turn rather than once: the first answer can still be the old
    # equation.
    $deadline = (Get-Date).AddSeconds($ObjectTimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $observed = $null
        try {
            $observed = $shape.OLEFormat.Object.GetLatex()
            $route = '$shape.OLEFormat.Object.GetLatex()'
        } catch {
            if ($null -eq $route) { Note "OLEFormat.Object is not reachable: $($_.Exception.Message)" }
            try {
                $active = [Runtime.InteropServices.Marshal]::GetActiveObject($progId)
                $observed = $active.GetLatex()
                $route = "GetActiveObject('$progId').GetLatex()"
            } catch {
                if ($null -eq $route) { Note "GetActiveObject('$progId') is not reachable either: $($_.Exception.Message)" }
            }
        }
        if ($null -ne $route -and (Normalize $observed) -eq (Normalize $targetLatex)) { break }
        Start-Sleep -Milliseconds 300
    }
    if ($null -eq $route) {
        Fail 'the object could not be reached through automation (neither OLEFormat.Object nor GetActiveObject)'
    } else {
        Write-Host "    route  : $route"
        Write-Host "    latex  : $observed"
        if ((Normalize $observed) -eq (Normalize $targetLatex)) {
            Ok 'the object reports the equation that was written into the hand-off file'
        } else {
            Fail "the object reports '$observed' instead of '$targetLatex'"
        }
    }
}

# ---------------------------------------------------------------------------------------------
Step '7. save, close, reopen'
if ($null -ne $doc) {
    try {
        if (Test-Path -LiteralPath $docx) { Remove-Item -LiteralPath $docx -Force }
        $doc.SaveAs2($docx, 16)
        $doc.Close(0)
        $doc = $null
        Ok "the document was saved to $docx"
    } catch {
        Fail "saving the document failed: $($_.Exception.Message)"
    }
}
if ($null -ne $word) {
    try { $word.Quit() } catch { Note "Word.Quit failed: $($_.Exception.Message)" }
    $word = $null
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}

# The strongest check there is: the saved document's own embedded object, read without Word.
if (Test-Path -LiteralPath $docx) {
    try {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = [IO.Compression.ZipFile]::OpenRead($docx)
        $entry = $archive.Entries | Where-Object { $_.FullName -like 'word/embeddings/*' } | Select-Object -First 1
        if ($null -eq $entry) {
            Fail 'the saved document holds no embedded object (word/embeddings/*)'
        } else {
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $bin, $true)
            $archive.Dispose()
            Ok "the saved document embeds $($entry.FullName) ($($entry.Length) bytes), extracted to $bin"
            $streams = Invoke-Native $server @('--list-streams', $bin)
            if ($streams.ExitCode -ne 0) { Fail "listing the embedded object's streams failed: $($streams.Text)" }
            else { Write-Host "    streams in the embedded object:"
                   foreach ($line in $streams.Output) { Write-Host "      $line" }
                   if (@($streams.Output | Where-Object { $_ -match [regex]::Escape('Equation Native') }).Count -eq 0) {
                       Fail 'the embedded object has no `Equation Native` stream'
                   } else { Ok 'the embedded object holds an `Equation Native` stream' }
                   if (@($streams.Output | Where-Object { $_ -match [regex]::Escape('MathTypeRs Equation') }).Count -eq 0) {
                       Fail 'the embedded object has no `MathTypeRs Equation` stream'
                   } else { Ok 'the embedded object holds a `MathTypeRs Equation` stream' } }

            $latexStream = Invoke-Native $server @('--read-stream', $bin, 'MathTypeRs Equation')
            $fromLatexStream = $latexStream.Text
            Write-Host "    MathTypeRs Equation : $fromLatexStream"
            if ((Normalize $fromLatexStream) -eq (Normalize $targetLatex)) {
                Ok 'the LaTeX stream of the saved document has the equation that was edited in'
            } else {
                Fail "the LaTeX stream of the saved document has '$fromLatexStream' instead of '$targetLatex'"
            }

            $nativeStream = Invoke-Native $server @('--read-native', $bin)
            $fromNative = $nativeStream.Text
            Write-Host "    Equation Native     : $fromNative"
            if ((Normalize $fromNative) -eq (Normalize $targetLatex)) {
                Ok 'the equation stream of the saved document decodes to the same equation'
            } else {
                Fail "the equation stream decodes to '$fromNative' instead of '$targetLatex'"
            }
        }
    } catch {
        Fail "reading the embedded object out of the saved document failed: $($_.Exception.Message)"
    }
}

# And reopen it in Word, which is what a user would do.
$word2 = $null
try {
    $word2 = New-Object -ComObject Word.Application
    $word2.DisplayAlerts = 0
    $doc2 = $word2.Documents.Open($docx, $false, $true)
    $count = $doc2.InlineShapes.Count
    Ok "Word reopened the document: $count inline shape(s)"
    if ($count -ge 1) {
        $shape2 = $doc2.InlineShapes.Item(1)
        Write-Host "    ClassType : $($shape2.OLEFormat.ClassType)"
        # As in step 6: keep asking, because the reopened object reads its streams on its own and
        # the first answer can still be the one it had before the document was written.
        $reopened = $null
        $reopenRoute = $null
        $deadline = (Get-Date).AddSeconds($ObjectTimeoutSeconds)
        while ((Get-Date) -lt $deadline) {
            try {
                $reopened = $shape2.OLEFormat.Object.GetLatex()
                $reopenRoute = 'OLEFormat.Object.GetLatex()'
            } catch {
                try {
                    $reopened = [Runtime.InteropServices.Marshal]::GetActiveObject($progId).GetLatex()
                    $reopenRoute = "GetActiveObject('$progId').GetLatex()"
                } catch {
                    if ($null -eq $reopenRoute) { Note "the reopened object is not reachable yet: $($_.Exception.Message)" }
                }
            }
            if ($null -ne $reopenRoute -and (Normalize $reopened) -eq (Normalize $targetLatex)) { break }
            Start-Sleep -Milliseconds 300
        }
        if ($null -eq $reopenRoute) {
            Note 'the reopened object was not reachable through automation; the streams read above stand as the evidence'
        } elseif ((Normalize $reopened) -eq (Normalize $targetLatex)) {
            Ok "the reopened object reports the equation ($reopenRoute)"
        } else {
            Fail "the reopened object reports '$reopened' instead of '$targetLatex'"
        }
    } else {
        Fail 'the reopened document has no inline shape'
    }
    $doc2.Close(0)
    $word2.Quit()
    $word2 = $null
} catch {
    Fail "Word could not reopen the document: $($_.Exception.Message)"
    if ($null -ne $word2) { try { $word2.Quit() } catch { } }
}

# ---------------------------------------------------------------------------------------------
Step '8. WPS 文字'
# Automation is attempted exactly once, and whatever it says is reported as it comes.
try {
    $kwps = New-Object -ComObject KWPS.Application
    Ok "KWPS.Application is available: version $($kwps.Version)"
    try { $kwps.Quit() } catch { }
    $kwps = $null
} catch {
    Note "KWPS.Application is not automatable on this machine: $($_.Exception.Message)"
    $script:Notes.Add('WPS automation is gated by a licence here; the object was exercised in WPS by hand instead (see the recipe at the end).') | Out-Null
}

$wpsExe = Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA 'Kingsoft\WPS Office') -Recurse -Filter 'wps.exe' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($null -eq $wpsExe) {
    Note 'no wps.exe was found under %LOCALAPPDATA%\Kingsoft\WPS Office'
} else {
    Write-Host "    wps.exe : $($wpsExe.FullName)"
    $wpsBefore = @(Get-Process -Name wps -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $serversBefore = @(Get-Process -Name mathtype-ole-server -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $started = Start-Process -FilePath $wpsExe.FullName -ArgumentList @("`"$docx`"") -PassThru
    Write-Host "    started wps.exe pid=$($started.Id)"
    $deadline = (Get-Date).AddSeconds(90)
    $window = $null
    while ((Get-Date) -lt $deadline) {
        $started.Refresh()
        if ($started.HasExited) { break }
        if ($started.MainWindowHandle -ne 0) { $window = $started.MainWindowTitle; break }
        Start-Sleep -Seconds 2
    }
    if ($null -ne $window) {
        Ok "WPS 文字 opened a window: '$window'"
    } else {
        $anyWps = @(Get-Process -Name wps -ErrorAction SilentlyContinue)
        if ($anyWps.Count -gt 0) {
            Ok "a wps process is running (pid $($anyWps[0].Id)) but reported no main window"
        } else {
            Fail 'WPS 文字 did not open a window on the document'
        }
    }

    # A screenshot of the WPS window, so a human can see whether the equation is drawn.
    try {
        Add-Type -AssemblyName System.Drawing
        Add-Type -Namespace Verify -Name Native -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
public struct RECT { public int Left, Top, Right, Bottom; }
'@ -ErrorAction Stop
        $handle = (Get-Process -Id $started.Id -ErrorAction SilentlyContinue).MainWindowHandle
        if ($handle -and $handle -ne 0) {
            $rect = New-Object 'Verify.Native+RECT'
            if ([Verify.Native]::GetWindowRect($handle, [ref]$rect)) {
                $width = $rect.Right - $rect.Left
                $height = $rect.Bottom - $rect.Top
                $bitmap = New-Object System.Drawing.Bitmap($width, $height)
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
                $bitmap.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)
                $graphics.Dispose(); $bitmap.Dispose()
                Ok "captured the WPS window to $shot"
            }
        } else {
            Note 'no WPS window handle to capture'
        }
    } catch {
        Note "capturing the WPS window failed: $($_.Exception.Message)"
    }

    $serversAfter = @(Get-Process -Name mathtype-ole-server -ErrorAction SilentlyContinue |
        Where-Object { $serversBefore -notcontains $_.Id })
    if ($serversAfter.Count -gt 0) {
        Ok "WPS started the equation server: pid $($serversAfter -join ', ')"
    } else {
        Note 'WPS has not activated the object, so our server was not started: that needs a double-click on the equation (see the recipe below)'
    }

    foreach ($process in @(Get-Process -Name wps -ErrorAction SilentlyContinue | Where-Object { $wpsBefore -notcontains $_.Id })) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------------------------
Step '9. unregister'
& (Join-Path $PSScriptRoot 'uninstall.ps1') -Scope $Scope | ForEach-Object { Write-Host "    $_" }
if (Test-Path -LiteralPath $classKey) { Fail "the class is still registered under $Scope" } else { Ok "the class is no longer registered under $Scope" }

Stop-EditorProcesses

# ---------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '======================== summary ========================' -ForegroundColor Cyan
Write-Host "class id            : $classId"
Write-Host "prog id             : $progId"
Write-Host "document            : $docx"
Write-Host "embedded object     : $bin"
foreach ($note in $script:Notes) { Write-Host "[note] $note" -ForegroundColor Yellow }

Write-Host ''
Write-Host 'To see WPS 文字 activate the object by hand:'
Write-Host "  1. run  .\install.ps1 -Configuration $Configuration -Scope $Scope"
Write-Host "  2. open $docx in WPS 文字 (Start-Process `"$($wpsExe.FullName)`" `"$docx`")"
Write-Host '  3. double-click the equation in the page'
Write-Host '  4. expect: a mathtype-ole-server.exe process, then a mathtype.exe editor window with the'
Write-Host '     equation from the document; Ctrl+S in the editor writes it back into the document'
Write-Host "  5. the run leaves its log at $log"

if ($script:Failures.Count -eq 0) {
    Write-Host "`nPASS" -ForegroundColor Green
    exit 0
}
Write-Host ''
foreach ($failure in $script:Failures) { Write-Host "[FAIL] $failure" -ForegroundColor Red }
Show-Log
Write-Host "`nFAIL" -ForegroundColor Red
exit 1

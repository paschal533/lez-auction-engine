<#
.SYNOPSIS
  Records the whole demo story in Basecamp to an mp4 (Plan 4 Task 18).

.DESCRIPTION
  record-story.ps1 -Out C:\Users\user\Videos\rfp014-demo\basecamp.mp4

  1. Runs scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/story.mjs in WSL, with
     STORY_PACE_MS=1500 (story.mjs waits that long after each step so viewers can read).
  2. When the WSLg Basecamp window appears (title "... Logos Basecamp (<distro>)"; WSLg may prefix it, e.g.
     "[WARN:COPY MODE]"), moves it to a capture screen (the first non-primary screen if there is one), makes it
     topmost so no terminal can cover it, and starts Windows ffmpeg gdigrab on that window's screen region.
     gdigrab's title= capture of a WSLg (msrdc RAIL) window records black frames, so the region is used.
  3. Stops ffmpeg when the story ends, then runs cut-idle.py: every tx-lane wait longer than 3 s is cut to its
     first and last 1.5 s, the capture before the first step and after the last is trimmed, and the title cards
     in title-cards.txt are drawn at their steps' starts. The raw capture is deleted once the output checks out.

  Whatever happens (a failure, a throw, Ctrl-C), the script stops ffmpeg, stops the run (run-app.sh, Basecamp,
  the sequencer) and takes the window off topmost. It refuses to start below -MinFreeGB free on C: and stops
  everything if C: drops below -FloorGB. Extra WSL environment for run-app.sh (e.g. AUCTIONS_ROOT=...
  AUCTION_ENGINE_ROOT=... LOGOS_QT_MCP=... when the Nix caches are unreachable) goes in -WslEnv.

  Exit codes: 0 ok; 2 a tool is missing; 3 low disk at start; 4 aborted (disk floor, no window, ffmpeg died);
  5 the story failed; 6 nothing captured or no capture start time; 7 cut-idle.py failed; 8 the output did not
  check out. The raw capture is kept on every failure after capture started.
#>
param(
  [Parameter(Mandatory = $true)][string]$Out,
  [int]$PaceMs = 1500,
  [double]$Threshold = 3.0,
  [double]$Keep = 1.5,
  [double]$MinFreeGB = 12,
  [double]$FloorGB = 10,
  [string]$Distro = 'Ubuntu-20.04',
  [string]$WslEnv = '',
  [string]$Ffmpeg = 'ffmpeg',
  [string]$Python = 'python',
  [int]$Fps = 30,
  [int]$WindowWaitS = 1800,
  [switch]$KeepRaw
)
$ErrorActionPreference = 'Stop'

Add-Type @'
using System; using System.Runtime.InteropServices;
public static class RecWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
'@
# Per-monitor DPI aware: window rects and screen bounds in physical pixels, as gdigrab captures them.
[RecWin]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
Add-Type -AssemblyName System.Windows.Forms

# Writes the message to stderr and exits with the code (Write-Error would throw first under 'Stop').
function Fail([int]$code, [string]$msg) { [Console]::Error.WriteLine("record-story: $msg"); exit $code }
function Resolve-Tool([string]$name, [string]$fallback) {
  $c = Get-Command $name -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  if ($fallback -and (Test-Path $fallback)) { return $fallback }
  Fail 2 "$name not found"
}
function FreeGB { (Get-PSDrive C).Free / 1GB }
function To-Wsl([string]$p) {
  $full = [System.IO.Path]::GetFullPath($p)
  '/mnt/' + $full.Substring(0, 1).ToLower() + ($full.Substring(2) -replace '\\', '/')
}
function NowMs { [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }
# Runs a native command without PowerShell turning its stderr into a terminating error.
function Invoke-Native([scriptblock]$cmd) {
  $prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try { & $cmd 2>$null } finally { $ErrorActionPreference = $prev }
}
# WSL clock minus Windows clock (ms): the best of three round trips.
function Measure-Skew {
  $best = $null
  for ($i = 0; $i -lt 3; $i++) {
    $t1 = NowMs
    $w = [double](Invoke-Native { wsl.exe -d $Distro -- date '+%s%3N' })
    $t2 = NowMs
    if ($null -eq $best -or ($t2 - $t1) -lt $best.rtt) { $best = @{ rtt = $t2 - $t1; skew = $w - ($t1 + $t2) / 2 } }
  }
  $best
}
function Find-BasecampWindow {
  Get-Process msrdc -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowTitle -like "*Logos Basecamp ($Distro)*" } | Select-Object -First 1
}
# Asks ffmpeg to finish the file ('q'), kills it after 20 s.
function Stop-Ffmpeg($p) {
  if ($null -eq $p -or $p.HasExited) { return }
  try { $p.StandardInput.Write('q'); $p.StandardInput.Flush() } catch { }
  if (-not $p.WaitForExit(20000)) { try { $p.Kill() } catch { } }
}
# TERM to this run's run-app.sh (its exit trap stops Basecamp and the sequencer); after 60 s, kill what is left
# of its process tree (Basecamp and its module hosts are its children) and its sequencer. Only the pid the run
# recorded is signalled: other Basecamp sessions in the distro are left alone.
function Stop-Run($p) {
  if ($null -eq $p -or $p.HasExited) { return }
  Write-Warning 'record-story: stopping the run'
  $runPid = (Get-Content -ErrorAction SilentlyContinue $runPidFile | Select-Object -First 1)
  if ("$runPid" -match '^\d+$') {
    Invoke-Native { wsl.exe -d $Distro -- kill -TERM $runPid } | Out-Null
  }
  if ($p.WaitForExit(60000)) { return }
  try { $p.Kill() } catch { }
  $stopSh = Join-Path $env:TEMP "record-story-stop-$PID.sh"
  @"
cd '$(To-Wsl $repo)'
pid="`$(cat '$(To-Wsl $runPidFile)' 2>/dev/null || true)"
tree() { local c; for c in `$(pgrep -P "`$1"); do tree "`$c"; done; echo "`$1"; }
if [[ "`$pid" =~ ^[0-9]+`$ ]] && kill -0 "`$pid" 2>/dev/null; then
  pids="`$(tree "`$pid")"
  kill -TERM `$pids 2>/dev/null || true; sleep 2; kill -9 `$pids 2>/dev/null || true
fi
(. scripts/lib/localnet.sh && ln_stop_sequencer) || true
"@ -replace "`r", '' | Set-Content -NoNewline -Encoding ascii $stopSh
  Invoke-Native { wsl.exe -d $Distro -- bash -l (To-Wsl $stopSh) } | Out-Null
  Remove-Item -ErrorAction SilentlyContinue $stopSh
}

$ffmpegExe = Resolve-Tool $Ffmpeg 'C:\Users\user\AppData\Local\Microsoft\WinGet\Links\ffmpeg.exe'
$ffprobeExe = Join-Path (Split-Path $ffmpegExe) 'ffprobe.exe'
if (-not (Test-Path $ffprobeExe)) { Fail 2 "ffprobe not found next to $ffmpegExe" }
$pythonExe = Resolve-Tool $Python ''
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = (Resolve-Path (Join-Path $here '..\..')).Path
$Out = [System.IO.Path]::GetFullPath($Out)
$outDir = Split-Path $Out
New-Item -ItemType Directory -Force $outDir | Out-Null
$stem = [System.IO.Path]::GetFileNameWithoutExtension($Out)
$raw = Join-Path $outDir "$stem.raw.mp4"
$ffLog = Join-Path $outDir "$stem.ffmpeg.log"
$runLog = Join-Path $outDir "$stem.run.log"
$cutLog = Join-Path $outDir "$stem.cut.log"
$timeline = Join-Path $outDir "$stem.timeline.json"
$planFile = Join-Path $outDir "$stem.plan.json"

$free = FreeGB
Write-Host ("record-story: C: {0:N1} GB free" -f $free)
if ($free -lt $MinFreeGB) { Fail 3 ("C: has {0:N1} GB free, below {1} GB; not starting" -f $free, $MinFreeGB) }
$lowest = $free

$skew = Measure-Skew
Write-Host ("record-story: WSL clock - Windows clock = {0:N0} ms (round trip {1} ms)" -f $skew.skew, $skew.rtt)

# The run, as a script file: inline bash through wsl.exe loses quoting. It records its pid (run-app.sh's, after
# the exec) for Stop-Run.
$runSh = Join-Path $env:TEMP "record-story-run-$PID.sh"
$runPidFile = Join-Path $env:TEMP "record-story-run-$PID.pid"
Remove-Item -ErrorAction SilentlyContinue $runPidFile
@"
set -euo pipefail
echo `$`$ > '$(To-Wsl $runPidFile)'
cd '$(To-Wsl $repo)'
export STORY_PACE_MS=$PaceMs STORY_TIMELINE='$(To-Wsl $timeline)' $WslEnv
exec scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/story.mjs
"@ -replace "`r", '' | Set-Content -NoNewline -Encoding ascii $runSh
Remove-Item -ErrorAction SilentlyContinue $timeline, $raw, $planFile, $cutLog, $ffLog

$run = $null; $ff = $null; $errTask = $null; $h = [IntPtr]::Zero; $aborted = $null; $lastSeen = $null
$deadline = (Get-Date).AddSeconds($WindowWaitS)
try {
  $run = Start-Process wsl.exe -ArgumentList @('-d', $Distro, '--', 'bash', '-l', (To-Wsl $runSh)) `
    -RedirectStandardOutput $runLog -RedirectStandardError "$runLog.err" -NoNewWindow -PassThru
  # Windows PowerShell only keeps a Start-Process exit code if the handle was opened while the process ran.
  $null = $run.Handle
  Write-Host "record-story: run-app.sh started (log $runLog); waiting for the Basecamp window"

  while (-not $run.HasExited) {
    $free = FreeGB; if ($free -lt $lowest) { $lowest = $free }
    if ($free -lt $FloorGB) { $aborted = ("C: fell to {0:N1} GB" -f $free); break }
    if ($ff -and $ff.HasExited) { $aborted = "ffmpeg exited early (code $($ff.ExitCode))"; break }
    if ($null -eq $ff) {
      $win = Find-BasecampWindow
      if ($win) {
        $h = $win.MainWindowHandle
        $screens = [System.Windows.Forms.Screen]::AllScreens
        $target = ($screens | Where-Object { -not $_.Primary } | Select-Object -First 1)
        if (-not $target) { $target = $screens[0] }
        [RecWin]::ShowWindow($h, 9) | Out-Null   # restore if maximised or minimised
        # HWND_TOPMOST, keep the size (SWP_NOSIZE).
        [RecWin]::SetWindowPos($h, [IntPtr](-1), $target.Bounds.X + 20, $target.Bounds.Y + 20, 0, 0, 0x0001) | Out-Null
        Start-Sleep -Seconds 2
        $r = New-Object RecWin+RECT
        [RecWin]::GetWindowRect($h, [ref]$r) | Out-Null
        $w = ($r.R - $r.L) - (($r.R - $r.L) % 2); $hgt = ($r.B - $r.T) - (($r.B - $r.T) % 2)
        Write-Host "record-story: window '$($win.MainWindowTitle)' at $($r.L),$($r.T) ${w}x$hgt; capturing that region"
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $ffmpegExe
        $psi.Arguments = "-hide_banner -y -f gdigrab -framerate $Fps -draw_mouse 0 -offset_x $($r.L) -offset_y $($r.T) " +
          "-video_size ${w}x$hgt -i desktop -c:v libx264 -preset ultrafast -crf 28 -pix_fmt yuv420p `"$raw`""
        $psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardError = $true
        $ff = [System.Diagnostics.Process]::Start($psi)
        $errTask = $ff.StandardError.ReadToEndAsync()
      } elseif ((Get-Date) -gt $deadline) { $aborted = "no Basecamp window after $WindowWaitS s"; break }
    } elseif (Find-BasecampWindow) {
      # The last time the window was seen: the cut never keeps anything after it (run-app closes Basecamp
      # right after the story, and the region would then show whatever is behind it).
      $lastSeen = (NowMs) / 1000.0
    }
    if ($ff) { Start-Sleep -Milliseconds 500 } else { Start-Sleep -Seconds 2 }
  }
  if ($ff -and $ff.HasExited -and -not $aborted) { $aborted = "ffmpeg exited before the story ended (code $($ff.ExitCode))" }
} finally {
  # Runs on success, on any throw and on Ctrl-C.
  try { Stop-Ffmpeg $ff } catch { }
  try { if ($errTask) { Set-Content -Encoding utf8 $ffLog $errTask.Result } } catch { }
  try { if ($h -ne [IntPtr]::Zero) { [RecWin]::SetWindowPos($h, [IntPtr](-2), 0, 0, 0, 0, 0x0003) | Out-Null } } catch { }
  try { Stop-Run $run } catch { }
  Remove-Item -ErrorAction SilentlyContinue $runSh, $runPidFile
}
$run.WaitForExit()
$runStatus = $run.ExitCode
Write-Host ("record-story: run-app.sh exited {0}; C: lowest {1:N1} GB" -f $runStatus, $lowest)
if ($aborted) { Fail 4 "aborted: $aborted (raw capture, if any, kept at $raw)" }
if ($runStatus -ne 0) { Fail 5 "the story failed (exit $runStatus); see $runLog (raw capture kept at $raw)" }
if (-not $ff -or -not (Test-Path $raw)) { Fail 6 'nothing was captured' }

$m = [regex]::Match((Get-Content -Raw $ffLog), 'start: ([0-9.]+)')
if (-not $m.Success) { Fail 6 "no capture start time in $ffLog (raw capture kept at $raw)" }
$cutArgs = @('-I', "`"$(Join-Path $here 'cut-idle.py')`"", '--timeline', "`"$timeline`"",
  '--cards', "`"$(Join-Path $here 'title-cards.txt')`"", '--in', "`"$raw`"", '--out', "`"$Out`"",
  '--plan-out', "`"$planFile`"", '--capture-start', $m.Groups[1].Value, '--skew-ms', "$($skew.skew)",
  '--threshold', "$Threshold", '--keep', "$Keep", '--fps', "$Fps",
  '--ffmpeg', "`"$ffmpegExe`"", '--ffprobe', "`"$ffprobeExe`"")
if ($lastSeen) { $cutArgs += @('--window-gone', $lastSeen.ToString('F3', [Globalization.CultureInfo]::InvariantCulture)) }
$cut = Start-Process $pythonExe -ArgumentList $cutArgs -NoNewWindow -Wait -PassThru -RedirectStandardError $cutLog
Get-Content $cutLog | Write-Host
if ($cut.ExitCode -ne 0) { Fail 7 "cut-idle.py failed (exit $($cut.ExitCode)); see $cutLog (raw capture kept at $raw)" }

if (-not (Test-Path $Out) -or (Get-Item $Out).Length -eq 0) { Fail 8 "no output at $Out (raw capture kept at $raw)" }
$durText = Invoke-Native { & $ffprobeExe -v error -show_entries format=duration -of csv=p=0 $Out }
$dur = 0.0
if (-not [double]::TryParse("$durText".Trim(), [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$dur) -or $dur -le 0) {
  Fail 8 "unreadable duration '$durText' for $Out (raw capture kept at $raw)"
}
Write-Host ("record-story: {0} ({1:N1} s, {2:N1} MB); plan {3}" -f $Out, $dur, ((Get-Item $Out).Length / 1MB), $planFile)
if (-not $KeepRaw) { Remove-Item $raw; Write-Host 'record-story: raw capture deleted' }
exit 0

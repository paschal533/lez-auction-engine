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

  Stops everything (ffmpeg, the run, Basecamp, the localnet) if C: drops below -FloorGB, and refuses to start
  below -MinFreeGB. Extra WSL environment for run-app.sh (e.g. AUCTIONS_ROOT=... AUCTION_ENGINE_ROOT=...
  LOGOS_QT_MCP=... when the Nix caches are unreachable) goes in -WslEnv.
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

function Resolve-Tool([string]$name, [string]$fallback) {
  $c = Get-Command $name -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  if (Test-Path $fallback) { return $fallback }
  throw "record-story: $name not found"
}
function FreeGB { (Get-PSDrive C).Free / 1GB }
function To-Wsl([string]$p) {
  $full = [System.IO.Path]::GetFullPath($p)
  '/mnt/' + $full.Substring(0, 1).ToLower() + ($full.Substring(2) -replace '\\', '/')
}
function NowMs { [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }
# WSL clock minus Windows clock (ms): the best of three round trips.
function Measure-Skew {
  $best = $null
  for ($i = 0; $i -lt 3; $i++) {
    $t1 = NowMs
    $w = [double](& wsl.exe -d $Distro -- date '+%s%3N')
    $t2 = NowMs
    if ($null -eq $best -or ($t2 - $t1) -lt $best.rtt) { $best = @{ rtt = $t2 - $t1; skew = $w - ($t1 + $t2) / 2 } }
  }
  $best
}
function Find-BasecampWindow {
  Get-Process msrdc -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowTitle -like "*Logos Basecamp ($Distro)*" } | Select-Object -First 1
}
function Stop-Ffmpeg($p) {
  if ($null -eq $p -or $p.HasExited) { return }
  try { $p.StandardInput.Write('q'); $p.StandardInput.Flush() } catch { }
  if (-not $p.WaitForExit(20000)) { $p.Kill() }
}

$ffmpegExe = Resolve-Tool $Ffmpeg 'C:\Users\user\AppData\Local\Microsoft\WinGet\Links\ffmpeg.exe'
$ffprobeExe = Join-Path (Split-Path $ffmpegExe) 'ffprobe.exe'
$pythonExe = Resolve-Tool $Python 'python3'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = (Resolve-Path (Join-Path $here '..\..')).Path
$Out = [System.IO.Path]::GetFullPath($Out)
$outDir = Split-Path $Out
New-Item -ItemType Directory -Force $outDir | Out-Null
$stem = [System.IO.Path]::GetFileNameWithoutExtension($Out)
$raw = Join-Path $outDir "$stem.raw.mp4"
$ffLog = Join-Path $outDir "$stem.ffmpeg.log"
$runLog = Join-Path $outDir "$stem.run.log"
$timeline = Join-Path $outDir "$stem.timeline.json"
$planFile = Join-Path $outDir "$stem.plan.json"

$free = FreeGB
Write-Host ("record-story: C: {0:N1} GB free" -f $free)
if ($free -lt $MinFreeGB) { Write-Error ("record-story: C: has {0:N1} GB free, below {1} GB; not starting" -f $free, $MinFreeGB); exit 3 }
$lowest = $free

$skew = Measure-Skew
Write-Host ("record-story: WSL clock - Windows clock = {0:N0} ms (round trip {1} ms)" -f $skew.skew, $skew.rtt)

# The run, as a script file: inline bash through wsl.exe loses quoting.
$runSh = Join-Path $env:TEMP "record-story-run-$PID.sh"
@"
set -euo pipefail
cd '$(To-Wsl $repo)'
export STORY_PACE_MS=$PaceMs STORY_TIMELINE='$(To-Wsl $timeline)' $WslEnv
exec scripts/basecamp/run-app.sh --setup --test apps/auctions/tests/story.mjs
"@ -replace "`r", '' | Set-Content -NoNewline -Encoding ascii $runSh
Remove-Item -ErrorAction SilentlyContinue $timeline, $raw
$run = Start-Process wsl.exe -ArgumentList @('-d', $Distro, '--', 'bash', '-l', (To-Wsl $runSh)) `
  -RedirectStandardOutput $runLog -RedirectStandardError "$runLog.err" -NoNewWindow -PassThru
# Windows PowerShell only keeps a Start-Process exit code if the handle was opened while the process ran.
$null = $run.Handle
Write-Host "record-story: run-app.sh started (log $runLog); waiting for the Basecamp window"

$ff = $null; $captureStart = $null; $aborted = $null; $deadline = (Get-Date).AddSeconds($WindowWaitS)
try {
  while (-not $run.HasExited) {
    $free = FreeGB; if ($free -lt $lowest) { $lowest = $free }
    if ($free -lt $FloorGB) { $aborted = ("C: fell to {0:N1} GB" -f $free); break }
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
    }
    Start-Sleep -Seconds 2
  }
} finally {
  Stop-Ffmpeg $ff
  if ($ff) { Set-Content -Encoding utf8 $ffLog $errTask.Result }
  if ($aborted -and -not $run.HasExited) {
    Write-Warning "record-story: $aborted; stopping the run"
    # run-app.sh's exit trap stops Basecamp and the sequencer on TERM.
    & wsl.exe -d $Distro -- bash -lc "pkill -TERM -f 'scripts/basecamp/run-app.sh' || true"
    if (-not $run.WaitForExit(60000)) { $run.Kill() }
  }
  Remove-Item -ErrorAction SilentlyContinue $runSh
}
$run.WaitForExit()
$runStatus = $run.ExitCode
Write-Host ("record-story: run-app.sh exited {0}; C: lowest {1:N1} GB" -f $runStatus, $lowest)
if ($aborted) { Write-Error "record-story: aborted: $aborted (raw capture kept at $raw)"; exit 4 }
if ($runStatus -ne 0) { Write-Error "record-story: the story failed; see $runLog (raw capture kept at $raw)"; exit 5 }
if (-not $ff) { Write-Error 'record-story: nothing was captured'; exit 6 }

$m = [regex]::Match((Get-Content -Raw $ffLog), 'start: ([0-9.]+)')
if (-not $m.Success) { Write-Error "record-story: no capture start time in $ffLog"; exit 6 }
$captureStart = $m.Groups[1].Value
& $pythonExe -I (Join-Path $here 'cut-idle.py') --timeline $timeline --cards (Join-Path $here 'title-cards.txt') `
  --in $raw --out $Out --capture-start $captureStart --skew-ms $skew.skew --threshold $Threshold --keep $Keep `
  --fps $Fps --ffmpeg $ffmpegExe --ffprobe $ffprobeExe | Set-Content -Encoding utf8 $planFile
if ($LASTEXITCODE -ne 0) { Write-Error "record-story: cut-idle.py failed (raw capture kept at $raw)"; exit 7 }
$dur = [double](& $ffprobeExe -v error -show_entries format=duration -of csv=p=0 $Out)
Write-Host ("record-story: {0} ({1:N1} s, {2:N1} MB); plan {3}" -f $Out, $dur, ((Get-Item $Out).Length / 1MB), $planFile)
if ($dur -gt 0 -and -not $KeepRaw) { Remove-Item $raw; Write-Host 'record-story: raw capture deleted' }

# Plays a scripted lap into fake Assetto Corsa shared memory so the session recorder and the Sessions
# page can be tested without the game. The mappings are named acxstest_* (set ACXS_SHM_PREFIX=acxstest_
# for the reader). The car follows the real racing line of a track (ai\fast_lane.ai) and causes
# problems at known lap positions:
#   12%  jump          18-22% understeer   30%  front lock-up   45%  crash
#   60-62% off track   80-84% oversteer    90%  wheelspin
# Usage: fake-ac-drive.ps1 [-Seconds 40] [-TrackDir <folder with ai\fast_lane.ai>] [-TrackId ks_nordschleife] [-Layout nordschleife]
param(
    [int]$Seconds = 40,
    [string]$TrackDir = "C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\content\tracks\ks_nordschleife\nordschleife",
    [string]$TrackId = "ks_nordschleife",
    [string]$Layout = "nordschleife"
)
Add-Type -AssemblyName System.Core

# Racing line: header (version, count, 0, 0) then 20-byte points: x, y, z, distance, id.
$bytes = [System.IO.File]::ReadAllBytes((Join-Path $TrackDir "ai\fast_lane.ai"))
$count = [BitConverter]::ToInt32($bytes, 4)
$step = [math]::Max(1, [int]($count / 600))
$line = New-Object System.Collections.Generic.List[object]
for ($i = 0; $i -lt $count; $i += $step) {
    $o = 16 + 20 * $i
    $line.Add(@([BitConverter]::ToSingle($bytes, $o), [BitConverter]::ToSingle($bytes, $o + 8)))
}
$n = $line.Count

$phys = [System.IO.MemoryMappedFiles.MemoryMappedFile]::CreateNew("acxstest_physics", 580)
$gfx  = [System.IO.MemoryMappedFiles.MemoryMappedFile]::CreateNew("acxstest_graphics", 288)
$stat = [System.IO.MemoryMappedFiles.MemoryMappedFile]::CreateNew("acxstest_static", 684)
$pa = $phys.CreateViewAccessor(); $ga = $gfx.CreateViewAccessor(); $sa = $stat.CreateViewAccessor()
function WStr($acc, [int]$at, [string]$s) { $i = 0; foreach ($c in $s.ToCharArray()) { $acc.Write($at + 2 * $i, [int16][char]$c); $i++ } }
WStr $sa 68 "ks_bmw_m3_e92"; WStr $sa 134 $TrackId; WStr $sa 524 $Layout; WStr $sa 604 "fake_skin"
foreach ($i in 0..3) { $sa.Write(436 + 4 * $i, [single]0.3) }

$start = Get-Date
$lapSeconds = $Seconds - 2.0
$crashStart = -1.0
$gain = 0.1
while (((Get-Date) - $start).TotalSeconds -lt $Seconds) {
    $t = ((Get-Date) - $start).TotalSeconds
    $p = [math]::Min(0.999, $t / $lapSeconds)
    $speed = 80.0; $steer = 0.1 + 0.2 * [math]::Abs([math]::Sin($t * 2)); $lat = 0.1; $yawScale = 1.0
    $brake = 0.0; $throttle = 0.5; $load = 3000.0; $out = 0; $frontSlow = $false; $rearSpin = $false; $g0 = $lat
    if ($p -ge 0.119 -and $p -lt 0.129) { $load = 0.0; $speed = 150.0 }
    if ($p -ge 0.18 -and $p -lt 0.22) { $speed = 110.0; $steer = 0.3; $lat = 1.2; $yawScale = 0.6 }
    if ($p -ge 0.30 -and $p -lt 0.305) { $speed = 140.0; $brake = 1.0; $frontSlow = $true }
    if ($p -ge 0.45 -and $crashStart -lt 0) { $crashStart = $t }
    if ($crashStart -ge 0) {
        $c = $t - $crashStart
        if ($c -lt 0.3) { $speed = 150.0 - 130.0 * ($c / 0.3); $g0 = 6.0 } elseif ($c -lt 1.3) { $speed = 20.0 }
    }
    if ($p -ge 0.60 -and $p -lt 0.62) { $out = 4 }
    if ($p -ge 0.80 -and $p -lt 0.84) { $speed = 100.0; $steer = 0.3; $lat = 1.2; $yawScale = 1.6 }
    if ($p -ge 0.90 -and $p -lt 0.905) { $speed = 60.0; $throttle = 1.0; $rearSpin = $true }
    if ($g0 -lt $lat) { $g0 = $lat }
    if ($lat -gt 1.0 -and $g0 -lt $lat) { $g0 = $lat }
    $v = $speed / 3.6
    $roll = $v / 0.3
    $pa.Write(4, [single]$throttle); $pa.Write(8, [single]$brake)
    $pa.Write(24, [single]$steer); $pa.Write(28, [single]$speed)
    $pa.Write(44, [single]$g0)
    foreach ($i in 0..3) {
        $w = $roll
        if ($frontSlow -and $i -lt 2) { $w = 30.0 }
        if ($rearSpin -and $i -ge 2) { $w = $roll * 1.7 }
        $pa.Write(72 + 4 * $i, [single]$load)
        $pa.Write(104 + 4 * $i, [single]$w)
    }
    $pa.Write(244, [int]$out)
    $pa.Write(300, [single]($gain * $steer * $v * $yawScale))
    $ga.Write(4, [int]2)
    $ga.Write(132, [int]0)
    $idx = [math]::Min($n - 1, [int]($p * $n))
    $ga.Write(248, [single]$p)
    $ga.Write(252, [single]$line[$idx][0])
    $ga.Write(260, [single]$line[$idx][1])
    Start-Sleep -Milliseconds 25
}
$ga.Write(4, [int]0)
Start-Sleep -Milliseconds 500
$pa.Dispose(); $ga.Dispose(); $sa.Dispose(); $phys.Dispose(); $gfx.Dispose(); $stat.Dispose()


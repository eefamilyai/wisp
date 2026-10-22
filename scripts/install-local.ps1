# Installs the freshly built binaries over the installed copy and restarts the
# assistant. Run this after `cargo build --release` — otherwise the copy you
# launch from the Start menu is whatever the installer shipped, which is how a
# day of fixes ended up sitting in target\release unattended.
#
#   powershell -ExecutionPolicy Bypass -File scripts\install-local.ps1
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\NeedleVoice",
    [switch]$NoRestart
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$release = Join-Path $root "target\release"

if (-not (Test-Path (Join-Path $InstallDir "NeedleVoice.exe"))) {
    throw "No installed copy at $InstallDir — run the installer first."
}
foreach ($exe in @("NeedleVoice.exe", "NeedleVoiceConfig.exe")) {
    if (-not (Test-Path (Join-Path $release $exe))) {
        throw "$exe is not built yet — run: cargo build --release"
    }
}

# The assistant holds its own executable open while running.
Get-Process NeedleVoice, NeedleVoiceConfig -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$backup = Join-Path $root "target\installed-backup\$stamp"
New-Item -ItemType Directory -Force -Path $backup | Out-Null
foreach ($exe in @("NeedleVoice.exe", "NeedleVoiceConfig.exe")) {
    Copy-Item (Join-Path $InstallDir $exe) (Join-Path $backup $exe) -Force
}

foreach ($exe in @("NeedleVoice.exe", "NeedleVoiceConfig.exe")) {
    Copy-Item (Join-Path $release $exe) (Join-Path $InstallDir $exe) -Force
    Write-Host "installed $exe"
}
# Runtime libraries, in case they changed too.
Get-ChildItem (Join-Path $release "*.dll") -ErrorAction SilentlyContinue | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $InstallDir $_.Name) -Force
}

# Recogniser models the installed copy does not ship. Every engine is copied, not
# just the default one: selecting an engine whose files are missing leaves the
# assistant unable to hear anything, and it retried the failed load on every
# caption pass, which is exactly the "I changed the model and now it disengages".
$devModels = Join-Path $root "models"
$instModels = Join-Path $InstallDir "models"
# `streaming` holds several packs for measurement; only the caption's own
# model is installed.
foreach ($pack in @("moonshine", "sensevoice", "dolphin")) {
    $from = Join-Path $devModels $pack
    if (Test-Path $from) {
        Copy-Item $from (Join-Path $instModels $pack) -Recurse -Force
        Write-Host "installed the $pack models"
    }
}
# The streaming model the live transcript uses.
$streamSrc = Join-Path $devModels "streaming\sherpa-onnx-nemo-streaming-fast-conformer-ctc-en-80ms-int8"
if (Test-Path $streamSrc) {
    $streamDst = Join-Path $instModels "streaming\sherpa-onnx-nemo-streaming-fast-conformer-ctc-en-80ms-int8"
    New-Item -ItemType Directory -Force -Path $streamDst | Out-Null
    Copy-Item "$streamSrc\*" $streamDst -Force
    Write-Host "installed the streaming model (live transcript)"
}

$cfgFile = Join-Path $env:APPDATA "NeedleVoice\config.toml"

# The model behind the wake word and the command brain: without needle3.cact the
# agent answers every command with a web search, which is what "it doesn't
# understand anything" looked like.
foreach ($must in @("needle3.cact")) {
    $src = Join-Path $devModels $must
    $dst = Join-Path $instModels $must
    if ((Test-Path $src) -and -not (Test-Path $dst)) {
        Copy-Item $src $dst -Force
        Write-Host "installed $must"
    }
}

# The voice pack the config names. A voice pack that is missing is not an error
# the agent can show — it falls back to the Windows voice and the user hears a
# different assistant — so install it before the agent starts.
if (Test-Path $cfgFile) {
    $engine = (Select-String -Path $cfgFile -Pattern '^\s*voice_engine\s*=\s*"([^"]+)"').Matches.Groups[1].Value
    $kokoro = (Select-String -Path $cfgFile -Pattern '^\s*kokoro_voice\s*=').Matches.Count -gt 0
    if ($engine -eq "kokoro" -and $kokoro) {
        $pack = "kokoro-multi-lang-v1_0"
        $from = Join-Path $devModels "voices\$pack"
        $to = Join-Path $instModels "voices\$pack"
        if ((Test-Path $from) -and -not (Test-Path (Join-Path $to "voices.bin"))) {
            Copy-Item $from $to -Recurse -Force
            Write-Host "installed the $pack voices"
        }
    }
}

# Any Whisper model the config asks for that the install does not have.
if (Test-Path $cfgFile) {
    $wanted = (Select-String -Path $cfgFile -Pattern '^\s*whisper_model\s*=\s*"([^"]+)"').Matches.Groups[1].Value
    if ($wanted) {
        $src = Join-Path $devModels $wanted
        $dst = Join-Path $instModels $wanted
        if ((Test-Path $src) -and -not (Test-Path $dst)) {
            Copy-Item $src $dst -Force
            Write-Host "installed the Whisper model $wanted"
        }
    }
}

if (-not $NoRestart) {
    Start-Process -FilePath (Join-Path $InstallDir "NeedleVoice.exe") -ArgumentList "--background" -WorkingDirectory $InstallDir
    Start-Sleep -Seconds 6
    $log = Join-Path $env:APPDATA "NeedleVoice\agent.log"
    if (Test-Path $log) {
        Write-Host "--- last lines of the log"
        Get-Content $log -Tail 6 | Where-Object { $_ -notmatch "^whisper_|wake keywords|▁" }
    }
}

# Confirm the installed build is the one that was just built.
$fresh = (Get-FileHash (Join-Path $release "NeedleVoice.exe") -Algorithm SHA256).Hash
$placed = (Get-FileHash (Join-Path $InstallDir "NeedleVoice.exe") -Algorithm SHA256).Hash
if ($fresh -ne $placed) { throw "The installed copy does not match the build." }
Write-Host "installed build matches target\release ($($fresh.Substring(0,12))…)"

# Builds Wisp.exe, WispSettings.exe and WispUninstall.exe,
# then packs them into dist\WispSetup.exe.
#
#   -Sign   Authenticode-sign every binary in the payload and the installer,
#           using the certificate in $env:WISP_SIGN_CERT (a .pfx) and its password
#           in $env:WISP_SIGN_PASSWORD.
#
# Signing is all-or-nothing on purpose: signing Setup while leaving the binaries
# inside it unsigned can make Defender's opinion of the package worse, not
# better. See docs\installer.md.
param(
    [switch]$Sign,
    [switch]$NoModels
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    if (-not $NoModels) { & "$PSScriptRoot\fetch-models.ps1" }

    # The uninstaller is built before the installer, because the installer's
    # build script packs it into the payload. It is a program of its own and
    # deliberately small: see docs\installer.md.
    cargo build --release -p wisp-uninstall
    if ($LASTEXITCODE) { throw "uninstaller build failed" }

    cargo build --release -p wisp-agent -p wisp-config
    if ($LASTEXITCODE) { throw "app build failed" }

    cargo build --release -p wisp-setup
    if ($LASTEXITCODE) { throw "installer build failed" }

    $payload = @(
        "Wisp.exe",
        "WispSettings.exe",
        "WispUninstall.exe",
        "onnxruntime.dll",
        "onnxruntime_providers_shared.dll",
        "sherpa-onnx-c-api.dll",
        "sherpa-onnx-cxx-api.dll"
    ) | ForEach-Object { Join-Path "target\release" $_ }

    $setup = "target\release\WispSetup.exe"
    foreach ($f in $payload + $setup) {
        if (-not (Test-Path $f)) { throw "missing $f" }
    }

    if ($Sign) {
        & "$PSScriptRoot\sign.ps1" -Files ($payload + $setup)
    } else {
        # Say it out loud rather than leaving it to be discovered: an unsigned
        # build is the one most likely to be flagged. See docs\installer.md.
        Write-Warning "Building unsigned. A code-signing certificate is the single most effective remedy for the Defender false positive on this installer; pass -Sign with WISP_SIGN_CERT/WISP_SIGN_PASSWORD set."
    }

    New-Item -ItemType Directory -Force -Path dist | Out-Null
    Copy-Item $setup dist\ -Force

    $mb = [math]::Round((Get-Item dist\WispSetup.exe).Length / 1MB)
    $uninstKb = [math]::Round((Get-Item target\release\WispUninstall.exe).Length / 1KB)
    $sig = (Get-AuthenticodeSignature dist\WispSetup.exe).Status
    Write-Host "Built dist\WispSetup.exe ($mb MB, signature: $sig)"
    Write-Host "  uninstaller: $uninstKb KB, no embedded payload"
} finally { Pop-Location }

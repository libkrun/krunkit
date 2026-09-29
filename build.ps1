param (
    [ValidateSet("all", "debug", "install", "clean")]
    [string]$Task = "all",
    
    [string]$Prefix = $env:PREFIX,

    [string]$LibkrunSource = $env:LIBKRUN_SOURCE_DIR
)

$ErrorActionPreference = "Stop"
$BinName      = "krunkit.exe"
$ReleasePath  = "target\release\$BinName"
$DebugPath    = "target\debug\$BinName"

function Build-Krunkit([string]$Profile) {
    if ([string]::IsNullOrWhiteSpace($LibkrunSource)) {
        throw "Specify -LibkrunSource or set LIBKRUN_SOURCE_DIR"
    }

    $ReleaseArg = @()
    if ($Profile -eq "release") {
        $ReleaseArg += "--release"
    }
    cargo build --manifest-path (Join-Path $LibkrunSource "Cargo.toml") -p libkrun --features "ffi blk net" @ReleaseArg
    if ($LASTEXITCODE -ne 0) { throw "libkrun build failed" }

    $env:LIBKRUN_LIB_DIR = Join-Path $LibkrunSource "target\$Profile"
    cargo build @ReleaseArg
    if ($LASTEXITCODE -ne 0) { throw "krunkit build failed" }

    Copy-Item -Path (Join-Path $env:LIBKRUN_LIB_DIR "krun.dll") -Destination "target\$Profile\krun.dll" -Force
}

switch ($Task) {
    "all" {
        Build-Krunkit "release"
    }
    
    "debug" {
        Build-Krunkit "debug"
    }
    
    "install" {
        if ([string]::IsNullOrWhiteSpace($Prefix)) {
            throw "Specify -Prefix or set PREFIX"
        }

        if (-not (Test-Path $ReleasePath)) {
            Build-Krunkit "release"
        }
        
        $TargetBinDir = Join-Path $Prefix "bin"
        
        if (-not (Test-Path $TargetBinDir)) {
            New-Item -ItemType Directory -Path $TargetBinDir -Force | Out-Null
        }
        
        Copy-Item -Path $ReleasePath -Destination $TargetBinDir -Force
        Copy-Item -Path "target\release\krun.dll" -Destination $TargetBinDir -Force
        Write-Host "Successfully installed to: $TargetBinDir" -ForegroundColor Green
    }
    
    "clean" {
        cargo clean
    }
}

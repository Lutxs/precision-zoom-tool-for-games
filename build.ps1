# Builds a release executable into .\dist (tests and lints first unless -Fast).
param([switch]$Fast)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Rust is not installed. Get it from https://rustup.rs (MSVC toolchain).'
}
if (-not $Fast) {
    cargo fmt --check;                        if ($LASTEXITCODE) { throw 'cargo fmt --check failed' }
    cargo clippy --all-targets -- -D warnings; if ($LASTEXITCODE) { throw 'clippy failed' }
    cargo test;                               if ($LASTEXITCODE) { throw 'tests failed' }
}
cargo build --release; if ($LASTEXITCODE) { throw 'build failed' }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\precision-zoom.exe dist\ -Force
Write-Host "Built $(Resolve-Path dist\precision-zoom.exe)"

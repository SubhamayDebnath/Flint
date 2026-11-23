<#
  Builds everything in dist\ :
    Flint-<ver>-x64-per-user.msi      (no admin needed)
    Flint-<ver>-x64-per-machine.msi   (admin once, all users)
    Flint-<ver>-x64-portable.zip      (no installation at all)

  One-time setup (Windows):
    rustup default stable-x86_64-pc-windows-msvc
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.Util.wixext/5.0.2
#>
param([string]$Version = "")
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

if (-not $Version) {
    $Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
}
Write-Host "Flint $Version" -ForegroundColor Cyan

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$exe = (Resolve-Path "target\release\flint.exe").Path

New-Item -ItemType Directory -Force dist | Out-Null
Remove-Item dist\* -Force -ErrorAction SilentlyContinue

foreach ($scope in @(@("perUser", "per-user"), @("perMachine", "per-machine"))) {
    $out = "dist\Flint-$Version-x64-$($scope[1]).msi"
    wix build wix\Flint.wxs -arch x64 `
        -d Scope=$($scope[0]) -d Version=$Version -d ExePath=$exe `
        -ext WixToolset.Util.wixext `
        -sice ICE38 -sice ICE64 -sice ICE91 `
        -o $out
    if ($LASTEXITCODE -ne 0) { throw "wix build failed ($($scope[0]))" }
    Write-Host "  -> $out"
}

# portable: just the exe + a flag file (portable.flag = do not auto-start with Windows)
$stage = "dist\Flint-portable"
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe "$stage\Flint.exe"
Set-Content "$stage\portable.flag" "Portable mode: Flint does not enable start-with-Windows by itself."
Copy-Item README.md, LICENSE $stage
Compress-Archive -Path "$stage\*" -DestinationPath "dist\Flint-$Version-x64-portable.zip" -Force
Remove-Item $stage -Recurse -Force
Write-Host "  -> dist\Flint-$Version-x64-portable.zip"
Get-FileHash dist\* -Algorithm SHA256 | ForEach-Object { "$($_.Hash)  $(Split-Path $_.Path -Leaf)" } | Set-Content dist\SHA256SUMS.txt

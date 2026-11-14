# YTM Hybrid Windows Packaging Script
# This script creates portable ZIP archives, automated Windows installers, and MSIX packages

param(
    [string]$Version = "0.4.2",
    [string]$OutputDir = "./dist",
    [switch]$Build
)

$ErrorActionPreference = "Stop"
$ProjectDir = Split-Path -Parent $PSScriptRoot
$BuildDir = Join-Path $ProjectDir "target\release"
$DistDir = Join-Path $ProjectDir $OutputDir

Write-Host "=== YTM Hybrid Windows Packaging ===" -ForegroundColor Cyan
Write-Host "Version: $Version" -ForegroundColor Gray

# Create dist directory
New-Item -ItemType Directory -Path $DistDir -Force | Out-Null

# Build if requested or if binary doesn't exist
if ($Build -or -not (Test-Path (Join-Path $BuildDir "ytm-hybrid.exe"))) {
    Write-Host "[1/3] Building release executable..." -ForegroundColor Yellow
    cd $ProjectDir
    cargo build --release
}

$BinaryPath = Join-Path $BuildDir "ytm-hybrid.exe"
if (-not (Test-Path $BinaryPath)) {
    Write-Host "ERROR: Binary not found at $BinaryPath" -ForegroundColor Red
    exit 1
}
Write-Host "Found binary: $BinaryPath" -ForegroundColor Green

# ============================================================================
# 1. Portable ZIP Archive & Direct Installer (Windows)
# ============================================================================
Write-Host "[2/3] Creating portable ZIP archive and automated installer..." -ForegroundColor Yellow

$ZipDist = Join-Path $DistDir "ytm-hybrid-windows-x64"
Remove-Item -Path $ZipDist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $ZipDist -Force | Out-Null

# Copy binary
Copy-Item $BinaryPath -Destination $ZipDist

# Copy icon
$IconPath = "$ProjectDir\assets\linux\icons\hicolor\256x256\apps\ytm-hybrid.png"
if (Test-Path $IconPath) {
    Copy-Item $IconPath -Destination $ZipDist
}

# Copy LICENSE
$LicensePath = "$ProjectDir\LICENSE"
if (Test-Path $LicensePath) {
    Copy-Item $LicensePath -Destination $ZipDist
}

# Create automated Windows installation script (install.ps1) inside the ZIP bundle
@"
<#
.SYNOPSIS
    YTM Hybrid Windows Installation / Update Script
#>
param(
    [switch]$Uninstall
)

\$ErrorActionPreference = "Stop"
\$AppName = "ytm-hybrid"
\$Version = "$Version"
\$InstallDir = "\$env:LOCALAPPDATA\YTMHybrid"
\$BinDir = "\$env:LOCALAPPDATA\Programs\YTMHybrid"
\$DesktopDir = "\$env:UserProfile\Desktop"

if (\$Uninstall) {
    Write-Host "Uninstalling YTM Hybrid..." -ForegroundColor Yellow
    if (Get-Process -Name "ytm-hybrid" -ErrorAction SilentlyContinue) {
        Stop-Process -Name "ytm-hybrid" -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 1
    }
    Remove-Item -Path \$BinDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Path "\$DesktopDir\YTM Hybrid.lnk" -ErrorAction SilentlyContinue
    Write-Host "✓ YTM Hybrid uninstalled successfully." -ForegroundColor Green
    exit
}

Write-Host "Installing / Updating YTM Hybrid v\$Version..." -ForegroundColor Green

# Stop running instance to avoid file locks
if (Get-Process -Name "ytm-hybrid" -ErrorAction SilentlyContinue) {
    Write-Host "Stopping running ytm-hybrid process..." -ForegroundColor Yellow
    Stop-Process -Name "ytm-hybrid" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

New-Item -ItemType Directory -Path \$BinDir -Force | Out-Null

# Clean overwrite binary & assets
Write-Host "Copying files to \$BinDir..." -ForegroundColor Yellow
Remove-Item -Path "\$BinDir\ytm-hybrid.exe" -Force -ErrorAction SilentlyContinue
Copy-Item ".\ytm-hybrid.exe" -Destination "\$BinDir\ytm-hybrid.exe" -Force

if (Test-Path ".\ytm-hybrid.png") {
    Copy-Item ".\ytm-hybrid.png" -Destination "\$BinDir\ytm-hybrid.png" -Force
}

# Create Desktop Shortcut
Write-Host "Creating desktop shortcut..." -ForegroundColor Yellow
\$WScriptShell = New-Object -ComObject WScript.Shell
\$Shortcut = \$WScriptShell.CreateShortcut("\$DesktopDir\YTM Hybrid.lnk")
\$Shortcut.TargetPath = "\$BinDir\ytm-hybrid.exe"
if (Test-Path "\$BinDir\ytm-hybrid.png") {
    \$Shortcut.IconLocation = "\$BinDir\ytm-hybrid.png"
}
\$Shortcut.Save()

# Add to User PATH if not present
\$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (\$UserPath -notlike "*\$BinDir*") {
    Write-Host "Adding \$BinDir to user PATH..." -ForegroundColor Yellow
    [Environment]::SetEnvironmentVariable("Path", "\$UserPath;\$BinDir", "User")
    \$env:Path += ";\$BinDir"
}

Write-Host ""
Write-Host "✓ Installation / Update to v\$Version complete!" -ForegroundColor Green
Write-Host "You can launch YTM Hybrid from your Desktop or by typing 'ytm-hybrid' in terminal." -ForegroundColor Cyan
"@ | Set-Content -Path (Join-Path $ZipDist "install.ps1") -Encoding UTF8

# Create README
@"
# YTM Hybrid v$Version

Native YouTube Music Desktop Client built with Rust & GPUI.

## Installation

### Windows (Automated Installer)
1. Extract the ZIP file
2. Right-click on `install.ps1` and select **Run with PowerShell**

## Features
- Native YouTube Music client
- System tray integration
- Media key support
- Notifications
- Lyrics display

## License
GPL-3.0-or-later
"@ | Set-Content -Path (Join-Path $ZipDist "README.md") -Encoding UTF8

# Create ZIP
$ZipPath = Join-Path $DistDir "ytm-hybrid-v$Version-windows-x64.zip"
Remove-Item -Path $ZipPath -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $ZipDist "*") -DestinationPath $ZipPath -Force
Write-Host "Created: $ZipPath" -ForegroundColor Green

# ============================================================================
# 2. MSIX Package (Windows)
# ============================================================================
Write-Host "[3/3] Creating MSIX package..." -ForegroundColor Yellow

$ManifestPath = Join-Path $ProjectDir "Package.appxmanifest"

if (-not (Test-Path $ManifestPath)) {
@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="com.egehansimarik.ytmhybrid"
            Version="$Version.0"
            Publisher="CN=Egehan Simarik"
            ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>YTM Hybrid</DisplayName>
    <PublisherDisplayName>Egehan Simarik</PublisherDisplayName>
    <Logo>assets\linux\icons\hicolor\256x256\apps\ytm-hybrid.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop"
                        MinVersion="10.0.17763.0"
                        MaxVersionTested="10.0.22621.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="YTMHybrid"
                 Executable="$([io.path]::GetFileName($BinaryPath))"
                 EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
          DisplayName="YTM Hybrid"
          Square150x150Logo="assets\linux\icons\hicolor\256x256\apps\ytm-hybrid.png"
          Square44x44Logo="assets\linux\icons\hicolor\256x256\apps\ytm-hybrid.png"
          Description="YouTube Music Client"
          BackgroundColor="transparent">
        <uap:DefaultTile Wide310x150Logo="assets\linux\icons\hicolor\256x256\apps\ytm-hybrid.png" />
      </uap:VisualElements>
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"@ | Set-Content -Path $ManifestPath -Encoding UTF8
    Write-Host "MSIX manifest created at $ManifestPath" -ForegroundColor Green
}

# Find makeappx.exe automatically on Windows SDK paths or system PATH
$MakeAppxPath = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\makeappx.exe"
if (-not (Test-Path $MakeAppxPath)) {
    $FoundCmd = Get-Command "makeappx.exe" -ErrorAction SilentlyContinue
    if ($FoundCmd) {
        $MakeAppxPath = $FoundCmd.Source
    }
}

if ($MakeAppxPath -and (Test-Path $MakeAppxPath)) {
    $MsixOutputPath = Join-Path $DistDir "ytm-hybrid-v$Version-windows-x64.msix"
    Remove-Item -Path $MsixOutputPath -Force -ErrorAction SilentlyContinue

    Write-Host "Packing MSIX using MakeAppx..." -ForegroundColor Yellow
    & $MakeAppxPath pack /d $ZipDist /p $MsixOutputPath /v
    Write-Host "Created MSIX: $MsixOutputPath" -ForegroundColor Green
} else {
    Write-Host "WARNING: makeappx.exe not found. Skipping automated MSIX packaging (Requires Windows SDK)." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Windows Packaging Complete ===" -ForegroundColor Cyan
Get-ChildItem -Path $DistDir -File | Format-Table Name, Length
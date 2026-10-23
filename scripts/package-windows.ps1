# YTM Hybrid Windows Packaging Script
# This script creates portable and MSIX packages for Windows

param(
    [string]$Version = "0.2.0",
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
    Write-Host "[1/2] Building release executable..." -ForegroundColor Yellow
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
# 1. Portable ZIP Archive (Windows)
# ============================================================================
Write-Host "[1/2] Creating portable ZIP archive..." -ForegroundColor Yellow

$ZipDist = Join-Path $DistDir "ytm-hybrid-windows-x64"
Remove-Item -Path $ZipDist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $ZipDist -Force | Out-Null

# Copy binary
Copy-Item $BinaryPath -Destination $ZipDist

# Copy icon
Copy-Item "$ProjectDir\assets\linux\icons\hicolor\256x256\apps\ytm-hybrid.png" -Destination $ZipDist

# Copy LICENSE
Copy-Item "$ProjectDir\LICENSE" -Destination $ZipDist

# Create README
@"
# YTM Hybrid v$Version

Native YouTube Music Desktop Client built with Rust & GPUI.

## Installation

### Windows (Portable)
1. Extract the ZIP file
2. Run ytm-hybrid.exe

### MSIX (Pending)
Download and install the MSIX package.

## Features
- Native YouTube Music client
- System tray integration
- Media key support
- Notifications
- Lyrics display

## License
GPL-3.0-or-later
"@ | Set-Content -Path (Join-Path $ZipDist "README.md")

# Create ZIP
$ZipPath = Join-Path $DistDir "ytm-hybrid-v$Version-windows-x64.zip"
Compress-Archive -Path (Join-Path $ZipDist "*") -DestinationPath $ZipPath -Force
Write-Host "Created: $ZipPath" -ForegroundColor Green

# ============================================================================
# 2. MSIX Package (Windows)
# ============================================================================
Write-Host "[2/2] Creating MSIX package..." -ForegroundColor Yellow

# Note: MSIX packaging requires a Package.appxmanifest file
# This is a placeholder - you'll need to create a proper manifest
$ManifestPath = Join-Path $ProjectDir "Package.appxmanifest"

if (Test-Path $ManifestPath) {
    Write-Host "MSIX manifest found. Building MSIX package..." -ForegroundColor Yellow
    Write-Host "To generate MSIX, use: makeappx pack /d $ZipDist /p $DistDir\YTM_Hybrid.msix" -ForegroundColor Yellow
} else {
    Write-Host "MSIX manifest not found at $ManifestPath" -ForegroundColor Yellow
    Write-Host "Creating sample manifest for MSIX packaging..." -ForegroundColor Yellow

    # Create a basic Package.appxmanifest
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
    Write-Host "To build MSIX, install Windows SDK and run:" -ForegroundColor Yellow
    Write-Host "  makeappx pack /d $ZipDist /p $DistDir\YTM_Hybrid.msix" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Packaging Complete ===" -ForegroundColor Cyan
Write-Host "Output files:"
Get-ChildItem -Path $DistDir -File | Format-Table Name, Length

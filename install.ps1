# Sflang Installation Script for Windows
# Downloads and installs the latest version of Sflang
#
# Usage:
#   irm https://raw.githubusercontent.com/topxeq/sflang/main/install.ps1 | iex
#   OR
#   .\install.ps1
#

param(
    [string]$InstallDir = ""
)

$ErrorActionPreference = "Stop"
$Repo = "topxeq/sflang"
$BinaryName = "sf.exe"

function Write-Info {
    param([string]$Message)
    Write-Host "[INFO] " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Write-Warn {
    param([string]$Message)
    Write-Host "[WARN] " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

function Write-Err {
    param([string]$Message)
    Write-Host "[ERROR] " -ForegroundColor Red -NoNewline
    Write-Host $Message
    exit 1
}

# Detect architecture
function Get-Architecture {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64" { return "amd64" }
        "Arm64" { return "arm64" }
        default { Write-Err "Unsupported architecture: $arch" }
    }
}

# Get latest release version from GitHub API
function Get-LatestVersion {
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
        return $release.tag_name
    }
    catch {
        Write-Err "Failed to get latest version: $_"
    }
}

# Main installation function
function Install-Sflang {
    Write-Info "Detecting system..."
    $arch = Get-Architecture
    Write-Info "Architecture: $arch"

    Write-Info "Fetching latest version..."
    $version = Get-LatestVersion
    Write-Info "Latest version: $version"

    # Determine install directory
    if ($InstallDir -eq "") {
        $InstallDir = Join-Path $env:LOCALAPPDATA "sflang"
    }
    Write-Info "Install directory: $InstallDir"

    # Create install directory if it doesn't exist
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    # Download URL
    $url = "https://github.com/$Repo/releases/download/$version/sf-windows-$arch.zip"
    Write-Info "Downloading: $url"

    # Create temp directory
    $tmpDir = Join-Path $env:TEMP "sflang-install-$(Get-Random)"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        # Download archive
        $archivePath = Join-Path $tmpDir "sflang.zip"
        Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing

        # Extract
        Write-Info "Extracting..."
        Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force

        # Find and copy binary
        $binaryPath = Join-Path $tmpDir $BinaryName
        if (-not (Test-Path $binaryPath)) {
            # Search in subdirectories
            $binaryPath = Get-ChildItem -Path $tmpDir -Filter $BinaryName -Recurse | Select-Object -First 1 -ExpandProperty FullName
        }

        if (-not $binaryPath -or -not (Test-Path $binaryPath)) {
            Write-Err "Binary not found in archive"
        }

        # Copy to install directory
        $destPath = Join-Path $InstallDir $BinaryName
        Copy-Item -Path $binaryPath -Destination $destPath -Force
        Write-Info "Installed: $destPath"

        # Check if in PATH
        $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
        if ($userPath -notlike "*$InstallDir*") {
            Write-Info "Adding to PATH..."
            $newPath = "$userPath;$InstallDir"
            [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
            Write-Info "Added $InstallDir to user PATH"
        }

        # Verify installation
        $sfPath = Join-Path $InstallDir $BinaryName
        Write-Info "Installation complete!"
        Write-Host ""
        Write-Host "Sflang $version installed successfully!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Run 'sf' to use Sflang (you may need to restart your terminal)"
    }
    finally {
        # Cleanup
        if (Test-Path $tmpDir) {
            Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# Run installation
Install-Sflang
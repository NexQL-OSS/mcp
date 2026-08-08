# Install nexql-mcp from GitHub Releases (Windows).
#
# Usage (PowerShell):
#   irm https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.ps1 | iex
#
# Optional env:
#   $env:NEXQL_MCP_VERSION = "v0.2.1"
#   $env:NEXQL_MCP_INSTALL_DIR = "C:\Tools\nexql-mcp"
#   $env:NEXQL_MCP_REPO = "NexQL-OSS/mcp"
$ErrorActionPreference = "Stop"

$Repo = if ($env:NEXQL_MCP_REPO) { $env:NEXQL_MCP_REPO } else { "NexQL-OSS/mcp" }
$InstallRoot = if ($env:NEXQL_MCP_INSTALL_DIR) {
    $env:NEXQL_MCP_INSTALL_DIR
} else {
    Join-Path $env:LOCALAPPDATA "Programs\nexql-mcp"
}

function Write-Info([string]$Message) {
    Write-Host "==> $Message"
}

function Resolve-Tag {
    if ($env:NEXQL_MCP_VERSION) {
        return $env:NEXQL_MCP_VERSION
    }
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    return $release.tag_name
}

function Ensure-UserPath([string]$Directory) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $userPath) { $userPath = "" }
  $segments = $userPath -split ";" | Where-Object { $_ -and ($_ -ne $Directory) }
    $newPath = (@($Directory) + $segments) -join ";"
    if ($userPath -ne $newPath) {
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$Directory;$env:Path"
        Write-Info "Added $Directory to your user PATH (open a new terminal if nexql-mcp is not found)"
    }
}

$tag = Resolve-Tag
$triple = "x86_64-pc-windows-msvc"
$stage = "nexql-mcp-$tag-$triple"
$archive = "$stage.tar.gz"
$url = "https://github.com/$Repo/releases/download/$tag/$archive"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("nexql-mcp-install-" + [guid]::NewGuid().ToString())
$archivePath = Join-Path $tempDir $archive
$destBinary = Join-Path $InstallRoot "nexql-mcp.exe"

try {
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

    Write-Info "Installing nexql-mcp $tag for $triple"
    Write-Info "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing

    tar -xzf $archivePath -C $tempDir
    $sourceBinary = Join-Path $tempDir $stage "nexql-mcp.exe"
    if (-not (Test-Path $sourceBinary)) {
        throw "archive did not contain $stage/nexql-mcp.exe"
    }

    Copy-Item -Force $sourceBinary $destBinary
    Ensure-UserPath $InstallRoot

    Write-Host ""
    Write-Host "nexql-mcp installed successfully."
    Write-Host ""
    Write-Host "Next steps:"
    Write-Host ""
    Write-Host "  1. Verify the install:"
    Write-Host "       nexql-mcp --version"
    Write-Host ""
    Write-Host "  2. Test your Postgres connection:"
    Write-Host "       nexql-mcp postgres://USER:PASS@localhost:5432/DBNAME doctor"
    Write-Host ""
    Write-Host "  3. Wire an MCP client (pick one):"
    Write-Host "       nexql-mcp init cursor"
    Write-Host "       nexql-mcp init claude-desktop"
    Write-Host "       nexql-mcp init vscode-copilot"
    Write-Host ""
    Write-Host "     Or run the guided setup wizard:"
    Write-Host "       nexql-mcp tui"
    Write-Host ""
    Write-Host "     Installed via uv? Same commands after 'uv tool install nexql-mcp'."
    Write-Host ""
    Write-Host "  Docs: https://github.com/NexQL-OSS/mcp/blob/main/docs/clients/README.md"
}
finally {
    if (Test-Path $tempDir) {
        Remove-Item -Recurse -Force $tempDir
    }
}

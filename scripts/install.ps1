# hardgate Windows installer:
#   irm https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.ps1 | iex
param(
  [string]$Version = "latest",
  [string]$InstallDir = "$env:USERPROFILE\.cargo\bin"
)
$ErrorActionPreference = "Stop"
$Repo = "Tech-Byte-Frontier/hardgate"
$Pkg = "hardgate-win32-x64"
if ($Version -eq "latest") {
  $Url = "https://github.com/$Repo/releases/latest/download/$Pkg.tar.gz"
} else {
  $Url = "https://github.com/$Repo/releases/download/$Version/$Pkg.tar.gz"
}
Write-Host "hardgate: downloading $Url"
$tmp = Join-Path $env:TEMP "hardgate-install"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
Invoke-WebRequest -Uri $Url -OutFile "$tmp\pkg.tar.gz"
tar xzf "$tmp\pkg.tar.gz" -C $tmp
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item "$tmp\$Pkg\hardgate.exe" "$InstallDir\hardgate.exe" -Force
& "$InstallDir\hardgate.exe" --version

param(
    [Parameter(Mandatory = $true)][string]$BinaryPath,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$OutDir
)

$ErrorActionPreference = "Stop"

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$BinAbs = (Resolve-Path $BinaryPath).Path
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
$OutAbs = (Resolve-Path $OutDir).Path

# Copy standalone portable executable and zip archive
Copy-Item $BinAbs (Join-Path $OutAbs "textree-$Tag-windows-x86_64.exe") -Force

$Stage = Join-Path $OutAbs "stage_win"
New-Item -ItemType Directory -Path $Stage -Force | Out-Null
Copy-Item $BinAbs (Join-Path $Stage "textree.exe")
Copy-Item (Join-Path $RootDir "README.md") (Join-Path $Stage "README.md")
Compress-Archive -Path "$Stage\*" -DestinationPath (Join-Path $OutAbs "textree-$Tag-windows-x86_64.zip") -Force
Remove-Item -Path $Stage -Recurse -Force

# Locate or install Inno Setup 6
$Iscc = (Get-Command "iscc.exe" -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source)
if (-not $Iscc) {
    $DefaultIscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
    if (Test-Path $DefaultIscc) {
        $Iscc = $DefaultIscc
    }
}
if (-not $Iscc) {
    Write-Host "Installing Inno Setup via Chocolatey..."
    choco install innosetup -y --no-progress | Out-Null
    $DefaultIscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
    if (Test-Path $DefaultIscc) {
        $Iscc = $DefaultIscc
    } else {
        $Iscc = (Get-Command "iscc.exe" -ErrorAction Stop | Select-Object -First 1 -ExpandProperty Source)
    }
}

$IssPath = Join-Path $PSScriptRoot "textree.iss"
$SetupBase = "textree-$Tag-windows-x86_64-setup"
& $Iscc "/DMyAppVersion=$Version" "/DBinaryPath=$BinAbs" "/DOutputDir=$OutAbs" "/DOutputBaseFilename=$SetupBase" $IssPath
if ($LASTEXITCODE -ne 0) {
    throw "ISCC failed with exit code $LASTEXITCODE"
}

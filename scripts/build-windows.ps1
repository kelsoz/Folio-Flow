# Builds and verifies the portable Windows package. Run with PowerShell 7.
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$outputDir = Join-Path $projectRoot 'dist\Folio Flow'
$checksDir = Join-Path $projectRoot 'target\checks'

Push-Location $projectRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }

    New-Item -ItemType Directory -Force -Path $outputDir, $checksDir | Out-Null
    # Refuse to update a package that is still running; never stop the user's app.
    foreach ($name in @('Folio Flow.exe', 'pdfium.dll')) {
        $existingPath = Join-Path $outputDir $name
        if (Test-Path -LiteralPath $existingPath) {
            $handle = [IO.File]::Open($existingPath, 'Open', 'ReadWrite', 'None')
            $handle.Dispose()
        }
    }

    Copy-Item -LiteralPath 'target\release\folio.exe' -Destination (Join-Path $outputDir 'Folio Flow.exe') -Force
    Copy-Item -LiteralPath 'target\release\pdfium.dll' -Destination $outputDir -Force
    Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $outputDir 'LICENSE.txt') -Force
    Copy-Item -LiteralPath 'assets\icons\LICENSE' -Destination (Join-Path $outputDir 'LICENSE.lucide') -Force
    Copy-Item -LiteralPath 'vendor\pdfium\x86_64-pc-windows-msvc\LICENSE.pdfium' -Destination $outputDir -Force
    Copy-Item -LiteralPath 'docs\USAGE.zh-CN.md' -Destination (Join-Path $outputDir 'USAGE.zh-CN.md') -Force

    $binaryPath = Join-Path $outputDir 'Folio Flow.exe'
    $smokePath = Join-Path $checksDir 'portable-export.md'
    if (Test-Path -LiteralPath $smokePath) { Remove-Item -LiteralPath $smokePath }
    $processInfo = [Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $binaryPath
    $processInfo.UseShellExecute = $false
    $processInfo.CreateNoWindow = $true
    $processInfo.ArgumentList.Add('--export-markdown')
    $processInfo.ArgumentList.Add((Join-Path $projectRoot 'tests\fixtures\sample.pdf'))
    $processInfo.ArgumentList.Add($smokePath)
    $process = [Diagnostics.Process]::Start($processInfo)
    try {
        if (-not $process.WaitForExit(30000)) {
            $process.Kill()
            throw 'Portable export check timed out.'
        }
        if ($process.ExitCode -ne 0) { throw 'Portable export check failed.' }
    } finally { $process.Dispose() }
    $markdown = Get-Content -LiteralPath $smokePath -Raw -Encoding utf8
    if (-not ($markdown.Contains('Introduction') -and $markdown.Contains('Results'))) {
        throw 'Portable export did not contain the expected fixture content.'
    }

    $files = [ordered]@{}
    foreach ($name in @('Folio Flow.exe', 'pdfium.dll')) {
        $path = Join-Path $outputDir $name
        $files[$name] = @{
            bytes = (Get-Item -LiteralPath $path).Length
            sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
    [ordered]@{
        product = 'Folio Flow'
        built_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        platform = 'Windows x64'
        portable_export_check = 'passed'
        files = $files
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputDir 'build-info.json') -Encoding utf8

    Write-Host "Ready: $binaryPath"
} finally { Pop-Location }

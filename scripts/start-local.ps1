param(
    [ValidateRange(1024, 65535)]
    [int]$Port = 8080,
    [string]$Config,
    [ValidatePattern('^[0-9]{1,20}$')]
    [string]$Seed = '42',
    [string]$DataDir,
    [string]$Resume,
    [string]$Checkpoint,
    [switch]$Cells,
    [switch]$OpenBrowser
)

$ErrorActionPreference = 'Stop'
$projectPath = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectPath
try {
    if ($Resume -and ($Config -or $PSBoundParameters.ContainsKey('Seed'))) {
        throw 'Resume uses the saved config and seed; do not pass Config or Seed.'
    }
    if ($Checkpoint -and -not $Resume) { throw 'Checkpoint requires Resume.' }
    cargo build --release -p liminis
    if ($LASTEXITCODE -ne 0) { throw 'Liminis build failed.' }

    $chosenPort = $Port
    if ($Cells -and -not $PSBoundParameters.ContainsKey('Port')) { $chosenPort = 8083 }
    while ($chosenPort -le 65535) {
        $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $chosenPort)
        try {
            $probe.Start()
            break
        } catch [System.Net.Sockets.SocketException] {
            $chosenPort++
        } finally {
            $probe.Stop()
        }
    }
    if ($chosenPort -gt 65535) { throw 'No free local port found.' }

    $binaryPath = Join-Path $projectPath 'target\release\liminis.exe'
    $runPath = Join-Path $projectPath "target\local-server-$chosenPort.exe"
    Copy-Item -LiteralPath $binaryPath -Destination $runPath -Force
    $logPath = Join-Path $projectPath "target\local-$chosenPort.log"
    $errorPath = Join-Path $projectPath "target\local-$chosenPort.error.log"
    if (-not $DataDir) {
        $storageFolder = if ($Cells) { '.liminis\cells' } else { '.liminis\runs' }
        $DataDir = Join-Path $projectPath $storageFolder
    }
    $dataPath = [System.IO.Path]::GetFullPath($DataDir)
    $command = if ($Cells) { 'cells' } else { 'serve' }
    $serverArgs = @($command, '--port', $chosenPort, '--data-dir', "`"$dataPath`"")
    if ($Resume) {
        $serverArgs += @('--resume', "`"$Resume`"")
        if ($Checkpoint) { $serverArgs += @('--checkpoint', "`"$Checkpoint`"") }
    } else {
        $serverArgs += @('--seed', $Seed)
    }
    if ($Config) {
        $configPath = (Resolve-Path -LiteralPath $Config).Path
        $serverArgs += @('--config', "`"$configPath`"")
    }
    $serverProcess = Start-Process -FilePath $runPath -ArgumentList $serverArgs `
        -WorkingDirectory $projectPath -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $logPath -RedirectStandardError $errorPath
    $localUrl = "http://127.0.0.1:$chosenPort/"
    $ready = $false
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        Start-Sleep -Milliseconds 250
        if ($serverProcess.HasExited) {
            throw "Liminis stopped. See $errorPath"
        }
        try {
            $state = Invoke-RestMethod -Uri "${localUrl}api/state" -TimeoutSec 2
            if ($state.alive) { $ready = $true; break }
        } catch { }
    }
    if (-not $ready) {
        Stop-Process -Id $serverProcess.Id
        throw "Liminis did not answer. See $errorPath"
    }
    Write-Output "Liminis: $localUrl (process $($serverProcess.Id))"
    Write-Output "Experiment: $($state.persistence.run_id) in $dataPath"
    if ($OpenBrowser) { Start-Process $localUrl }
} finally {
    Pop-Location
}

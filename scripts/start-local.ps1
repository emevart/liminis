param(
    [ValidateRange(1024, 65535)]
    [int]$Port = 8080,
    [switch]$OpenBrowser
)

$ErrorActionPreference = 'Stop'
$projectPath = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectPath
try {
    cargo build --release -p liminis
    if ($LASTEXITCODE -ne 0) { throw 'Liminis build failed.' }

    $chosenPort = $Port
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
    $serverProcess = Start-Process -FilePath $runPath -ArgumentList @('serve', '--port', $chosenPort) `
        -WorkingDirectory $projectPath -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $logPath -RedirectStandardError $errorPath
    $localUrl = "http://127.0.0.1:$chosenPort/"
    $ready = $false
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
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
    if ($OpenBrowser) { Start-Process $localUrl }
} finally {
    Pop-Location
}

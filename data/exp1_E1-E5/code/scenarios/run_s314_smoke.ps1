param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('B1', 'B2', 'G3')]
    [string]$Baseline
)

$ErrorActionPreference = 'Stop'

$expRoot = '/home/gz785/code/exp2'
$nodeExe = 'D:\nodejs\node.exe'
$validator = "$expRoot/scripts/common/validate_s314_smoke.py"
$rpcPort = 9944
$p2pPort = 30333
$prometheusPort = 9615

$specs = @{
    B1 = @{
        Worktree = '/home/gz785/code/assetxchain-b1'
        Node = '/home/gz785/code/assetxchain-b1/target-s314/release/assetxchain-node'
        Driver = "$expRoot/scripts/scenarios/s314_b1_live_smoke.js"
        Artifacts = @(
            '/home/gz785/code/assetxchain-b1/target-s314/release/assetxchain-node',
            '/home/gz785/code/assetxchain-b1/contracts/market_pure_contract/target/ink/market_pure_contract.wasm',
            '/home/gz785/code/assetxchain-b1/contracts/market_pure_contract/target/ink/market_pure_contract.json'
        )
        BuildCommands = @(
            'cargo +1.85.0 contract build --manifest-path /home/gz785/code/assetxchain-b1/contracts/market_pure_contract/Cargo.toml --release --optimization-passes z',
            'CARGO_TARGET_DIR=/home/gz785/code/assetxchain-b1/target-s314 cargo +1.85.0 build --manifest-path /home/gz785/code/assetxchain-b1/Cargo.toml --release --locked -p assetxchain-node'
        )
    }
    B2 = @{
        Worktree = '/home/gz785/code/assetxchain-b2'
        Node = '/home/gz785/code/assetxchain-b2/target-s314/release/assetxchain-node'
        Driver = "$expRoot/scripts/scenarios/s314_b2_live_smoke.js"
        Artifacts = @('/home/gz785/code/assetxchain-b2/target-s314/release/assetxchain-node')
        BuildCommands = @(
            'CARGO_TARGET_DIR=/home/gz785/code/assetxchain-b2/target-s314 cargo +1.85.0 build --manifest-path /home/gz785/code/assetxchain-b2/Cargo.toml --release --locked -p assetxchain-node'
        )
    }
    G3 = @{
        Worktree = '/home/gz785/code/assetxchain-g3-v2'
        Node = '/home/gz785/code/assetxchain-g3-v2/target-s314/release/assetxchain-node'
        Driver = "$expRoot/scripts/scenarios/s314_g3_live_smoke.js"
        Artifacts = @(
            '/home/gz785/code/assetxchain-g3-v2/target-s314/release/assetxchain-node',
            '/home/gz785/code/assetxchain-g3-v2/target/ink/market_orderbook/market_orderbook.wasm'
        )
        BuildCommands = @(
            'cargo +1.85.0 contract build --manifest-path /home/gz785/code/assetxchain-g3-v2/contracts/market_orderbook/Cargo.toml --release -Z original-manifest --generate code-only',
            'CARGO_TARGET_DIR=/home/gz785/code/assetxchain-g3-v2/target-s314 cargo +1.85.0 build --manifest-path /home/gz785/code/assetxchain-g3-v2/Cargo.toml --release --locked -p assetxchain-node'
        )
    }
}

function Convert-WslPath([string]$Path) {
    return '\\wsl.localhost\Ubuntu' + $Path.Replace('/', '\')
}

function Invoke-WslGit([string]$Worktree, [string[]]$Arguments) {
    $output = & wsl.exe -d Ubuntu -- git -C $Worktree @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git failed for $Worktree"
    }
    return ($output -join "`n").Trim()
}

function Test-RpcReady([int]$Port, [int]$TimeoutSeconds, [System.Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($Process.HasExited) {
            return $false
        }
        $client = [System.Net.Sockets.TcpClient]::new()
        try {
            $task = $client.ConnectAsync('127.0.0.1', $Port)
            if ($task.Wait(1000) -and $client.Connected) {
                return $true
            }
        }
        catch {
        }
        finally {
            $client.Dispose()
        }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

function Test-TcpListening([int]$Port) {
    $client = [System.Net.Sockets.TcpClient]::new()
    try {
        $task = $client.ConnectAsync('127.0.0.1', $Port)
        return $task.Wait(500) -and $client.Connected
    }
    catch {
        return $false
    }
    finally {
        $client.Dispose()
    }
}

function Get-BaselineNodePids([string]$NodePath) {
    $pattern = "^$([Regex]::Escape($NodePath)) --dev --tmp"
    $rows = & wsl.exe -d Ubuntu -- pgrep -f $pattern
    if ($LASTEXITCODE -notin @(0, 1)) {
        throw "could not inspect WSL node processes for $NodePath"
    }
    return @($rows | Where-Object { $_ -match '^\d+$' })
}

function Write-ExclusiveJson([string]$Path, [object]$Value) {
    $json = $Value | ConvertTo-Json -Depth 20
    $encoding = [System.Text.UTF8Encoding]::new($false)
    $stream = [System.IO.FileStream]::new($Path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
    try {
        $writer = [System.IO.StreamWriter]::new($stream, $encoding)
        try {
            $writer.Write($json)
            $writer.Write("`n")
        }
        finally {
            $writer.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

$spec = $specs[$Baseline]
if (-not (Test-Path -LiteralPath $nodeExe -PathType Leaf)) {
    throw "Windows Node is missing: $nodeExe"
}
foreach ($path in @($spec.Node, $spec.Driver) + $spec.Artifacts) {
    if (-not (Test-Path -LiteralPath (Convert-WslPath $path) -PathType Leaf)) {
        throw "Required S3.14 artifact is missing: $path"
    }
}
foreach ($port in @($rpcPort, $p2pPort, $prometheusPort)) {
    if (Test-TcpListening $port) {
        throw "Required S3.14 port is already in use: $port"
    }
}
if ((Get-BaselineNodePids $spec.Node).Count -gt 0) {
    throw "A node for $Baseline is already running"
}

$head = Invoke-WslGit $spec.Worktree @('rev-parse', 'HEAD')
$branch = Invoke-WslGit $spec.Worktree @('branch', '--show-current')
$dirtyText = Invoke-WslGit $spec.Worktree @('status', '--porcelain=v1')
[object[]]$dirtyPaths = @()
if ($dirtyText) {
    $dirtyPaths = @($dirtyText -split "`n")
}
$dirty = $dirtyPaths.Count -gt 0
$stamp = [DateTimeOffset]::Now.ToString('yyyyMMddTHHmmsszzz').Replace(':', '')
$runId = "s314-$($Baseline.ToLower())-$stamp-$($head.Substring(0, 8))"
$rawDir = "$expRoot/results/raw/$runId"
$logDir = "$expRoot/logs/$runId"
$rawDirWindows = Convert-WslPath $rawDir
$logDirWindows = Convert-WslPath $logDir
if ((Test-Path -LiteralPath $rawDirWindows) -or (Test-Path -LiteralPath $logDirWindows)) {
    throw "run_id collision: $runId"
}
[System.IO.Directory]::CreateDirectory($rawDirWindows) | Out-Null
[System.IO.Directory]::CreateDirectory($logDirWindows) | Out-Null

$evidence = "$rawDir/live-smoke.json"
$manifest = "$rawDir/manifest.json"
$nodeLog = "$logDir/node.log"
$nodeStdout = "$logDir/node.stdout.log"
$driverLog = "$logDir/driver.log"
$driverStdout = "$logDir/driver.stdout.log"
$nodeArgs = @(
    '-d', 'Ubuntu', '--', $spec.Node, '--dev', '--tmp',
    '--rpc-port', "$rpcPort", '--port', "$p2pPort", '--prometheus-port', "$prometheusPort",
    '--rpc-methods', 'Unsafe', '--rpc-cors', 'all'
)
$nodeCommand = @($spec.Node, '--dev', '--tmp', '--rpc-port', "$rpcPort", '--port', "$p2pPort", '--prometheus-port', "$prometheusPort", '--rpc-methods', 'Unsafe', '--rpc-cors', 'all')
$driverWindows = Convert-WslPath $spec.Driver
$driverCommand = @($nodeExe, $driverWindows)
$startedAt = [DateTimeOffset]::Now.ToString('yyyy-MM-ddTHH:mm:sszzz')
$nodeProcess = $null
$rpcReady = $false
$status = 'FAIL'
$failure = $null

try {
    $nodeProcess = Start-Process -FilePath 'wsl.exe' -ArgumentList $nodeArgs -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput (Convert-WslPath $nodeStdout) -RedirectStandardError (Convert-WslPath $nodeLog)
    $rpcReady = Test-RpcReady $rpcPort 90 $nodeProcess
    if (-not $rpcReady) {
        throw 'RPC did not become ready'
    }

    $env:ASSETX_RPC = "ws://127.0.0.1:$rpcPort"
    $env:RUN_ID = $runId
    $env:RAW_DIR = $rawDirWindows
    $env:IMPLEMENTATION_COMMIT = $head
    $env:GIT_DIRTY = $dirty.ToString().ToLowerInvariant()
    if ($Baseline -eq 'B1') {
        $env:B1_CONTRACT_WASM = Convert-WslPath $spec.Artifacts[1]
        $env:B1_CONTRACT_METADATA = Convert-WslPath $spec.Artifacts[2]
    }
    elseif ($Baseline -eq 'G3') {
        $env:PROJECT_ROOT = Convert-WslPath $spec.Worktree
        $env:G3_V2_WASM = Convert-WslPath $spec.Artifacts[1]
    }
    $driverProcess = Start-Process -FilePath $nodeExe -ArgumentList @($driverWindows) -PassThru `
        -WindowStyle Hidden -Wait -RedirectStandardOutput (Convert-WslPath $driverStdout) `
        -RedirectStandardError (Convert-WslPath $driverLog)
    if ($driverProcess.ExitCode -ne 0) {
        throw "live driver exited $($driverProcess.ExitCode)"
    }
    & wsl.exe -d Ubuntu -- python3 $validator --evidence $evidence
    if ($LASTEXITCODE -ne 0) {
        throw 'live evidence validation failed'
    }
    $status = 'PASS'
}
catch {
    $failure = $_.Exception.Message
}
finally {
    foreach ($linuxPid in @(Get-BaselineNodePids $spec.Node)) {
        & wsl.exe -d Ubuntu -- kill -TERM $linuxPid
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Could not terminate WSL node PID $linuxPid"
        }
    }
    if ($null -ne $nodeProcess) {
        $nodeProcess.WaitForExit(10000) | Out-Null
        if (-not $nodeProcess.HasExited) {
            Stop-Process -Id $nodeProcess.Id
            $nodeProcess.WaitForExit(10000) | Out-Null
        }
    }
}

$finishedAt = [DateTimeOffset]::Now.ToString('yyyy-MM-ddTHH:mm:sszzz')
$artifactRows = @()
foreach ($artifact in $spec.Artifacts) {
    $windowsArtifact = Convert-WslPath $artifact
    $item = Get-Item -LiteralPath $windowsArtifact
    $artifactRows += [ordered]@{
        path = $artifact
        sha256 = (Get-FileHash -LiteralPath $windowsArtifact -Algorithm SHA256).Hash.ToLowerInvariant()
        bytes = [int64]$item.Length
    }
}
$nodeExitCode = if ($null -ne $nodeProcess -and $nodeProcess.HasExited) { $nodeProcess.ExitCode } else { -1 }
$manifestValue = [ordered]@{
    schema_id = 'assetxchain-exp2-s314-run-manifest'
    schema_version = 1
    run_id = $runId
    baseline_id = $Baseline
    status = $status
    validity_level = 'legacy-pilot'
    profile_id = 'phase3-s314-legacy-pilot-v1'
    started_at = $startedAt
    finished_at = $finishedAt
    timezone = 'Asia/Shanghai'
    implementation = [ordered]@{
        worktree = $spec.Worktree
        branch = $branch
        head = $head
        dirty = $dirty
        dirty_paths = $dirtyPaths
    }
    build = [ordered]@{ commands = $spec.BuildCommands; artifacts = $artifactRows }
    execution = [ordered]@{
        node_command = $nodeCommand
        driver_command = $driverCommand
        rpc = "ws://127.0.0.1:$rpcPort"
        reset = '--dev --tmp'
        node_exit_code = [int]$nodeExitCode
        rpc_ready = $rpcReady
    }
    paths = [ordered]@{
        raw_dir = $rawDir
        log_dir = $logDir
        evidence = $evidence
        node_log = $nodeLog
        driver_log = $driverLog
    }
    config_path = 'configs/phase3/s314-smoke-v1.json'
    claim_boundary = 'build-and-live-smoke-evidence-only; not a paper result'
    formal_experiment_executed = $false
    e6_executed = $false
}
Write-ExclusiveJson (Convert-WslPath $manifest) $manifestValue

if ($status -eq 'PASS') {
    & wsl.exe -d Ubuntu -- python3 $validator --evidence $evidence --manifest $manifest
    if ($LASTEXITCODE -ne 0) {
        throw "sealed manifest validation failed for $runId"
    }
    [ordered]@{ status = $status; baseline = $Baseline; run_id = $runId; manifest = $manifest } | ConvertTo-Json
    exit 0
}

Write-Error "S3.14 $Baseline smoke failed; immutable run preserved at $rawDir`: $failure"
exit 1

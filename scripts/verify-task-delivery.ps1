param(
    [Parameter(Mandatory)][string]$BaseRoot,
    [Parameter(Mandatory)][ValidatePattern('^sha256:[0-9a-f]{64}$')][string]$RustImage,
    [Parameter(Mandatory)][ValidatePattern('^sha256:[0-9a-f]{64}$')][string]$PostgresImage,
    [string]$DockerToolsImage,
    [ValidatePattern('^sdlc_pg_delivery[A-Za-z0-9_]*$')][string]$PostgresTestFilter = 'sdlc_pg_delivery',
    [Parameter(Mandatory)][string]$TargetCache,
    [Parameter(Mandatory)][string]$CargoCache,
    [Parameter(Mandatory)][string]$RustupCache,
    [ValidateSet('full', 'delivery', 'oci', 'postgres')][string]$Gate = 'full',
    [string]$DockerContext = 'desktop-linux'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$sdk = (Resolve-Path -LiteralPath $BaseRoot).Path
if ($Gate -in @('oci', 'postgres') -and $DockerToolsImage -notmatch '^sha256:[0-9a-f]{64}$') {
    throw 'OCI and PostgreSQL gates require a pinned Docker CLI/Compose image ID'
}
$project = 'sdlc-qa-forge-delivery-' + [guid]::NewGuid().ToString('N').Substring(0, 12)
$compose = Join-Path $repo 'deploy/qa/task-delivery.compose.yml'
$composeArgs = @('-f', $compose)
if ($Gate -in @('oci', 'postgres')) { $composeArgs += @('-f', (Join-Path $repo 'deploy/qa/task-delivery-engine-tools.compose.yml')) }
if ($Gate -eq 'oci') { $composeArgs += @('-f', (Join-Path $repo 'deploy/qa/task-delivery-oci.compose.yml')) }
if ($Gate -eq 'postgres') { $composeArgs += @('-f', (Join-Path $repo 'deploy/qa/task-delivery-postgres.compose.yml')) }
$output = Join-Path $repo ('.local/task-delivery-qa/' + $project)
[void](New-Item -ItemType Directory -Path $output -Force)
$composeHost = Join-Path $output 'oci-compose'
$composeLocal = $composeHost.Replace('\','/')
$junction = $null
python -B "$sdk/scripts/verify_base_revision.py" --base $sdk --revision "$repo/.base-revision"
if ($LASTEXITCODE -ne 0) { throw 'Pinned Base validation failed' }
$images = @($RustImage, $PostgresImage)
if ($Gate -in @('oci', 'postgres')) { $images += $DockerToolsImage }
foreach ($image in $images) {
    $actual = docker --context $DockerContext image inspect $image --format '{{.Id}}'
    if ($LASTEXITCODE -ne 0 -or $actual -ne $image) { throw 'Required pinned QA image unavailable' }
}
function SourceHashes {
    $hashes = [ordered]@{}
    foreach ($entry in @(@{root=$repo; name='CI-CD'; paths=@('backend','deploy','scripts','openapi','.base-revision')},
                        @{root=$sdk; name='services-base'; paths=@('crates','Cargo.toml','Cargo.lock','LICENSE','scripts/platform_backup.py','scripts/platform_postgres.py','scripts/compose_helpers.py')})) {
        $paths = git -C $entry.root ls-files -- $entry.paths
        if ($LASTEXITCODE -ne 0) { throw 'Source inventory failed' }
        foreach ($path in $paths) { $hashes[$entry.name + '/' + $path] = (Get-FileHash -LiteralPath (Join-Path $entry.root $path) -Algorithm SHA256).Hash }
    }
    return $hashes
}
$before = SourceHashes
$before | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $output 'sources.json') -Encoding utf8
$variables = @{
    CICD_QA_SOURCE_ROOT=$repo; CICD_QA_BASE_ROOT=$sdk; CICD_QA_OUTPUT_ROOT=$output;
    CICD_QA_RUST_IMAGE=$RustImage; CICD_QA_POSTGRES_IMAGE=$PostgresImage; CICD_QA_DOCKER_TOOLS_IMAGE=$DockerToolsImage;
    CICD_QA_TARGET_CACHE=$TargetCache; CICD_QA_CARGO_CACHE=$CargoCache;
    CICD_QA_RUSTUP_CACHE=$RustupCache; CICD_QA_GATE=$Gate
    CICD_QA_POSTGRES_TEST_FILTER=$PostgresTestFilter
    CICD_QA_OCI_PROJECT=('sdlc-qa-forge-oci-' + $project.Substring($project.Length - 12))
    CICD_QA_PG_PROJECT=('sdlc-qa-forge-pg-' + $project.Substring($project.Length - 12))
    CICD_QA_OCI_NETWORK=($project + '_qa'); CICD_QA_OCI_VOLUME=($project + '_delivery-qa')
    CICD_QA_OCI_COMPOSE_HOST=$composeHost
    CICD_QA_OCI_COMPOSE_LOCAL=$(if ($Gate -in @('oci','postgres') -and $IsWindows) { '/' + $project + '-compose' } else { $composeLocal })
}
$previous = @{}
foreach ($key in $variables.Keys) {
    $previous[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
    [Environment]::SetEnvironmentVariable($key, $variables[$key], 'Process')
}
$result = 1
Write-Output "QA_PROJECT=$project"
Write-Output "QA_EVIDENCE=$output"
try {
    if ($Gate -in @('oci','postgres')) {
        [void](New-Item -ItemType Directory -Path $composeHost)
        if ($IsWindows) {
            $junction = Join-Path ([IO.Path]::GetPathRoot($repo)) ($project + '-compose')
            if (Test-Path -LiteralPath $junction) { throw 'Unique QA path alias already exists' }
            [void](New-Item -ItemType Junction -Path $junction -Target $composeHost)
        }
    }
    docker --context $DockerContext compose -p $project @composeArgs config -q
    if ($LASTEXITCODE -ne 0) { throw 'QA Compose config invalid' }
    docker --context $DockerContext compose -p $project @composeArgs build
    if ($LASTEXITCODE -ne 0) { throw 'QA Compose build failed' }
    docker --context $DockerContext compose -p $project @composeArgs up --pull never --abort-on-container-exit --exit-code-from qa 2>&1 | Tee-Object -FilePath (Join-Path $output 'compose.log')
    $result = $LASTEXITCODE
    $qaContainers = @(docker --context $DockerContext ps -aq --filter "label=com.docker.compose.project=$project")
    if ($LASTEXITCODE -ne 0 -or $qaContainers.Count -eq 0) { throw 'QA container readback unavailable' }
    $qaInventory = @(docker --context $DockerContext container inspect $qaContainers | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'QA container inspection failed' }
    $qaService = $qaInventory | Where-Object { $_.Config.Labels.'com.docker.compose.service' -eq 'qa' }
    if ($qaService.Count -ne 1 -or $qaService[0].State.Status -ne 'exited' -or $qaService[0].State.ExitCode -ne 0) {
        $result = 1
    }
    if ($Gate -in @('oci', 'postgres')) {
        $toolsService = $qaInventory | Where-Object { $_.Config.Labels.'com.docker.compose.service' -eq 'engine-tools' }
        $toolsStartedHealthy = Select-String -LiteralPath (Join-Path $output 'compose.log') -SimpleMatch "Container $project-engine-tools-1 Healthy" -Quiet
        if ($toolsService.Count -ne 1 -or $toolsService[0].Config.Image -ne $DockerToolsImage -or -not $toolsStartedHealthy) {
            $result = 1
        }
    }
} finally {
    try {
        if ($Gate -eq 'postgres') {
            $childCompose = Join-Path $output 'pg-compose.json'
            if (Test-Path -LiteralPath $childCompose) {
                docker --context $DockerContext compose -p $variables.CICD_QA_PG_PROJECT -f $childCompose down --remove-orphans
                if ($LASTEXITCODE -ne 0) { throw 'Owned PostgreSQL child cleanup failed' }
            }
            $childRemaining = docker --context $DockerContext ps -aq --filter "label=com.docker.compose.project=$($variables.CICD_QA_PG_PROJECT)"
            if ($LASTEXITCODE -ne 0 -or $childRemaining) { throw 'Owned PostgreSQL containers remain; preserve QA volumes' }
            $pgVolume = $variables.CICD_QA_PG_PROJECT + '_postgres-data'
            $ownedPg = docker --context $DockerContext volume ls -q --filter "name=^${pgVolume}$"
            if ($LASTEXITCODE -ne 0) { throw 'Disposable PostgreSQL volume inventory failed' }
            if ($ownedPg) {
                $owner = docker --context $DockerContext volume inspect $pgVolume --format '{{index .Labels "com.docker.compose.project"}}'
                if ($LASTEXITCODE -ne 0 -or $owner -ne $variables.CICD_QA_PG_PROJECT) { throw 'Disposable PostgreSQL ownership mismatch' }
                docker --context $DockerContext volume rm $pgVolume
                if ($LASTEXITCODE -ne 0) { throw 'Owned PostgreSQL volume cleanup failed' }
            }
        }
        if ($Gate -eq 'oci') {
            $childCompose = Join-Path $output 'oci-compose.json'
            if (Test-Path -LiteralPath $childCompose) {
                docker --context $DockerContext compose -p $variables.CICD_QA_OCI_PROJECT -f $childCompose down --remove-orphans
                if ($LASTEXITCODE -ne 0) { throw 'Owned OCI child cleanup failed' }
            }
            $childRemaining = docker --context $DockerContext ps -aq --filter "label=com.docker.compose.project=$($variables.CICD_QA_OCI_PROJECT)"
            if ($LASTEXITCODE -ne 0 -or $childRemaining) { throw 'Owned OCI containers remain; preserve QA volume' }
        }
        docker --context $DockerContext compose -p $project @composeArgs down --remove-orphans 2>&1 | Tee-Object -FilePath (Join-Path $output 'cleanup.log')
        if ($LASTEXITCODE -ne 0) { throw 'Owned Compose cleanup failed' }
        $remaining = docker --context $DockerContext ps -aq --filter "label=com.docker.compose.project=$project"
        if ($LASTEXITCODE -ne 0 -or $remaining) { throw 'Owned containers remain or inventory failed' }
        $networks = docker --context $DockerContext network ls -q --filter "label=com.docker.compose.project=$project"
        if ($LASTEXITCODE -ne 0 -or $networks) { throw 'Owned networks remain or inventory failed' }
        $composeVolumes = @('delivery-qa')
        if ($Gate -in @('oci', 'postgres')) { $composeVolumes += 'engine-tools' }
        $allVolumes = @(docker --context $DockerContext volume ls -q)
        if ($LASTEXITCODE -ne 0) { throw 'Disposable volume inventory failed' }
        foreach ($composeVolume in $composeVolumes) {
            $ownVolume = $project + '_' + $composeVolume
            if ($allVolumes -ccontains $ownVolume) {
                $metadata = @(docker --context $DockerContext volume inspect $ownVolume | ConvertFrom-Json)
                if ($LASTEXITCODE -ne 0 -or $metadata.Count -ne 1 -or
                    $metadata[0].Labels.'com.docker.compose.project' -cne $project -or
                    $metadata[0].Labels.'com.docker.compose.volume' -cne $composeVolume) {
                    throw "Disposable Compose volume ownership mismatch: $composeVolume"
                }
                $attached = @(docker --context $DockerContext ps -aq --filter "volume=$ownVolume")
                if ($LASTEXITCODE -ne 0 -or $attached.Count -gt 0) {
                    throw "Disposable Compose volume is still attached: $composeVolume"
                }
                docker --context $DockerContext volume rm $ownVolume
                if ($LASTEXITCODE -ne 0) { throw "Disposable Compose volume cleanup failed: $composeVolume" }
            }
        }
        $remainingVolumes = @(docker --context $DockerContext volume ls -q)
        if ($LASTEXITCODE -ne 0) { throw 'Disposable volume readback failed' }
        foreach ($composeVolume in $composeVolumes) {
            if ($remainingVolumes -ccontains ($project + '_' + $composeVolume)) {
                throw "Disposable Compose volume remains after cleanup: $composeVolume"
            }
        }
        $after = SourceHashes
        if ($before.Count -ne $after.Count) { throw 'Source inventory changed during QA' }
        foreach ($key in $before.Keys) { if ($before[$key] -ne $after[$key]) { throw "Source changed during QA: $key" } }
        Write-Output "SOURCE_HASHES_UNCHANGED=$($before.Count)"
    } finally {
        foreach ($key in $previous.Keys) { [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process') }
        if ($junction -and (Test-Path -LiteralPath $junction)) {
            $link = Get-Item -LiteralPath $junction -Force
            if ($link.LinkType -ne 'Junction' -or [IO.Path]::GetFullPath($link.Target) -ne [IO.Path]::GetFullPath($composeHost)) { throw 'Owned QA alias changed; preserve it' }
            Remove-Item -LiteralPath $junction -Force
        }
    }
}
if ($result -ne 0) { throw "Task delivery QA failed: $result ($output)" }
Write-Output "TASK_DELIVERY_GATE=$Gate`:PASS"

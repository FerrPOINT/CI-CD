param(
    [Parameter(Mandatory)][string]$BaseRoot,
    [Parameter(Mandatory)][ValidatePattern('^sha256:[0-9a-f]{64}$')][string]$RustImage,
    [Parameter(Mandatory)][ValidatePattern('^sha256:[0-9a-f]{64}$')][string]$PostgresImage,
    [Parameter(Mandatory)][string]$TargetCache,
    [Parameter(Mandatory)][string]$CargoCache,
    [Parameter(Mandatory)][string]$RustupCache,
    [ValidateSet('full', 'delivery')][string]$Gate = 'full',
    [string]$DockerContext = 'desktop-linux'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$sdk = (Resolve-Path -LiteralPath $BaseRoot).Path
$project = 'sdlc-qa-forge-delivery-' + [guid]::NewGuid().ToString('N').Substring(0, 12)
$compose = Join-Path $repo 'deploy/qa/task-delivery.compose.yml'
$output = Join-Path $repo ('.local/task-delivery-qa/' + $project)
[void](New-Item -ItemType Directory -Path $output -Force)
python -B "$sdk/scripts/verify_base_revision.py" --base $sdk --revision "$repo/.base-revision"
if ($LASTEXITCODE -ne 0) { throw 'Pinned Base validation failed' }
foreach ($image in @($RustImage, $PostgresImage)) {
    $actual = docker --context $DockerContext image inspect $image --format '{{.Id}}'
    if ($LASTEXITCODE -ne 0 -or $actual -ne $image) { throw 'Required pinned QA image unavailable' }
}
function SourceHashes {
    $hashes = [ordered]@{}
    foreach ($entry in @(@{root=$repo; name='CI-CD'; paths=@('backend','deploy','scripts','openapi','.base-revision')},
                        @{root=$sdk; name='services-base'; paths=@('crates','Cargo.toml','Cargo.lock','LICENSE')})) {
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
    CICD_QA_RUST_IMAGE=$RustImage; CICD_QA_POSTGRES_IMAGE=$PostgresImage;
    CICD_QA_TARGET_CACHE=$TargetCache; CICD_QA_CARGO_CACHE=$CargoCache;
    CICD_QA_RUSTUP_CACHE=$RustupCache; CICD_QA_GATE=$Gate
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
    docker --context $DockerContext compose -p $project -f $compose config -q
    if ($LASTEXITCODE -ne 0) { throw 'QA Compose config invalid' }
    docker --context $DockerContext compose -p $project -f $compose build
    if ($LASTEXITCODE -ne 0) { throw 'QA Compose build failed' }
    docker --context $DockerContext compose -p $project -f $compose up --pull never --abort-on-container-exit --exit-code-from qa 2>&1 | Tee-Object -FilePath (Join-Path $output 'compose.log')
    $result = $LASTEXITCODE
} finally {
    try {
        docker --context $DockerContext compose -p $project -f $compose down --remove-orphans 2>&1 | Tee-Object -FilePath (Join-Path $output 'cleanup.log')
        if ($LASTEXITCODE -ne 0) { throw 'Owned Compose cleanup failed' }
        $remaining = docker --context $DockerContext ps -aq --filter "label=com.docker.compose.project=$project"
        if ($LASTEXITCODE -ne 0 -or $remaining) { throw 'Owned containers remain or inventory failed' }
        $networks = docker --context $DockerContext network ls -q --filter "label=com.docker.compose.project=$project"
        if ($LASTEXITCODE -ne 0 -or $networks) { throw 'Owned networks remain or inventory failed' }
        $ownVolume = $project + '_delivery-qa'
        $volume = docker --context $DockerContext volume ls -q --filter "name=^${ownVolume}$"
        if ($LASTEXITCODE -ne 0) { throw 'Disposable volume inventory failed' }
        if ($volume) {
            $owner = docker --context $DockerContext volume inspect $ownVolume --format '{{index .Labels "com.docker.compose.project"}}'
            if ($LASTEXITCODE -ne 0 -or $owner -ne $project) { throw 'Disposable volume ownership mismatch' }
            docker --context $DockerContext volume rm $ownVolume
            if ($LASTEXITCODE -ne 0) { throw 'Disposable delivery volume cleanup failed' }
        }
        $after = SourceHashes
        if ($before.Count -ne $after.Count) { throw 'Source inventory changed during QA' }
        foreach ($key in $before.Keys) { if ($before[$key] -ne $after[$key]) { throw "Source changed during QA: $key" } }
        Write-Output "SOURCE_HASHES_UNCHANGED=$($before.Count)"
    } finally {
        foreach ($key in $previous.Keys) { [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process') }
    }
}
if ($result -ne 0) { throw "Task delivery QA failed: $result ($output)" }
Write-Output "TASK_DELIVERY_GATE=$Gate`:PASS"

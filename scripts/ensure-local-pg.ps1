# Machine PostgreSQL on :5432 only. Never starts Docker.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$psql = @(
    "E:\Programs\PostgreSQL\17\bin\psql.exe",
    "C:\Program Files\PostgreSQL\17\bin\psql.exe",
    "C:\Program Files\PostgreSQL\16\bin\psql.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $psql) {
    $cmd = Get-Command psql -ErrorAction SilentlyContinue
    if ($cmd) { $psql = $cmd.Source }
}

$listening = $false
try {
    $listening = [bool](Get-NetTCPConnection -LocalPort 5432 -State Listen -ErrorAction SilentlyContinue)
} catch {
    $listening = $false
}
if (-not $listening) {
    Write-Output "Local PostgreSQL is not listening on 5432. Start the Windows service (postgresql-x64-17)."
    exit 1
}

$env:PGPASSWORD = "cpm"
$env:PGCLIENTENCODING = "UTF8"
if ($psql) {
    & $psql -w -U cpm -h 127.0.0.1 -d cpm -c "SELECT 1" 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Write-Output "DATABASE_URL=postgres://cpm:cpm@127.0.0.1:5432/cpm"
        exit 0
    }
}

Write-Output "Local PostgreSQL is running on 5432. Create the app role once:"
Write-Output "  psql -U postgres -h 127.0.0.1 -f infra/local-pg.sql"
Write-Output "Then: DATABASE_URL=postgres://cpm:cpm@127.0.0.1:5432/cpm"
Write-Output "Or set DATABASE_URL in .env to an existing local role."
exit 1

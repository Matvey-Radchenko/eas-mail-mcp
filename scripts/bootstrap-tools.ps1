$ErrorActionPreference = 'Stop'

function Invoke-CheckedCommand {
    param([string]$Command, [string[]]$Arguments)

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

Invoke-CheckedCommand rustup @('component', 'add', 'llvm-tools-preview', '--toolchain', '1.95.0')
Invoke-CheckedCommand rustup @('target', 'add', 'x86_64-pc-windows-msvc', '--toolchain', '1.95.0')

$cargoTools = @(
    @{ Command = 'cargo-nextest'; Crate = 'cargo-nextest'; Version = $null },
    @{ Command = 'cargo-deny'; Crate = 'cargo-deny'; Version = $null },
    @{ Command = 'cargo-llvm-cov'; Crate = 'cargo-llvm-cov'; Version = $null },
    @{ Command = 'cargo-mutants'; Crate = 'cargo-mutants'; Version = '27.1.0' }
)

foreach ($tool in $cargoTools) {
    if (Get-Command $tool.Command -ErrorAction SilentlyContinue) {
        continue
    }
    $arguments = @('install', '--locked')
    if ($tool.Version) {
        $arguments += @('--version', $tool.Version)
    }
    $arguments += $tool.Crate
    Invoke-CheckedCommand cargo $arguments
}

if (-not (Get-Command gitleaks -ErrorAction SilentlyContinue)) {
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw 'Install Gitleaks and ensure gitleaks.exe is available in PATH'
    }
    Invoke-CheckedCommand winget @(
        'install', '--exact', '--id', 'Gitleaks.Gitleaks', '--silent',
        '--accept-package-agreements', '--accept-source-agreements'
    )
    $env:PATH = @(
        [Environment]::GetEnvironmentVariable('PATH', 'Machine'),
        [Environment]::GetEnvironmentVariable('PATH', 'User')
    ) -join ';'
}

Invoke-CheckedCommand cargo @('nextest', '--version')
Invoke-CheckedCommand cargo @('deny', '--version')
Invoke-CheckedCommand cargo @('llvm-cov', '--version')
Invoke-CheckedCommand cargo @('mutants', '--version')
Invoke-CheckedCommand gitleaks @('version')

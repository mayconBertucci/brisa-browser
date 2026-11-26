param(
    [switch]$Test,
    [switch]$Offline,
    [switch]$Translation
)

$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath $PSScriptRoot
try {
    $assetDir = Join-Path $PSScriptRoot 'assets'
    New-Item -ItemType Directory -Path $assetDir -Force | Out-Null
    foreach ($name in 'easylist', 'easyprivacy') {
        $destination = Join-Path $assetDir "$name.txt"
        if (-not (Test-Path -LiteralPath $destination)) {
            if ($Offline) { throw "Lista de filtros ausente: $destination" }
            $temporary = "$destination.part"
            Invoke-WebRequest -UseBasicParsing -Uri "https://easylist.to/easylist/$name.txt" -OutFile $temporary
            Move-Item -LiteralPath $temporary -Destination $destination -Force
        }
    }

    $toolDir = Join-Path $PSScriptRoot '.toolchain'
    $translationDir = Join-Path $toolDir 'translation'
    if ($Translation) {
        $nodeDir = Join-Path $env:ProgramFiles 'nodejs'
        $nodePath = Join-Path $nodeDir 'node.exe'
        $npmCli = Join-Path $nodeDir 'node_modules/npm/bin/npm-cli.js'
        if (-not (Test-Path -LiteralPath $nodePath) -or -not (Test-Path -LiteralPath $npmCli)) {
            throw 'Instale o Node.js para Windows com npm antes de preparar a tradução.'
        }
        New-Item -ItemType Directory -Path $translationDir -Force | Out-Null
        Copy-Item -LiteralPath 'src/translation-package-lock.json' -Destination (Join-Path $translationDir 'package-lock.json') -Force
        Copy-Item -LiteralPath 'src/translation-package.json' -Destination (Join-Path $translationDir 'package.json') -Force
        $npmCache = Join-Path $toolDir 'npm-cache'
        $npmFlags = @('--ignore-scripts', '--no-audit', '--no-fund', '--fetch-retries=0', '--cache', $npmCache)
        if ($Offline) { $npmFlags += '--offline' } else { $npmFlags += '--offline=false' }
        & $nodePath $npmCli ci --prefix $translationDir @npmFlags
        if ($LASTEXITCODE -ne 0) { throw 'Não foi possível preparar o Bergamot para tradução local.' }
    }
    if (Test-Path -LiteralPath (Join-Path $translationDir 'node_modules/@browsermt/bergamot-translator')) {
        Copy-Item -LiteralPath 'src/translate.mjs','src/translation-worker.mjs','src/translation-models.json' -Destination $translationDir -Force
    }
    $localCargo = Join-Path $toolDir 'cargo/bin/cargo.exe'
    $llvmDir = Get-ChildItem -LiteralPath $toolDir -Directory -Filter 'llvm-mingw-*' -ErrorAction SilentlyContinue |
        Select-Object -First 1
    $cargo = 'cargo'
    if ((Test-Path -LiteralPath $localCargo) -and $llvmDir) {
        $env:RUSTUP_HOME = Join-Path $toolDir 'rustup'
        $env:CARGO_HOME = Join-Path $toolDir 'cargo'
        $llvmBin = Join-Path $llvmDir.FullName 'bin'
        $env:Path = "$llvmBin;$env:Path"
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $llvmBin 'x86_64-w64-mingw32-clang.exe'
        $env:RUSTFLAGS = '-C linker-flavor=gcc -C link-self-contained=yes'
        $cargo = $localCargo
    }
    $flags = @('--release', '--locked')
    if ($Offline) { $flags += '--offline' }
    if ($Test) {
        & $cargo test @flags
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    & $cargo build @flags
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $loaderDir = Join-Path $toolDir 'webview2'
    $loader = Join-Path $loaderDir 'WebView2Loader.dll'
    if (-not (Test-Path -LiteralPath $loader)) {
        if ($Offline) { throw 'WebView2Loader.dll x64 ausente no cache .toolchain/webview2.' }
        New-Item -ItemType Directory -Path $loaderDir -Force | Out-Null
        $version = '1.0.4370.26'
        $package = Join-Path $loaderDir "microsoft.web.webview2.$version.nupkg"
        $packageUri = "https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/$version/microsoft.web.webview2.$version.nupkg"
        Invoke-WebRequest -UseBasicParsing -Uri $packageUri -OutFile $package
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = [System.IO.Compression.ZipFile]::OpenRead($package)
        try {
            $entry = $archive.GetEntry('runtimes/win-x64/native/WebView2Loader.dll')
            if (-not $entry) { throw 'O SDK não contém o carregador WebView2 x64.' }
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $loader, $true)
        } finally {
            $archive.Dispose()
        }
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $loader
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*Microsoft Corporation*') {
        throw 'O carregador WebView2 não passou na verificação de assinatura da Microsoft.'
    }
    $outputLoader = 'target/release/WebView2Loader.dll'
    if (-not (Test-Path -LiteralPath $outputLoader) -or
        (Get-FileHash -LiteralPath $loader).Hash -ne (Get-FileHash -LiteralPath $outputLoader).Hash) {
        Copy-Item -LiteralPath $loader -Destination $outputLoader -Force
    }
} finally {
    Pop-Location
}

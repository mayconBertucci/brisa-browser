param(
    [switch]$Offline,
    [switch]$SkipBuild,
    [string]$Compiler = (Join-Path $PSScriptRoot '.toolchain/inno/compiler/ISCC.exe'),
    [string]$CodeSigningThumbprint,
    [string]$SignTool = 'signtool.exe'
)

$ErrorActionPreference = 'Stop'

function Sign-BrisaFile([string]$Path) {
    & $SignTool sign /sha1 $CodeSigningThumbprint /fd SHA256 /tr 'https://timestamp.digicert.com' /td SHA256 $Path
    if ($LASTEXITCODE -ne 0) { throw "Falha ao assinar $Path." }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $CodeSigningThumbprint) {
        throw "A assinatura de $Path não foi validada."
    }
}

Push-Location -LiteralPath $PSScriptRoot
try {
    if (!(Test-Path -LiteralPath $Compiler)) {
        $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if (!$command) { throw 'Instale o Inno Setup 6 e indique o caminho do ISCC.exe com -Compiler.' }
        $Compiler = $command.Source
    }
    $bootstrapper = '.toolchain/webview2/MicrosoftEdgeWebview2Setup.exe'
    if (!(Test-Path -LiteralPath $bootstrapper)) {
        if ($Offline) { throw 'O bootstrapper do WebView2 está ausente do cache.' }
        New-Item -ItemType Directory -Path '.toolchain/webview2' -Force | Out-Null
        Invoke-WebRequest -UseBasicParsing -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile "$bootstrapper.part"
        Move-Item -LiteralPath "$bootstrapper.part" -Destination $bootstrapper -Force
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $bootstrapper
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*Microsoft Corporation*') {
        throw 'O bootstrapper do WebView2 não passou na verificação de assinatura.'
    }

    if (!$SkipBuild) {
        $buildFlags = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', './build.ps1', '-Translation')
        if ($Offline) { $buildFlags += '-Offline' }
        & powershell.exe @buildFlags
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    $projectVersion = [regex]::Match((Get-Content -LiteralPath 'Cargo.toml' -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
    if ((Get-Item -LiteralPath 'target/release/brisa.exe').VersionInfo.ProductVersion -ne $projectVersion) {
        throw 'Compile o binário com a versão atual antes de empacotar.'
    }
    if ($CodeSigningThumbprint) {
        $CodeSigningThumbprint = $CodeSigningThumbprint -replace '\s', ''
        if ($CodeSigningThumbprint -notmatch '^[a-fA-F0-9]{40}$') { throw 'Informe a impressão digital do certificado de assinatura de código.' }
        Sign-BrisaFile 'target/release/brisa.exe'
    }

    $output = 'target/distribution'
    $notices = Join-Path $output 'notices'
    New-Item -ItemType Directory -Path $notices -Force | Out-Null
    Copy-Item -LiteralPath 'installer/TERCEIROS.md' -Destination $notices -Force
    Copy-Item -LiteralPath 'vendor/windows-webview/license-mit','vendor/windows-webview/license-apache-2.0' -Destination $notices -Force
    $sdk = Get-ChildItem -LiteralPath '.toolchain/webview2' -Filter '*1.0.4370.26.nupkg' -File | Select-Object -First 1
    if (!$sdk) {
        if ($Offline) { throw 'O pacote do SDK WebView2 está ausente do cache.' }
        $destination = '.toolchain/webview2/microsoft.web.webview2.1.0.4370.26.nupkg'
        $uri = 'https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/1.0.4370.26/microsoft.web.webview2.1.0.4370.26.nupkg'
        Invoke-WebRequest -UseBasicParsing -Uri $uri -OutFile $destination
        $sdk = Get-Item -LiteralPath $destination
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($sdk.FullName)
    try {
        foreach ($file in 'LICENSE.txt','NOTICE.txt') {
            $entry = $archive.GetEntry($file)
            $licensePath = Join-Path (Resolve-Path -LiteralPath $notices).Path "WebView2-$file"
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $licensePath, $true)
        }
    } finally {
        $archive.Dispose()
    }

    & $Compiler './installer/brisa.iss'
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $installer = "Brisa-$projectVersion-Windows-x64.exe"
    if ($CodeSigningThumbprint) { Sign-BrisaFile (Join-Path $output $installer) }
    $hash = (Get-FileHash -LiteralPath (Join-Path $output $installer) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $installer" | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding ASCII
    $kit = Join-Path (Resolve-Path -LiteralPath $output).Path "Brisa-$projectVersion-GitHub-Pages.zip"
    $temporaryKit = "$kit.part"
    if (Test-Path -LiteralPath $temporaryKit) { Remove-Item -LiteralPath $temporaryKit }
    $archive = [System.IO.Compression.ZipFile]::Open($temporaryKit, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        $files = [ordered]@{
            'docs/.nojekyll' = 'docs/.nojekyll'
            'docs/brisa.ico' = 'docs/brisa.ico'
            'docs/brisa.png' = 'docs/brisa.png'
            'docs/download.js' = 'docs/download.js'
            'docs/styles.css' = 'docs/styles.css'
            'PUBLICAR.md' = 'installer/PUBLICAR.md'
            "release/$installer" = (Join-Path $output $installer)
            'release/SHA256SUMS.txt' = (Join-Path $output 'SHA256SUMS.txt')
        }
        foreach ($file in $files.GetEnumerator()) {
            [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive, (Resolve-Path -LiteralPath $file.Value).Path, $file.Key
            )
        }
        $index = $archive.CreateEntry('docs/index.html')
        $writer = [System.IO.StreamWriter]::new($index.Open(), [System.Text.UTF8Encoding]::new($false))
        try {
            $writer.Write((Get-Content -LiteralPath 'docs/index.html' -Raw).Replace('../target/distribution/', '../release/'))
        } finally {
            $writer.Dispose()
        }
    } finally {
        $archive.Dispose()
    }
    Move-Item -LiteralPath $temporaryKit -Destination $kit -Force
    Get-Item -LiteralPath (Join-Path $output $installer), $kit
} finally {
    Pop-Location
}

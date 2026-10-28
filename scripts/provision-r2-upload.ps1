param(
    [Parameter(Mandatory = $true)]
    [string]$Endpoint,
    [switch]$EnableForCurrentUser
)
# PowerShell 7, Windows only. Never prints or writes a plaintext credential.
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$configuration = Get-Content -LiteralPath (Join-Path $workspace 'config/app.json') -Raw | ConvertFrom-Json
$Endpoint = $Endpoint.TrimEnd('/')
$parsedEndpoint = [Uri]$Endpoint
if ($parsedEndpoint.Scheme -ne 'https' -or $parsedEndpoint.IsLoopback -or $parsedEndpoint.UserInfo -or $parsedEndpoint.AbsolutePath -ne '/' -or $parsedEndpoint.Query -or $parsedEndpoint.Fragment) { throw 'Use the deployed HTTPS Worker origin.' }
if ($EnableForCurrentUser -and (Get-Process CurrentActivity -ErrorAction SilentlyContinue)) { throw 'Exit CurrentActivity before configuring its local profile.' }
$workerCli = Join-Path $workspace 'cloudflare/node_modules/wrangler/bin/wrangler.js'
if (-not (Test-Path -LiteralPath $workerCli -PathType Leaf)) { throw 'Install the Cloudflare project dependencies first.' }
$randomBytes = [Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
$uploadToken = [Convert]::ToHexString($randomBytes)
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Command node).Source
    $start.WorkingDirectory = Join-Path $workspace 'cloudflare'
    foreach ($argument in @($workerCli,'secret','put','ICON_UPLOAD_TOKEN')) { $start.ArgumentList.Add($argument) }
    $start.Environment['WRANGLER_SEND_METRICS'] = 'false'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    $standardOutput = $process.StandardOutput.ReadToEndAsync()
    $standardError = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.WriteLine($uploadToken)
    $process.StandardInput.Close()
    $process.WaitForExit()
    $null = $standardOutput.GetAwaiter().GetResult()
    $errorOutput = $standardError.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) { throw ('Worker secret provisioning failed: ' + $errorOutput.Replace($uploadToken,'[redacted]')) }
    Add-Type -AssemblyName System.Security
    $plainBytes = [Text.Encoding]::UTF8.GetBytes($uploadToken)
    $entropy = [Text.Encoding]::UTF8.GetBytes($Endpoint)
    $encrypted = [Security.Cryptography.ProtectedData]::Protect($plainBytes,$entropy,[Security.Cryptography.DataProtectionScope]::CurrentUser)
    $localProof = Join-Path $workspace '.local/provisioned-icon-upload.dpapi'
    [IO.Directory]::CreateDirectory((Split-Path $localProof)) | Out-Null
    [IO.File]::WriteAllBytes($localProof,$encrypted)
    if ($EnableForCurrentUser) {
        $profileDirectory = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'CurrentActivity'
        [IO.Directory]::CreateDirectory($profileDirectory) | Out-Null
        $settingsPath = Join-Path $profileDirectory 'settings.json'
        $credentialPath = Join-Path $profileDirectory 'icon-upload-token.dpapi'
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmssfff'
        $document = if (Test-Path -LiteralPath $settingsPath) {
            $existing = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json -AsHashtable
            if ($existing -isnot [Collections.IDictionary] -or $existing.settings -isnot [Collections.IDictionary]) { throw 'Existing settings require review; they were preserved.' }
            Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $profileDirectory "settings.pre-r2-$stamp.json")
            $existing
        } else { @{ settings = @{}; descriptions = @{} } }
        if (Test-Path -LiteralPath $credentialPath) { Copy-Item -LiteralPath $credentialPath -Destination (Join-Path $profileDirectory "icon-upload-token.pre-r2-$stamp.dpapi") }
        $document.settings.iconUpload = @{
            enabled = $true; consentGranted = $true; endpoint = $Endpoint
            publicBase = [string]$configuration.iconUploadPublicBase
        }
        [IO.File]::WriteAllBytes($credentialPath,$encrypted)
        $temporarySettings = Join-Path $profileDirectory 'settings.r2.tmp'
        $document | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $temporarySettings -Encoding utf8NoBOM
        Move-Item -LiteralPath $temporarySettings -Destination $settingsPath -Force
    }
    [pscustomobject]@{ WorkerSecretConfigured=$true; DesktopConfigured=[bool]$EnableForCurrentUser; CredentialProtection='Windows DPAPI'; SharingStartsOff=$true } | ConvertTo-Json
} finally {
    [Array]::Clear($randomBytes)
    if ($plainBytes) { [Array]::Clear($plainBytes) }
    $uploadToken = $null
}

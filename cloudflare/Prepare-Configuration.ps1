param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$')]
    [string]$BucketName,
    [ValidatePattern('^[a-z0-9][a-z0-9-]{0,61}[a-z0-9]$')]
    [string]$WorkerName = 'your-icon-worker',
    [string]$PublicImageBase = ''
)
$ErrorActionPreference = 'Stop'
if ($PublicImageBase) {
    $baseUri = $null
    if (![Uri]::TryCreate($PublicImageBase, [UriKind]::Absolute, [ref]$baseUri)) {
        throw 'PublicImageBase must be a public HTTPS DNS origin.'
    }
    $baseHost = $baseUri.DnsSafeHost.ToLowerInvariant().TrimEnd('.')
    $parsedIp = $null
    $isIp = [Net.IPAddress]::TryParse($baseHost, [ref]$parsedIp)
    $isLocal = $baseHost -match '(^|\.)(localhost|local|internal|lan|home|test|invalid)$'
    if ($baseUri.Scheme -ne 'https' -or $baseUri.UserInfo -or $baseUri.Query -or $baseUri.Fragment -or
        $baseUri.AbsolutePath -ne '/' -or !$baseUri.IsDefaultPort -or !$baseHost.Contains('.') -or $isIp -or $isLocal) {
        throw 'PublicImageBase must be a public HTTPS DNS origin without credentials, path, query or fragment.'
    }
    $PublicImageBase = $baseUri.GetLeftPart([UriPartial]::Authority)
    if ($PublicImageBase.Length + 75 -gt 300) { throw 'PublicImageBase would exceed the Discord image URL limit.' }
}
$target = Join-Path $PSScriptRoot 'wrangler.jsonc'
if (Test-Path -LiteralPath $target) {
    throw 'wrangler.jsonc already exists. Review and update it yourself to preserve existing configuration.'
}
$configuration = [ordered]@{
    name = $WorkerName
    main = 'src/worker.ts'
    compatibility_date = '2026-10-27'
    workers_dev = $true
    observability = @{ enabled = $false }
    r2_buckets = @(@{ binding = 'ICONS'; bucket_name = $BucketName })
    vars = @{ PUBLIC_IMAGE_BASE = $PublicImageBase }
}
$configuration | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $target -Encoding utf8
Write-Output 'Prepared local wrangler.jsonc only. No login, secret creation, bucket access, or deployment occurred.'
Write-Output 'Review the configuration. Blank PUBLIC_IMAGE_BASE uses the deployed Worker origin for public image reads.'
Write-Output 'Then, from this directory, run these commands yourself when ready:'
Write-Output '  $env:WRANGLER_SEND_METRICS = "false"'
Write-Output '  npm ci --ignore-scripts'
Write-Output '  npx wrangler login'
Write-Output '  npx wrangler secret put ICON_UPLOAD_TOKEN'
Write-Output '  npm run deploy'
Write-Output 'Enter a new random token of 32–1024 printable ASCII characters without spaces at the secret prompt; do not paste Cloudflare credentials into CurrentActivity.'

param([string]$SdkDirectory = $env:DISCORD_SOCIAL_SDK_DIR, [switch]$Preview)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$tauriArguments = @('run', 'tauri', '--', 'build')
$temporaryConfig = $null
$buildConfiguration = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../config/app.json') -Raw | ConvertFrom-Json
if (-not $env:DISCORD_CLIENT_ID) { $env:DISCORD_CLIENT_ID = [string]$buildConfiguration.discordClientId }
try {
    if ($Preview) {
        if ($env:DISCORD_SOCIAL_SDK_DIR) { throw 'Remove DISCORD_SOCIAL_SDK_DIR for a predictable SDK-free preview build.' }
        $env:CURRENTACTIVITY_REQUIRE_SDK = '0'
        Write-Host 'Building an SDK-free preview installer. This build CANNOT publish Discord activity.'
    } else {
        if (-not $SdkDirectory) { throw 'Set DISCORD_SOCIAL_SDK_DIR to the official SDK archive root. See docs/DISCORD_SDK.md.' }
        $resolvedSdk = (Resolve-Path -LiteralPath $SdkDirectory).Path
        $runtimeDll = Join-Path $resolvedSdk 'bin/release/discord_partner_sdk.dll'
        if (-not (Test-Path -LiteralPath $runtimeDll -PathType Leaf)) { throw 'Official SDK runtime DLL is missing.' }
        $sdkNotices = Join-Path $resolvedSdk 'License-Notices.txt'
        if (-not (Test-Path -LiteralPath $sdkNotices -PathType Leaf)) { throw 'Official SDK License-Notices.txt is missing; preserve its bundled third-party notices.' }
        $numericApplicationId = [UInt64]0
        if (-not $env:DISCORD_CLIENT_ID -or $env:DISCORD_CLIENT_ID -notmatch '^\d+$' -or -not [UInt64]::TryParse($env:DISCORD_CLIENT_ID, [ref]$numericApplicationId) -or $numericApplicationId -eq 0) { throw 'Set DISCORD_CLIENT_ID to your public positive numeric Discord application identifier.' }
        $env:DISCORD_SOCIAL_SDK_DIR = $resolvedSdk
        $env:CURRENTACTIVITY_REQUIRE_SDK = '1'
        $resources = @{}
        $resources[$runtimeDll] = 'discord_partner_sdk.dll'
        $resources[$sdkNotices] = 'notices/Discord-Social-SDK-License-Notices.txt'
        $temporaryConfig = Join-Path $env:TEMP ('currentactivity-bundle-' + [Guid]::NewGuid().ToString('N') + '.json')
        @{ bundle = @{ resources = $resources } } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $temporaryConfig -Encoding utf8
        $tauriArguments += @('--config', $temporaryConfig)
    }
    & npm @tauriArguments
    if ($LASTEXITCODE -ne 0) { throw "Tauri packaging failed (exit $LASTEXITCODE)." }
} finally {
    if ($temporaryConfig -and (Test-Path -LiteralPath $temporaryConfig)) { Remove-Item -LiteralPath $temporaryConfig }
}

param(
  [string]$PlaywrightModule = "$env:TEMP\currentactivity-visual\node_modules\playwright",
  [switch]$SdkEnabled,
  [switch]$IconTransitions,
  [string]$UploadEndpoint = '',
  [string]$PublicImageBase = '',
  [string]$UploadTokenDpapiPath = '',
  [ValidateRange(0,120)][int]$ObserveSeconds = 0
)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$executable = Join-Path $workspace 'src-tauri\target\debug\CurrentActivity.exe'
if (-not (Test-Path -LiteralPath $executable)) { throw 'Build the debug Windows executable before running this smoke test.' }
if (-not (Test-Path -LiteralPath $PlaywrightModule)) { throw 'Install Playwright into a temporary directory and provide -PlaywrightModule.' }
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('CurrentActivity-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$fixture = Join-Path $scratch 'CurrentActivitySmokeFixture.exe'
$source = Join-Path $scratch 'fixture.cs'
@'
using System;
using System.Windows.Forms;
using System.Reflection;
using System.Runtime.InteropServices;
[assembly: AssemblyProduct("CurrentActivity Test App")]
[assembly: AssemblyDescription("CurrentActivity Test App")]
class Fixture {
  [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr hWnd, int command);
  [STAThread] static void Main() {
    Application.EnableVisualStyles();
    var first = new Form { Text = "CurrentActivity owned smoke fixture", Width = 320, Height = 140, ControlBox = false, ShowInTaskbar = true };
    var second = new Form { Text = "CurrentActivity owned smoke fixture", Width = 320, Height = 140, ControlBox = false, ShowInTaskbar = true };
    first.FormClosing += delegate(object sender, FormClosingEventArgs e) { e.Cancel = true; };
    second.FormClosing += delegate(object sender, FormClosingEventArgs e) { e.Cancel = true; };
    first.Load += delegate { second.Show(); };
    var timer = new Timer { Interval = 250 };
    timer.Tick += delegate { ShowWindow(first.Handle, 5); ShowWindow(second.Handle, 5); timer.Stop(); };
    timer.Start();
    Application.Run(first);
  }
}
'@ | Set-Content -LiteralPath $source
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
& $compiler /nologo /target:winexe "/out:$fixture" /reference:System.Windows.Forms.dll $source
if ($LASTEXITCODE -ne 0) { throw 'Fixture compilation failed.' }
$originalEnvironment = @{}
foreach ($name in @('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS','WEBVIEW2_USER_DATA_FOLDER','CURRENTACTIVITY_TEST_DATA_DIR','CURRENTACTIVITY_SMOKE_UPLOAD_TOKEN')) {
  $originalEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CurrentActivitySmokeWindow {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
}
'@
$appProcess = $null; $fixtureProcess = $null; $viteProcess = $null; $secondProcess = $null
try {
  if (Get-Process CurrentActivity -ErrorAction SilentlyContinue) { throw 'Close existing CurrentActivity instances before smoke testing; no user process will be stopped.' }
  $probe = [Net.Sockets.TcpClient]::new()
  try { $occupied = $probe.ConnectAsync('127.0.0.1',9224).Wait(250) -and $probe.Connected } catch { $occupied = $false } finally { $probe.Dispose() }
  if ($occupied) { throw 'CDP port 9224 is already in use; refusing to attach to an unrelated process.' }
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9224'
  $env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $scratch 'webview'
  $env:CURRENTACTIVITY_TEST_DATA_DIR = Join-Path $scratch 'data'
  $env:CURRENTACTIVITY_SMOKE_UPLOAD_TOKEN = $null
  if (-not (Test-NetConnection 127.0.0.1 -Port 1420 -InformationLevel Quiet -WarningAction SilentlyContinue)) {
    $viteProcess = Start-Process -FilePath (Get-Command node).Source -ArgumentList @('node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1420') -WorkingDirectory $workspace -WindowStyle Hidden -PassThru
  }
  $fixtureProcess = Start-Process -FilePath $fixture -WindowStyle Hidden -PassThru
  $appProcess = Start-Process -FilePath $executable -WorkingDirectory $workspace -WindowStyle Hidden -PassThru
  $smokeMode = if ($SdkEnabled) { 'sdk' } else { 'offline' }
  if ($UploadEndpoint) {
    if (-not $SdkEnabled -or -not $PublicImageBase -or -not $UploadTokenDpapiPath) { throw 'R2 smoke requires SDK mode, public image base and a DPAPI token file.' }
    Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
    $encryptedToken = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $UploadTokenDpapiPath))
    $entropy = [Text.Encoding]::UTF8.GetBytes($UploadEndpoint.TrimEnd('/'))
    $clearTokenBytes = [Security.Cryptography.ProtectedData]::Unprotect($encryptedToken,$entropy,[Security.Cryptography.DataProtectionScope]::CurrentUser)
    try { $env:CURRENTACTIVITY_SMOKE_UPLOAD_TOKEN = [Text.Encoding]::UTF8.GetString($clearTokenBytes) } finally { [Array]::Clear($clearTokenBytes,0,$clearTokenBytes.Length) }
  }
  try {
    & node (Join-Path $PSScriptRoot 'native-smoke.mjs') $PlaywrightModule $fixtureProcess.Id $workspace $smokeMode $ObserveSeconds ([bool]$IconTransitions).ToString().ToLowerInvariant() $UploadEndpoint $PublicImageBase
  } finally { $env:CURRENTACTIVITY_SMOKE_UPLOAD_TOKEN = $null }
  if ($LASTEXITCODE -ne 0) { throw 'Native smoke assertions failed.' }
  $appProcess.Refresh()
  if ($appProcess.HasExited) { throw 'Native host exited when the CDP client disconnected.' }
  $ownedWindow = $appProcess.MainWindowHandle
  if ($ownedWindow -eq [IntPtr]::Zero -or -not [CurrentActivitySmokeWindow]::IsWindowVisible($ownedWindow)) { throw 'Owned main window must be visible before close-to-tray test.' }
  if (-not $appProcess.CloseMainWindow()) { throw 'WM_CLOSE could not be sent to the owned main window.' }
  $hideDeadline = [DateTime]::UtcNow.AddSeconds(5)
  while ([CurrentActivitySmokeWindow]::IsWindowVisible($ownedWindow) -and [DateTime]::UtcNow -lt $hideDeadline) { Start-Sleep -Milliseconds 100 }
  $appProcess.Refresh()
  if ($appProcess.HasExited -or [CurrentActivitySmokeWindow]::IsWindowVisible($ownedWindow)) { throw 'Close-to-tray did not keep the owned host alive with its main window hidden.' }
  $secondProcess = Start-Process -FilePath $executable -WorkingDirectory $workspace -WindowStyle Hidden -PassThru
  $showDeadline = [DateTime]::UtcNow.AddSeconds(10)
  while (-not [CurrentActivitySmokeWindow]::IsWindowVisible($ownedWindow) -and [DateTime]::UtcNow -lt $showDeadline) { Start-Sleep -Milliseconds 100 }
  if (-not [CurrentActivitySmokeWindow]::IsWindowVisible($ownedWindow)) { throw 'A second launch failed to reopen the existing owned main window.' }
  if (-not $secondProcess.WaitForExit(5000)) { throw 'Second launch did not exit after handing off to the existing instance.' }
  $resultName = if ($SdkEnabled) { 'native-sdk-smoke-results.json' } else { 'native-smoke-results.json' }
  $resultPath = Join-Path (Join-Path $workspace 'docs') $resultName
  $results = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
  $results.checks = @($results.checks | Where-Object { $_ -notlike 'Window core close/show*' }) + @('Owned HWND WM_CLOSE hides to tray while the native host remains alive','Second executable launch reopens the existing main window and exits')
  $results | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $resultPath
  Write-Output 'Native OS checks passed: close-to-tray and single-instance reopen.'
} finally {
  foreach ($owned in @($appProcess,$fixtureProcess,$viteProcess,$secondProcess)) {
    if ($null -ne $owned -and -not $owned.HasExited) { Stop-Process -Id $owned.Id -ErrorAction SilentlyContinue }
  }
  # WebView2 subprocesses can outlive their owned host briefly. Match only this
  # fresh test profile; never stop unrelated user WebView2 processes.
  Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf($scratch,[StringComparison]::OrdinalIgnoreCase) -ge 0 } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 500
  foreach ($entry in $originalEnvironment.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, 'Process') }
  # Only this exact, freshly created directory can be removed.
  $resolvedScratch = [IO.Path]::GetFullPath($scratch)
  $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
  if ($resolvedScratch.StartsWith($resolvedTemp,[StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolvedScratch -Leaf).StartsWith('CurrentActivity-smoke-')) {
    Remove-Item -LiteralPath $resolvedScratch -Recurse -Force -ErrorAction SilentlyContinue
  }
}


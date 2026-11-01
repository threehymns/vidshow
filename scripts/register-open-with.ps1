<#
.SYNOPSIS
    Adds vidshow to Explorer's "Open with" list for video files, for the current user only.

.DESCRIPTION
    Writes the registry entries Windows uses to build the "Open with" menu:
      HKCU\Software\Classes\vidshow.Video                       (the ProgID: icon and command)
      HKCU\Software\Classes\Applications\vidshow.exe            (the app, its name and types)
      HKCU\Software\Classes\.<ext>\OpenWithProgids              (one per extension)
    No administrator rights are needed, and nothing outside the user's own hive is touched.
    Pass -OpenWithDefault to also make vidshow the default handler for the listed extensions.

.EXAMPLE
    .\register-open-with.ps1 -ExePath 'C:\Tools\vidshow\vidshow.exe'

.EXAMPLE
    # After `cargo build --release`, register the build output for the current user.
    .\register-open-with.ps1
#>
[CmdletBinding()]
param(
    # The vidshow.exe to register. Defaults to the release build beside this script's folder.
    [string]$ExePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\vidshow.exe'),

    # Make vidshow the default app for these extensions, not just an "Open with" choice.
    [switch]$OpenWithDefault,

    [string[]]$Extensions = @(
        'mp4', 'm4v', 'mov', 'mkv', 'webm', 'avi', 'wmv', 'asf', 'flv', 'f4v',
        'mpg', 'mpeg', 'm2v', 'ts', 'm2ts', 'mts', 'ogv', 'ogm', '3gp', '3g2',
        'vob', 'mxf', 'divx', 'rm', 'rmvb', 'y4m'
    )
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    throw "vidshow.exe was not found at '$ExePath'. Build it first (cargo build --release), or pass -ExePath."
}
$exe = (Resolve-Path -LiteralPath $ExePath).Path
$progId = 'vidshow.Video'
$classes = 'HKCU:\Software\Classes'
$command = '"' + $exe + '" "%1"'

function Set-DefaultValue {
    param([string]$Path, [string]$Value)
    if (-not (Test-Path -LiteralPath $Path)) {
        New-Item -Path $Path -Force | Out-Null
    }
    Set-ItemProperty -Path $Path -Name '(default)' -Value $Value
}

# The ProgID: what a file's type points at, with its icon and launch command.
Set-DefaultValue -Path (Join-Path $classes $progId) -Value 'Video (vidshow)'
Set-DefaultValue -Path (Join-Path $classes "$progId\DefaultIcon") -Value "$exe,0"
Set-DefaultValue -Path (Join-Path $classes "$progId\shell\open\command") -Value $command

New-Item -Path (Join-Path $classes "$progId\shell\open") -Force | Out-Null
Set-ItemProperty -Path (Join-Path $classes "$progId\shell\open") -Name 'FriendlyAppName' -Value 'vidshow'

# The Applications entry: the "Open with" list and Choose another app read this.
$appKey = Join-Path $classes 'Applications\vidshow.exe'
Set-DefaultValue -Path $appKey -Value ''
Set-ItemProperty -Path $appKey -Name 'FriendlyAppName' -Value 'vidshow'
Set-DefaultValue -Path (Join-Path $appKey 'shell\open\command') -Value $command
New-Item -Path (Join-Path $appKey 'SupportedTypes') -Force | Out-Null

foreach ($extension in $Extensions) {
    $ext = $extension.TrimStart('.')
    $extKey = Join-Path $classes ".$ext"
    New-Item -Path $extKey -Force | Out-Null

    $openWith = Join-Path $extKey 'OpenWithProgids'
    New-Item -Path $openWith -Force | Out-Null
    New-ItemProperty -Path $openWith -Name $progId -Value ([byte[]]@()) -PropertyType Binary -Force | Out-Null

    New-ItemProperty -Path (Join-Path $appKey 'SupportedTypes') -Name ".$ext" -Value '' -PropertyType String -Force | Out-Null

    if ($OpenWithDefault) {
        Set-DefaultValue -Path $extKey -Value $progId
    }
}

# Tell Explorer the associations changed, so the menu updates without a sign-out.
try {
    Add-Type -Namespace Vidshow -Name Shell -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, int flags, System.IntPtr item1, System.IntPtr item2);
'@
    [Vidshow.Shell]::SHChangeNotify(0x08000000, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
}
catch {
    Write-Warning "Registered, but Explorer could not be told to refresh: $($_.Exception.Message)"
}

Write-Host "vidshow is registered for $($Extensions.Count) video extensions: $exe"
if ($OpenWithDefault) {
    Write-Host 'vidshow is now the default app for those extensions.'
} else {
    Write-Host 'Right-click a video and choose "Open with" (or choose another app) to pick vidshow.'
}

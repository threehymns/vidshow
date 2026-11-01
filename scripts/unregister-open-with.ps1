<#
.SYNOPSIS
    Removes the "Open with" entries register-open-with.ps1 added for the current user.
#>
[CmdletBinding()]
param(
    [string[]]$Extensions = @(
        'mp4', 'm4v', 'mov', 'mkv', 'webm', 'avi', 'wmv', 'asf', 'flv', 'f4v',
        'mpg', 'mpeg', 'm2v', 'ts', 'm2ts', 'mts', 'ogv', 'ogm', '3gp', '3g2',
        'vob', 'mxf', 'divx', 'rm', 'rmvb', 'y4m'
    )
)

$ErrorActionPreference = 'Stop'
$progId = 'vidshow.Video'
$classes = 'HKCU:\Software\Classes'

foreach ($extension in $Extensions) {
    $ext = $extension.TrimStart('.')
    $openWith = Join-Path $classes ".$ext\OpenWithProgids"
    if (Test-Path -LiteralPath $openWith) {
        Remove-ItemProperty -Path $openWith -Name $progId -ErrorAction SilentlyContinue
    }
    # Only remove a default this script set; other apps' defaults are left alone.
    $extKey = Join-Path $classes ".$ext"
    if (Test-Path -LiteralPath $extKey) {
        $current = (Get-ItemProperty -Path $extKey -Name '(default)' -ErrorAction SilentlyContinue).'(default)'
        if ($current -eq $progId) {
            Remove-ItemProperty -Path $extKey -Name '(default)' -ErrorAction SilentlyContinue
        }
    }
}

foreach ($path in @(
    (Join-Path $classes $progId),
    (Join-Path $classes 'Applications\vidshow.exe')
)) {
    if (Test-Path -LiteralPath $path) {
        Remove-Item -Path $path -Recurse -Force
    }
}

try {
    Add-Type -Namespace Vidshow -Name Shell -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, int flags, System.IntPtr item1, System.IntPtr item2);
'@
    [Vidshow.Shell]::SHChangeNotify(0x08000000, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
}
catch {
    Write-Warning "Unregistered, but Explorer could not be told to refresh: $($_.Exception.Message)"
}

Write-Host 'vidshow was removed from the Open with menu.'

param(
  [Parameter(Mandatory=$true)][int]$AppPid,
  [Parameter(Mandatory=$true)][string]$Destination
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SubscriptionWindow {
  [StructLayout(LayoutKind.Sequential)]
  public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr window, out Rect rect);
  [DllImport("user32.dll")]
  public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
'@
$window = (Get-Process -Id $AppPid).MainWindowHandle
if ($window -eq [IntPtr]::Zero) { throw 'The installed app has no visible window.' }
$rect = [SubscriptionWindow+Rect]::new()
if (-not [SubscriptionWindow]::GetWindowRect($window, [ref]$rect)) { throw 'The native window bounds are unavailable.' }
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
if ($width -le 0 -or $height -le 0 -or $width -gt 8192 -or $height -gt 8192) { throw 'The native window bounds are invalid.' }
$bitmap = [System.Drawing.Bitmap]::new($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
  $dc = $graphics.GetHdc()
  try {
    if (-not [SubscriptionWindow]::PrintWindow($window, $dc, 2)) { throw 'The native window capture failed.' }
  } finally { $graphics.ReleaseHdc($dc) }
  $bitmap.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
  $graphics.Dispose()
  $bitmap.Dispose()
}

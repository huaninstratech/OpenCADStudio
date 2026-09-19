Add-Type @"
using System;
using System.Runtime.InteropServices;
public class C2 {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static void Click(IntPtr h, int x, int y) {
    IntPtr lp = (IntPtr)((y << 16) | (x & 0xFFFF));
    PostMessage(h, 0x0200, (IntPtr)0, lp);
    PostMessage(h, 0x0201, (IntPtr)0x0001, lp);
    PostMessage(h, 0x0202, (IntPtr)0, lp);
  }
}
"@
$proc = Get-Process OpenCADStudio -ErrorAction Stop | Select-Object -First 1
[C2]::Click($proc.MainWindowHandle, 1223, 826)
Write-Output "clicked download"

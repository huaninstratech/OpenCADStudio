Add-Type @"
using System;
using System.Runtime.InteropServices;
public class M {
  [StructLayout(LayoutKind.Sequential)]
  public struct INPUT { public uint type; public InputUnion u; }
  [StructLayout(LayoutKind.Explicit)]
  public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)]
  public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  public static void MoveTo(int x, int y) {
    var inputs = new INPUT[1];
    inputs[0].type = 0; inputs[0].u.mi.dwFlags = 0x0001; inputs[0].u.mi.dx = x*65535/2047; inputs[0].u.mi.dy = y*65535/1279;
    SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
[M]::MoveTo(960, 600)
Start-Sleep -Milliseconds 1200
[M]::MoveTo(960, 600)
Write-Output "mouse parked at canvas center"

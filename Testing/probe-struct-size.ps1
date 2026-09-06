$ErrorActionPreference = 'Continue'
# Probe: INPUT struct size A/B test for SendInput on this machine
# Struct A: union contains MOUSEINPUT(32) -> INPUT = 40 bytes (the win32 x64-correct layout)
# Struct B: union contains KEYBDINPUT(24)+long pad -> INPUT = 32 bytes (used by probe-driverless2/round2/round3)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SizeProbe {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    // A: full union -> INPUT 40
    [StructLayout(LayoutKind.Explicit)] public struct UNION_A { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT_A { public uint type; public UNION_A u; }
    // B: kb-only union -> INPUT 32
    [StructLayout(LayoutKind.Explicit)] public struct UNION_B { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT_B { public uint type; public UNION_B u; }
    [DllImport("user32.dll")] static extern uint SendInputA(uint n, INPUT_A[] inputs, int size);
    [DllImport("user32.dll", EntryPoint="SendInput")] static extern uint SendInputB(uint n, INPUT_B[] inputs, int size);
    [DllImport("kernel32.dll")] public static extern uint GetLastError();
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    const uint KEYEVENTF_EXTENDEDKEY = 0x1;
    const uint KEYEVENTF_KEYUP = 0x2;
    const uint KEYEVENTF_SCANCODE = 0x8;
    const uint INPUT_KEYBOARD = 1;
    public static uint A_RAltDown() { INPUT_A i = new INPUT_A(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = 0x38; i.u.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY; return SendInputA(1, new INPUT_A[] { i }, Marshal.SizeOf(typeof(INPUT_A))); }
    public static uint A_RAltUp() { INPUT_A i = new INPUT_A(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = 0x38; i.u.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP; return SendInputA(1, new INPUT_A[] { i }, Marshal.SizeOf(typeof(INPUT_A))); }
    public static uint B_RAltDown() { INPUT_B i = new INPUT_B(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = 0x38; i.u.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY; return SendInputB(1, new INPUT_B[] { i }, Marshal.SizeOf(typeof(INPUT_B))); }
    public static uint B_RAltUp() { INPUT_B i = new INPUT_B(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = 0; i.u.ki.wScan = 0x38; i.u.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP; return SendInputB(1, new INPUT_B[] { i }, Marshal.SizeOf(typeof(INPUT_B))); }
    public static uint A_Batch4_WinH() {
        INPUT_A[] list = new INPUT_A[4];
        list[0] = MkA(0x5B, 0); list[1] = MkA(0x48, 0); list[2] = MkA(0x48, KEYEVENTF_KEYUP); list[3] = MkA(0x5B, KEYEVENTF_KEYUP);
        return SendInputA((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT_A)));
    }
    public static uint B_Batch4_WinH() {
        INPUT_B[] list = new INPUT_B[4];
        list[0] = MkB(0x5B, 0); list[1] = MkB(0x48, 0); list[2] = MkB(0x48, KEYEVENTF_KEYUP); list[3] = MkB(0x5B, KEYEVENTF_KEYUP);
        return SendInputB((uint)list.Length, list, Marshal.SizeOf(typeof(INPUT_B)));
    }
    static INPUT_A MkA(ushort vk, uint f) { INPUT_A i = new INPUT_A(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = f; return i; }
    static INPUT_B MkB(ushort vk, uint f) { INPUT_B i = new INPUT_B(); i.type = INPUT_KEYBOARD; i.u.ki.wVk = vk; i.u.ki.dwFlags = f; return i; }
    public static int SizeA { get { return Marshal.SizeOf(typeof(INPUT_A)); } }
    public static int SizeB { get { return Marshal.SizeOf(typeof(INPUT_B)); } }
    public static bool RAltDownState { get { return (GetAsyncKeyState(0xA5) & 0x8000) != 0; } }
}
'@
Write-Host ("bitness64=" + [Environment]::Is64BitProcess + " sizeA(union-full)=" + [SizeProbe]::SizeA + " sizeB(union-kb)=" + [SizeProbe]::SizeB)

function Test-Variant([string]$tag, [scriptblock]$down, [scriptblock]$up) {
    $sentDown = & $down
    Start-Sleep -Milliseconds 250
    $held = [SizeProbe]::RAltDownState
    $sentUp = & $up
    Start-Sleep -Milliseconds 150
    $after = [SizeProbe]::RAltDownState
    Write-Host ("{0}: sentDown={1} held={2} sentUp={3} afterHeld={4}" -f $tag, $sentDown, $held, $sentUp, $after)
}

Test-Variant "A-single(40B)" { [SizeProbe]::A_RAltDown() } { [SizeProbe]::A_RAltUp() }
Test-Variant "B-single(32B)" { [SizeProbe]::B_RAltDown() } { [SizeProbe]::B_RAltUp() }

Write-Host "--- batch4 Win+H dry (no focus requirements, just return values) ---"
Start-Sleep -Milliseconds 400
$b4 = [SizeProbe]::B_Batch4_WinH()
Start-Sleep -Milliseconds 400
$a4 = [SizeProbe]::A_Batch4_WinH()
Write-Host ("B_batch4 sent={0} (win-h may have fired; will dismiss next)" -f $b4)
Write-Host ("A_batch4 sent={0}" -f $a4)
Start-Sleep -Milliseconds 300
# dismiss any voice-typing overlay that opened: Esc tap via A struct
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class EscTap {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
    [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    public static uint Tap(ushort vk) {
        INPUT[] a = new INPUT[2];
        INPUT i1 = new INPUT(); i1.type = 1; i1.u.ki.wVk = vk; i1.u.ki.dwFlags = 0; a[0] = i1;
        INPUT i2 = new INPUT(); i2.type = 1; i2.u.ki.wVk = vk; i2.u.ki.dwFlags = 2; a[1] = i2;
        return SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@
[void][EscTap]::Tap(0x1B)
Write-Host "esc sent (dismiss any overlay)"

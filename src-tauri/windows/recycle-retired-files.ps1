param([string]$InstallDirectory, [switch]$FunctionsOnly, [switch]$ProductFiles)
$ErrorActionPreference = 'Stop'

function Get-SayAllRetiredFiles([string]$Directory, [switch]$ProductFiles) {
    $root = [IO.Path]::GetFullPath($Directory).TrimEnd('\', '/')
    if ($root -eq [IO.Path]::GetPathRoot($root).TrimEnd('\', '/')) { throw 'Volume root is not an installation directory' }
    if (-not [IO.Directory]::Exists($root)) { throw 'Installation directory does not exist' }
    # Validate every existing ancestor, including the installation root itself.
    $ancestor = Get-Item -LiteralPath $root -Force
    while ($null -ne $ancestor) {
        if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse installation directory rejected' }
        $ancestor = $ancestor.Parent
    }
    $known = @('sayall-hid-host-helper.exe', 'sayall-component-helper.exe',
        'SayAllInput/SayAllInput.inf', 'SayAllInput/SayAllInput.sys', 'SayAllInput/SayAllInput.cat')
    if ($ProductFiles) {
        # Current payload only; never enumerate settings, authorization or unknown files.
        $known += @('sayall-windows-app.exe', 'sayall-helper.exe', 'frida-gadget.dll',
            'licenses/ATTRIBUTION.md', 'licenses/Frida-COPYING.txt', 'licenses/LICENSE', 'uninstall.exe')
    }
    foreach ($relative in $known) {
        $candidate = [IO.Path]::GetFullPath((Join-Path $root $relative))
        if (-not $candidate.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Retired path escaped installation directory' }
        $cursor = $root
        foreach ($part in $relative.Split('/')) {
            $cursor = Join-Path $cursor $part
            if (Test-Path -LiteralPath $cursor) {
                if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse retired payload rejected' }
            }
        }
        if (Test-Path -LiteralPath $candidate) {
            if (-not [IO.File]::Exists($candidate)) { throw 'Retired payload is not a file' }
            $candidate
        }
    }
}

function Initialize-SayAllRecycleApi {
    if ('SayAllRecycle' -as [type]) { return }
    # Public Windows Shell API. RECYCLEONDELETE plus the progress sink refuses
    # operations lacking recycle semantics; no permanent-delete fallback exists.
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("947AAB5F-0A5C-4C13-B4D6-4BF7836FC9F8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IFileOperation {
 void Advise(IFileOperationProgressSink sink, out uint cookie); void Unadvise(uint cookie);
 void SetOperationFlags(uint flags); void SetProgressMessage([MarshalAs(UnmanagedType.LPWStr)] string text);
 void SetProgressDialog(IntPtr dialog); void SetProperties(IntPtr properties); void SetOwnerWindow(IntPtr window);
 void ApplyPropertiesToItem(IntPtr item); void ApplyPropertiesToItems(IntPtr items);
 void RenameItem(IntPtr item, [MarshalAs(UnmanagedType.LPWStr)] string name, IntPtr sink); void RenameItems(IntPtr items, [MarshalAs(UnmanagedType.LPWStr)] string name);
 void MoveItem(IntPtr item, IntPtr destination, [MarshalAs(UnmanagedType.LPWStr)] string name, IntPtr sink); void MoveItems(IntPtr items, IntPtr destination);
 void CopyItem(IntPtr item, IntPtr destination, [MarshalAs(UnmanagedType.LPWStr)] string name, IntPtr sink); void CopyItems(IntPtr items, IntPtr destination);
 void DeleteItem(IntPtr item, IFileOperationProgressSink sink); void DeleteItems(IntPtr items);
 void NewItem(IntPtr destination, uint attributes, [MarshalAs(UnmanagedType.LPWStr)] string name, [MarshalAs(UnmanagedType.LPWStr)] string template, IntPtr sink);
 void PerformOperations(); void GetAnyOperationsAborted([MarshalAs(UnmanagedType.Bool)] out bool aborted);
}
[ComVisible(true), Guid("04B0F1A7-9490-44BC-96E1-4296A31252E2"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IFileOperationProgressSink {
 [PreserveSig] int StartOperations(); [PreserveSig] int FinishOperations(int result);
 [PreserveSig] int PreRenameItem(uint flags, IntPtr item, IntPtr name); [PreserveSig] int PostRenameItem(uint flags, IntPtr item, IntPtr name, int result, IntPtr created);
 [PreserveSig] int PreMoveItem(uint flags, IntPtr item, IntPtr destination, IntPtr name); [PreserveSig] int PostMoveItem(uint flags, IntPtr item, IntPtr destination, IntPtr name, int result, IntPtr created);
 [PreserveSig] int PreCopyItem(uint flags, IntPtr item, IntPtr destination, IntPtr name); [PreserveSig] int PostCopyItem(uint flags, IntPtr item, IntPtr destination, IntPtr name, int result, IntPtr created);
 [PreserveSig] int PreDeleteItem(uint flags, IntPtr item); [PreserveSig] int PostDeleteItem(uint flags, IntPtr item, int result, IntPtr created);
 [PreserveSig] int PreNewItem(uint flags, IntPtr destination, IntPtr name); [PreserveSig] int PostNewItem(uint flags, IntPtr destination, IntPtr name, IntPtr template, uint attributes, int result, IntPtr created);
 [PreserveSig] int UpdateProgress(uint total, uint completed); [PreserveSig] int ResetTimer(); [PreserveSig] int PauseTimer(); [PreserveSig] int ResumeTimer();
}
[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
class RecycleSink : IFileOperationProgressSink {
 public bool Recycled;
 public int StartOperations(){return 0;} public int FinishOperations(int r){return 0;}
 public int PreDeleteItem(uint f, IntPtr i){return (f & 0x80) != 0 ? 0 : unchecked((int)0x80004004);}
 public int PostDeleteItem(uint f, IntPtr i, int r, IntPtr created){Recycled = r >= 0 && created != IntPtr.Zero; return 0;}
 public int PreRenameItem(uint f, IntPtr i, IntPtr n){return 0;} public int PostRenameItem(uint f, IntPtr i, IntPtr n, int r, IntPtr c){return 0;}
 public int PreMoveItem(uint f, IntPtr i, IntPtr d, IntPtr n){return 0;} public int PostMoveItem(uint f, IntPtr i, IntPtr d, IntPtr n, int r, IntPtr c){return 0;}
 public int PreCopyItem(uint f, IntPtr i, IntPtr d, IntPtr n){return 0;} public int PostCopyItem(uint f, IntPtr i, IntPtr d, IntPtr n, int r, IntPtr c){return 0;}
 public int PreNewItem(uint f, IntPtr d, IntPtr n){return 0;} public int PostNewItem(uint f, IntPtr d, IntPtr n, IntPtr t, uint a, int r, IntPtr c){return 0;}
 public int UpdateProgress(uint t,uint c){return 0;} public int ResetTimer(){return 0;} public int PauseTimer(){return 0;} public int ResumeTimer(){return 0;}
}
public static class SayAllRecycle {
 [DllImport("shell32.dll", CharSet=CharSet.Unicode, PreserveSig=false)]
 static extern void SHCreateItemFromParsingName(string path, IntPtr bind, ref Guid iid, out IntPtr item);
 public static void File(string path) {
  IFileOperation operation = (IFileOperation)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("3AD05575-8857-4850-9277-11B85BDB8E09")));
  IntPtr item = IntPtr.Zero;
  try {
   Guid iid = new Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE");
   SHCreateItemFromParsingName(path, IntPtr.Zero, ref iid, out item);
   operation.SetOperationFlags(0x00080000 | 0x00100000 | 0x4000 | 0x0400 | 0x0010 | 0x0004);
   RecycleSink sink = new RecycleSink(); operation.DeleteItem(item, sink); operation.PerformOperations();
   bool aborted; operation.GetAnyOperationsAborted(out aborted);
   if (aborted || !sink.Recycled || System.IO.File.Exists(path)) throw new InvalidOperationException("Recycle operation not confirmed");
  } finally { if (item != IntPtr.Zero) Marshal.Release(item); Marshal.FinalReleaseComObject(operation); }
 }
}
'@
}

function Invoke-SayAllRetiredFileRecycle([string]$Directory, [switch]$ProductFiles) {
    $files = @(Get-SayAllRetiredFiles $Directory -ProductFiles:$ProductFiles)
    if ($files.Count -gt 0) { Initialize-SayAllRecycleApi }
    foreach ($file in $files) {
        # Revalidate immediately before each operation; never recurse or accept a wildcard.
        if ($file -notin @(Get-SayAllRetiredFiles $Directory -ProductFiles:$ProductFiles)) { throw 'Product file changed during cleanup' }
        [SayAllRecycle]::File($file)
    }
    Write-Output ('retired-files: recycled_count=' + $files.Count)
}

if ($FunctionsOnly) { return }
try { Invoke-SayAllRetiredFileRecycle $InstallDirectory -ProductFiles:$ProductFiles; exit 0 }
catch { Write-Output 'retired-files: unconfirmed; no permanent-delete fallback'; exit 15 }

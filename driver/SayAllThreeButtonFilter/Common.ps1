# Shared, read-only preflight. Loading this file does not change system state.
$SayAllFilterName = 'SayAllThreeButtonFilter'
$SayAllHardwareId = 'HID\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4'

function Get-SayAllCodeIntegrity {
    if (-not ('SayAll.DriverCodeIntegrity' -as [type])) {
        Add-Type @'
using System;
using System.Runtime.InteropServices;
namespace SayAll {
    public static class DriverCodeIntegrity {
        [StructLayout(LayoutKind.Sequential)] struct Info { public uint Length; public uint Options; }
        [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int c, ref Info i, uint n, out uint r);
        public static uint Read() {
            var i = new Info { Length = 8 }; uint r;
            int status = NtQuerySystemInformation(103, ref i, 8, out r);
            if (status < 0) throw new InvalidOperationException("Cannot query running kernel code integrity");
            return i.Options;
        }
    }
}
'@
    }
    $options = [SayAll.DriverCodeIntegrity]::Read()
    [pscustomobject]@{ options=$options; testSigningActive=[bool]($options -band 2) }
}

function Assert-SayAllStopped {
    try {
        $mutex = [Threading.Mutex]::OpenExisting('SayAll.Windows.SingleInstance')
    } catch [Threading.WaitHandleCannotBeOpenedException] {
        return
    }
    $mutex.Dispose()
    throw 'Exit SayAll using its tray menu first. Closing the window only hides it.'
}

function Get-SayAllCollections {
    # Restrict enumeration to the vendor/product, but validate the complete INF ID.
    # Do not print instance IDs, Bluetooth addresses, or device paths.
    Get-PnpDevice -PresentOnly | Where-Object {
        $_.InstanceId -like 'HID\*VID&012717_PID&32B8*'
    } | Where-Object {
        $ids = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds -ErrorAction Stop).Data
        $ids -contains $SayAllHardwareId
    }
}

function Assert-SayAllPackage([string]$Directory) {
    $manifest = Get-Content (Join-Path $Directory 'SHA256.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach ($required in @('SayAllThreeButtonFilter.sys','SayAllThreeButtonFilter.inf','SayAllThreeButtonFilter.cat','SayAllThreeButtonFilter.cer','Install.ps1','Uninstall.ps1','Common.ps1','Status.ps1')) {
        if (-not $manifest.PSObject.Properties[$required]) { throw "Missing checksum: $required" }
    }
    foreach ($entry in $manifest.PSObject.Properties) {
        if ($entry.Name -notmatch '^[a-zA-Z0-9_.-]+$' -or $entry.Value -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid checksum manifest' }
        if ((Get-FileHash -LiteralPath (Join-Path $Directory $entry.Name) -Algorithm SHA256).Hash -ne $entry.Value) { throw "Checksum mismatch: $($entry.Name)" }
    }
    $cert = New-Object Security.Cryptography.X509Certificates.X509Certificate2((Join-Path $Directory 'SayAllThreeButtonFilter.cer'))
    foreach ($name in @('SayAllThreeButtonFilter.sys','SayAllThreeButtonFilter.cat')) {
        $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $Directory $name)
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $cert.Thumbprint) { throw "Untrusted or unexpected signer: $name" }
    }
}

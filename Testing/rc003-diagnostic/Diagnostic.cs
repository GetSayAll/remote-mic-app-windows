// Standalone, observation-only Windows RC003 diagnostic. No key injection or suppression; short-lived observation hook only.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32.SafeHandles;
using Windows.Devices.Bluetooth;
using Windows.Devices.Bluetooth.GenericAttributeProfile;
using Windows.Devices.Enumeration;
using Windows.Foundation;
using Windows.Storage.Streams;

internal sealed class Log : IDisposable {
    readonly StreamWriter writer;
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly object sync = new object();
    readonly JavaScriptSerializer json = new JavaScriptSerializer();
    long bytes;
    bool closed;
    public string Step = "setup";
    public Log(string path) { writer = new StreamWriter(path, false, new UTF8Encoding(false)); writer.AutoFlush = true; }
    public void Write(string kind, object data) {
        lock(sync) {
            if (closed || bytes > 10 * 1024 * 1024) return;
            string line = json.Serialize(new { utc = DateTime.UtcNow.ToString("o"), ms = clock.ElapsedMilliseconds, step = Step, kind = kind, data = data });
            writer.WriteLine(line); bytes += Encoding.UTF8.GetByteCount(line) + 2;
        }
    }
    public void Error(string stage, Exception error) { Write("error", new { stage = stage, type = error.GetType().Name, hresult = error.HResult }); }
    public void Dispose() { lock(sync) { if (!closed) { closed = true; writer.Dispose(); } } }
}

internal static class Native {
    [DllImport("user32.dll")] internal static extern IntPtr GetForegroundWindow();
    internal sealed class Target { public string Label; public ulong Address; }
    [StructLayout(LayoutKind.Sequential)] internal struct Device { public IntPtr Handle; public uint Type; }
    [StructLayout(LayoutKind.Sequential)] internal struct Registration { public ushort Page, Usage; public uint Flags; public IntPtr Target; }
    [StructLayout(LayoutKind.Sequential)] internal struct Header { public uint Type, Size; public IntPtr Device, WParam; }
    [StructLayout(LayoutKind.Sequential)] internal struct Interface { public uint Size; public Guid Class; public uint Flags; public IntPtr Reserved; }
    [StructLayout(LayoutKind.Sequential)] internal struct Caps {
        public ushort Usage, Page, InputLength, OutputLength, FeatureLength;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst=17)] public ushort[] Reserved;
        public ushort Nodes, InputButtons, InputValues, InputIndices, OutputButtons, OutputValues, OutputIndices, FeatureButtons, FeatureValues, FeatureIndices;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct UsageAndPage { public ushort Usage, Page; }
    [DllImport("user32.dll", SetLastError=true)] internal static extern uint GetRawInputDeviceList(IntPtr data, ref uint count, uint size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern uint GetRawInputDeviceInfoW(IntPtr device, uint cmd, IntPtr data, ref uint size);
    [DllImport("user32.dll", SetLastError=true)] internal static extern uint GetRawInputData(IntPtr input, uint cmd, IntPtr data, ref uint size, uint header);
    [DllImport("user32.dll", SetLastError=true)] internal static extern bool RegisterRawInputDevices(Registration[] devices, uint count, uint size);
    [DllImport("hid.dll")] internal static extern void HidD_GetHidGuid(out Guid guid);
    [DllImport("hid.dll")] internal static extern bool HidD_GetPreparsedData(SafeFileHandle file, out IntPtr data);
    [DllImport("hid.dll")] internal static extern bool HidD_FreePreparsedData(IntPtr data);
    [DllImport("hid.dll")] internal static extern int HidP_GetCaps(IntPtr data, out Caps caps);
    [DllImport("hid.dll")] internal static extern int HidP_GetUsagesEx(int type, ushort collection, [Out] UsageAndPage[] usages, ref uint length, IntPtr preparsed, byte[] report, uint reportLength);
    [DllImport("hid.dll")] internal static extern int HidP_GetButtonCaps(int type, IntPtr caps, ref ushort count, IntPtr preparsed);
    [DllImport("hid.dll")] internal static extern int HidP_GetValueCaps(int type, IntPtr caps, ref ushort count, IntPtr preparsed);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern IntPtr SetupDiGetClassDevsW(ref Guid guid, IntPtr enumerator, IntPtr parent, uint flags);
    [DllImport("setupapi.dll", EntryPoint="SetupDiGetClassDevsW", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern IntPtr AllDevices(IntPtr guid, IntPtr enumerator, IntPtr parent, uint flags);
    [DllImport("setupapi.dll", SetLastError=true)] internal static extern bool SetupDiEnumDeviceInfo(IntPtr set, uint index, ref Interface device);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern bool SetupDiGetDeviceInstanceIdW(IntPtr set, ref Interface device, StringBuilder text, uint size, out uint needed);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern bool SetupDiGetDeviceRegistryPropertyW(IntPtr set, ref Interface device, uint property, out uint type, byte[] data, uint size, out uint needed);
    [DllImport("setupapi.dll", SetLastError=true)] internal static extern bool SetupDiEnumDeviceInterfaces(IntPtr set, IntPtr device, ref Guid guid, uint index, ref Interface item);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern bool SetupDiGetDeviceInterfaceDetailW(IntPtr set, ref Interface item, IntPtr detail, uint size, out uint needed, IntPtr device);
    [DllImport("setupapi.dll")] internal static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    [DllImport("cfgmgr32.dll")] internal static extern uint CM_Get_Parent(out uint parent, uint child, uint flags);
    [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode)] internal static extern uint CM_Get_Device_IDW(uint device, StringBuilder id, uint length, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    internal static bool Matches(string path) {
        string p = path.ToLowerInvariant();
        return (p.Contains("vid_2717") && p.Contains("pid_32b8")) ||
            ((p.Contains("dev_vid&002717") || p.Contains("dev_vid&012717")) && p.Contains("pid&32b8"));
    }
    internal static string Name(IntPtr device) {
        uint count = 0;
        if (GetRawInputDeviceInfoW(device, 0x20000007, IntPtr.Zero, ref count) == UInt32.MaxValue) throw new System.ComponentModel.Win32Exception();
        IntPtr p = Marshal.AllocHGlobal(checked((int)(count + 1) * 2));
        try { if (GetRawInputDeviceInfoW(device, 0x20000007, p, ref count) == UInt32.MaxValue) throw new System.ComponentModel.Win32Exception(); return Marshal.PtrToStringUni(p); }
        finally { Marshal.FreeHGlobal(p); }
    }
    internal static List<Target> BluetoothTargets() {
        var targets=new List<Target>(); var seen=new HashSet<ulong>(); var remoteAddresses=new HashSet<ulong>();
        IntPtr set=AllDevices(IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,6);
        if(set==new IntPtr(-1)) throw new System.ComponentModel.Win32Exception();
        try {
            for(uint i=0;;i++) {
                var info=new Interface {Size=(uint)Marshal.SizeOf(typeof(Interface))};
                if(!SetupDiEnumDeviceInfo(set,i,ref info)) { if(Marshal.GetLastWin32Error()!=259) throw new System.ComponentModel.Win32Exception(); break; }
                var id=new StringBuilder(1024); uint needed;
                if(!SetupDiGetDeviceInstanceIdW(set,ref info,id,1024,out needed)) continue;
                if(Matches(id.ToString())) {
                    uint current=info.Flags;
                    for(int depth=0;depth<16;depth++) {
                        var ancestor=new StringBuilder(1024);
                        if(CM_Get_Device_IDW(current,ancestor,1024,0)!=0) break;
                        var parentMatch=Regex.Match(ancestor.ToString(),@"^BTHLE\\DEV_([0-9A-F]{12})\\",RegexOptions.IgnoreCase);
                        if(parentMatch.Success) { remoteAddresses.Add(Convert.ToUInt64(parentMatch.Groups[1].Value,16)); break; }
                        uint parent; if(CM_Get_Parent(out parent,current,0)!=0) break; current=parent;
                    }
                }
                var match=Regex.Match(id.ToString(),@"^BTHLE\\DEV_([0-9A-F]{12})\\",RegexOptions.IgnoreCase);
                if(!match.Success) continue;
                ulong address=Convert.ToUInt64(match.Groups[1].Value,16); if(!seen.Add(address)) continue;
                byte[] name=new byte[2048]; uint type;
                bool ok=SetupDiGetDeviceRegistryPropertyW(set,ref info,12,out type,name,(uint)name.Length,out needed);
                if(!ok) ok=SetupDiGetDeviceRegistryPropertyW(set,ref info,0,out type,name,(uint)name.Length,out needed);
                string label=ok ? Encoding.Unicode.GetString(name,0,(int)Math.Min(needed,(uint)name.Length)).TrimEnd('\0') : "蓝牙 LE 设备";
                targets.Add(new Target {Label=label,Address=address});
            }
        } finally { SetupDiDestroyDeviceInfoList(set); }
        // Only allow GATT access to an ancestor of VID 2717 / PID 32B8,
        // never to arbitrary headphones or other nearby BLE peripherals.
        return targets.Where(t=>remoteAddresses.Contains(t.Address)).ToList();
    }
}

internal sealed class RawCapture : NativeWindow, IDisposable {
    readonly Log log;
    readonly Dictionary<string, string> aliases = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
    readonly HashSet<uint> collections = new HashSet<uint>();
    readonly HashSet<uint> registered = new HashSet<uint>();
    public int Events;
    public int Interfaces;
    public int UpDown, UpUp;
    public RawCapture(Log log, bool listen) {
        this.log = log;
        CreateHandle(new CreateParams { Caption = "SayAll RC003 diagnostic sink", Parent = new IntPtr(-3) });
        Inventory();
        if (listen) Register();
    }
    string Alias(string path) { string alias; if (!aliases.TryGetValue(path, out alias)) { alias = "interface_" + (aliases.Count + 1); aliases[path] = alias; } return alias; }
    void Describe(IntPtr data, string alias) {
        Native.Caps c;
        int status = Native.HidP_GetCaps(data, out c);
        log.Write("hid_caps", new { device = alias, status = status, page = c.Page, usage = c.Usage, inputBytes = c.InputLength, inputButtons = c.InputButtons, inputValues = c.InputValues });
        if (status != 0x110000) return;
        collections.Add(((uint)c.Page << 16) | c.Usage);
        // Native HIDP_*_CAPS are 72 bytes. Preserve descriptor-derived capability
        // records, not opaque preparsed memory (which can contain pointers).
        for (int kind=0; kind<2; kind++) {
            ushort count = kind == 0 ? c.InputButtons : c.InputValues;
            if (count == 0 || count > 512) continue;
            IntPtr p = Marshal.AllocHGlobal(count * 72);
            try {
                int result = kind == 0 ? Native.HidP_GetButtonCaps(0, p, ref count, data) : Native.HidP_GetValueCaps(0, p, ref count, data);
                if (result != 0x110000) { log.Write("hid_cap_error", new { device=alias, status=result, capKind=kind }); continue; }
                for (int i=0; i<count; i++) {
                    byte[] cap = new byte[72]; Marshal.Copy(IntPtr.Add(p,i*72),cap,0,72);
                    log.Write("hid_input_cap", new { device=alias, capKind=kind, page=BitConverter.ToUInt16(cap,0), reportId=cap[2], isRange=cap[12]!=0, usageMin=BitConverter.ToUInt16(cap,56), usageMax=BitConverter.ToUInt16(cap,58) });
                }
            } finally { Marshal.FreeHGlobal(p); }
        }
    }
    public void Inventory() {
        Guid guid; Native.HidD_GetHidGuid(out guid);
        IntPtr set = Native.SetupDiGetClassDevsW(ref guid, IntPtr.Zero, IntPtr.Zero, 0x12);
        if (set == new IntPtr(-1)) throw new System.ComponentModel.Win32Exception();
        int found=0;
        try {
            for(uint i=0; ; i++) {
                Native.Interface item = new Native.Interface { Size=(uint)Marshal.SizeOf(typeof(Native.Interface)) };
                if (!Native.SetupDiEnumDeviceInterfaces(set,IntPtr.Zero,ref guid,i,ref item)) {
                    int error=Marshal.GetLastWin32Error(); if(error!=259) log.Write("enumeration_error",new { error=error }); break;
                }
                uint needed; Native.SetupDiGetDeviceInterfaceDetailW(set,ref item,IntPtr.Zero,0,out needed,IntPtr.Zero);
                if(needed<8 || needed>65536) continue;
                IntPtr detail=Marshal.AllocHGlobal((int)needed);
                try {
                    Marshal.WriteInt32(detail,IntPtr.Size==8 ? 8 : 6);
                    if(!Native.SetupDiGetDeviceInterfaceDetailW(set,ref item,detail,needed,out needed,IntPtr.Zero)) { log.Write("detail_error",new {error=Marshal.GetLastWin32Error()}); continue; }
                    string path=Marshal.PtrToStringUni(IntPtr.Add(detail,4));
                    if(!Native.Matches(path)) continue;
                    found++; string alias=Alias(path);
                    using(SafeFileHandle file=Native.CreateFileW(path,0,3,IntPtr.Zero,3,0,IntPtr.Zero)) {
                        if(file.IsInvalid) { log.Write("hid_open_error",new {device=alias,error=Marshal.GetLastWin32Error()}); continue; }
                        IntPtr data;
                        if(Native.HidD_GetPreparsedData(file,out data)) { try { Describe(data,alias); } finally { Native.HidD_FreePreparsedData(data); } }
                        else log.Write("preparsed_error",new {device=alias,error=Marshal.GetLastWin32Error()});
                    }
                } finally { Marshal.FreeHGlobal(detail); }
            }
        } finally { Native.SetupDiDestroyDeviceInfoList(set); }
        Interfaces=found;
        uint count=0, size=(uint)Marshal.SizeOf(typeof(Native.Device));
        if(Native.GetRawInputDeviceList(IntPtr.Zero,ref count,size)==UInt32.MaxValue) throw new System.ComponentModel.Win32Exception();
        IntPtr list=Marshal.AllocHGlobal(checked((int)(Math.Max(count,1)*size)));
        try {
            uint got=Native.GetRawInputDeviceList(list,ref count,size);
            if(got==UInt32.MaxValue) throw new System.ComponentModel.Win32Exception();
            int matches=0;
            for(int i=0;i<got;i++) {
                Native.Device d=(Native.Device)Marshal.PtrToStructure(IntPtr.Add(list,i*(int)size),typeof(Native.Device));
                string path=Native.Name(d.Handle); if(!Native.Matches(path)) continue;
                matches++; log.Write("raw_device",new {device=Alias(path),type=d.Type});
            }
            log.Write("inventory",new {hidInterfaces=found,rawInterfaces=matches, note="VID/PID match is not proof of model; multiple remote units must be disconnected except target"});
        } finally { Marshal.FreeHGlobal(list); }
    }
    void Register() {
        collections.Add(0x00010006); collections.Add(0x000C0001);
        foreach(uint c in collections) {
            // Registration can itself enqueue device-arrival messages. Never
            // register an unchanged collection again from its arrival callback.
            if(registered.Contains(c)) continue;
            var r=new Native.Registration {Page=(ushort)(c>>16),Usage=(ushort)c,Flags=0x2100,Target=Handle};
            bool ok=Native.RegisterRawInputDevices(new[]{r},1,(uint)Marshal.SizeOf(typeof(Native.Registration)));
            if(ok) registered.Add(c);
            log.Write("raw_register",new {page=r.Page,usage=r.Usage,success=ok,error=ok?0:Marshal.GetLastWin32Error()});
        }
    }
    protected override void WndProc(ref Message m) {
        try {
            if(m.Msg==0xFE) { Inventory(); Register(); }
            if(m.Msg==0xFF) Capture(m.LParam);
        } catch(Exception e) { log.Error("raw_callback",e); }
        // DefWindowProc is required for foreground WM_INPUT cleanup.
        base.WndProc(ref m);
    }
    void Capture(IntPtr input) {
        uint size=0, header=(uint)Marshal.SizeOf(typeof(Native.Header));
        if(Native.GetRawInputData(input,0x10000003,IntPtr.Zero,ref size,header)==UInt32.MaxValue || size<header || size>1024*1024) return;
        IntPtr p=Marshal.AllocHGlobal((int)size);
        try {
            if(Native.GetRawInputData(input,0x10000003,p,ref size,header)!=size) throw new InvalidDataException();
            Native.Header h=(Native.Header)Marshal.PtrToStructure(p,typeof(Native.Header));
            if(h.Device==IntPtr.Zero) return;
            string path=Native.Name(h.Device); if(!Native.Matches(path)) return;
            string alias=Alias(path); Events++;
            byte[] body=new byte[size-header]; Marshal.Copy(IntPtr.Add(p,(int)header),body,0,body.Length);
            if(h.Type==1 && body.Length>=16) {
                if(BitConverter.ToUInt16(body,6)==0x26) { uint msg=BitConverter.ToUInt32(body,8); if(msg==256 || msg==260) UpDown++; if(msg==257 || msg==261) UpUp++; }
                log.Write("keyboard",new {device=alias,scan=BitConverter.ToUInt16(body,0),flags=BitConverter.ToUInt16(body,2),vk=BitConverter.ToUInt16(body,6),message=BitConverter.ToUInt32(body,8)});
            } else if(h.Type==2) {
                foreach(byte[] report in SplitReports(body)) {
                    log.Write("hid_report",new {device=alias,length=report.Length,hex=BitConverter.ToString(report.Take(256).ToArray()),truncated=report.Length>256});
                    uint len=0;
                    if(Native.GetRawInputDeviceInfoW(h.Device,0x20000005,IntPtr.Zero,ref len)==UInt32.MaxValue || len==0) continue;
                    IntPtr data=Marshal.AllocHGlobal((int)len);
                    try {
                        if(Native.GetRawInputDeviceInfoW(h.Device,0x20000005,data,ref len)==UInt32.MaxValue) continue;
                        var usages=new Native.UsageAndPage[128]; uint n=128;
                        int status=Native.HidP_GetUsagesEx(0,0,usages,ref n,data,report,(uint)report.Length);
                        log.Write("hid_usages",new {device=alias,status=status,usages=status==0x110000 ? usages.Take((int)n).Select(u=>new {page=u.Page,usage=u.Usage}).ToArray() : null});
                    } finally { Marshal.FreeHGlobal(data); }
                }
            } else log.Write("raw_unknown",new {device=alias,type=h.Type,length=body.Length});
        } finally { Marshal.FreeHGlobal(p); }
    }
    internal static List<byte[]> SplitReports(byte[] body) {
        if(body.Length<8) throw new InvalidDataException("short HID header");
        uint size=BitConverter.ToUInt32(body,0), count=BitConverter.ToUInt32(body,4);
        if(size==0 || count==0 || (ulong)size*count>(ulong)(body.Length-8)) throw new InvalidDataException("truncated HID reports");
        var result=new List<byte[]>();
        for(uint i=0;i<count;i++) { byte[] r=new byte[(int)size]; System.Buffer.BlockCopy(body,checked(8+(int)(i*size)),r,0,r.Length); result.Add(r); }
        return result;
    }
    public void Dispose() {
        foreach(uint c in registered) Native.RegisterRawInputDevices(new[]{new Native.Registration {Page=(ushort)(c>>16),Usage=(ushort)c,Flags=1,Target=IntPtr.Zero}},1,(uint)Marshal.SizeOf(typeof(Native.Registration)));
        DestroyHandle();
    }
}

internal sealed class KeyboardCapture : IDisposable {
    [StructLayout(LayoutKind.Sequential)] internal struct Edge { public uint Vk, Scan, Flags, Time; public UIntPtr Extra; }
    delegate IntPtr Hook(int code,IntPtr message,IntPtr data);
    [DllImport("user32.dll",SetLastError=true)] static extern IntPtr SetWindowsHookExW(int kind,Hook callback,IntPtr module,uint thread);
    [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr hook,int code,IntPtr message,IntPtr data);
    [DllImport("user32.dll",SetLastError=true)] static extern bool UnhookWindowsHookEx(IntPtr hook);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string name);
    readonly Log log;
    readonly Hook callback;
    IntPtr handle;
    public int Events;
    public int UpDown, UpUp;
    public KeyboardCapture(Log log) {
        this.log=log; callback=Observe;
        handle=SetWindowsHookExW(13,callback,GetModuleHandleW(null),0);
        log.Write("ll_register",new {success=handle!=IntPtr.Zero,error=handle==IntPtr.Zero?Marshal.GetLastWin32Error():0,scope="candidate_keys_only_unattributed",suppression=false});
        if(handle==IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
    }
    internal static bool Candidate(uint vk) { return vk==0xFF || vk==0xAE || vk==0xAF || vk==0xAD || vk==0xA6 || vk==0x26 || vk==0x08 || vk==0x1B; }
    IntPtr Observe(int code,IntPtr message,IntPtr data) {
        try {
            if(code==0) {
                Edge edge=(Edge)Marshal.PtrToStructure(data,typeof(Edge));
                if(Candidate(edge.Vk)) {
                    if(edge.Vk==0x26 && (edge.Flags&0x10)==0) { if(message.ToInt64()==256 || message.ToInt64()==260) UpDown++; if(message.ToInt64()==257 || message.ToInt64()==261) UpUp++; }
                    Events++;
                    log.Write("ll_keyboard",new {vk=edge.Vk,scan=edge.Scan,flags=edge.Flags,message=message.ToInt64(),injected=(edge.Flags&0x10)!=0,device="unattributed"});
                }
            }
        } catch(Exception e) { try {log.Error("ll_callback",e);} catch {} }
        return CallNextHookEx(handle,code,message,data);
    }
    public void Dispose() { if(handle!=IntPtr.Zero) { bool ok=UnhookWindowsHookEx(handle); log.Write("ll_stop",new {success=ok,error=ok?0:Marshal.GetLastWin32Error()}); if(ok) handle=IntPtr.Zero; } }
}

internal sealed class GattCapture {
    readonly Log log;
    readonly List<GattDeviceService> services=new List<GattDeviceService>();
    readonly List<Subscription> subscriptions=new List<Subscription>();
    BluetoothLEDevice device;
    CancellationToken cancellation;
    public int Events;
    public int Subscribed;
    public bool CleanupFailed;
    sealed class Subscription {
        public GattCharacteristic Characteristic;
        public GattClientCharacteristicConfigurationDescriptorValue Previous;
        public TypedEventHandler<GattCharacteristic,GattValueChangedEventArgs> Handler;
    }
    public GattCapture(Log log) { this.log=log; }
    internal static async Task<T> Wait<T>(IAsyncOperation<T> operation, CancellationToken cancellation = default(CancellationToken)) {
        var elapsed=Stopwatch.StartNew();
        try {
            while(operation.Status==AsyncStatus.Started) {
                if(cancellation.IsCancellationRequested) { operation.Cancel(); cancellation.ThrowIfCancellationRequested(); }
                if(elapsed.ElapsedMilliseconds>=10000) { operation.Cancel(); throw new TimeoutException(); }
                await Task.Delay(50);
            }
            return operation.GetResults();
        } finally { try { operation.Close(); } catch { /* A cancelled provider may still be finishing. */ } }
    }
    Task<T> Await<T>(IAsyncOperation<T> operation) { return Wait(operation,cancellation); }
    internal static byte[] Bytes(IBuffer buffer) {
        using(var reader=DataReader.FromBuffer(buffer)) { byte[] data=new byte[buffer.Length]; reader.ReadBytes(data); return data; }
    }
    internal static GattClientCharacteristicConfigurationDescriptorValue Mode(GattCharacteristicProperties p) {
        if((p & GattCharacteristicProperties.Notify)!=0) return GattClientCharacteristicConfigurationDescriptorValue.Notify;
        if((p & GattCharacteristicProperties.Indicate)!=0) return GattClientCharacteristicConfigurationDescriptorValue.Indicate;
        return GattClientCharacteristicConfigurationDescriptorValue.None;
    }
    public async Task Start(Native.Target selected, CancellationToken cancellation) {
        this.cancellation=cancellation;
        // Same public address-based connection path as the application's BLE layer.
        device=await Await(BluetoothLEDevice.FromBluetoothAddressAsync(selected.Address));
        if(device==null) throw new InvalidOperationException("device unavailable");
        var result=await Await(device.GetGattServicesAsync(BluetoothCacheMode.Uncached));
        log.Write("gatt_services",new {status=result.Status.ToString(),protocolError=result.ProtocolError,count=result.Services.Count});
        if(result.Status!=GattCommunicationStatus.Success) return;
        services.AddRange(result.Services);
        foreach(var service in services) {
            cancellation.ThrowIfCancellationRequested();
            string sid=service.Uuid.ToString();
            try {
                var cr=await Await(service.GetCharacteristicsAsync(BluetoothCacheMode.Uncached));
                log.Write("gatt_characteristics",new {service=sid,status=cr.Status.ToString(),protocolError=cr.ProtocolError,count=cr.Characteristics.Count});
                if(cr.Status!=GattCommunicationStatus.Success) continue;
                foreach(var ch in cr.Characteristics) {
                    cancellation.ThrowIfCancellationRequested();
                    string cid=ch.Uuid.ToString(); ushort handle=ch.AttributeHandle;
                    log.Write("gatt_characteristic",new {service=sid,characteristic=cid,handle=handle,properties=ch.CharacteristicProperties.ToString()});
                    try {
                        if(cid.StartsWith("00002a24-")) {
                            var model=await Await(ch.ReadValueAsync(BluetoothCacheMode.Uncached));
                            string name=model.Status==GattCommunicationStatus.Success ? Encoding.UTF8.GetString(Bytes(model.Value)).Trim('\0',' ') : "";
                            log.Write("model",new {status=model.Status.ToString(),model=name=="RC003" || name=="RC001" ? name : "unknown"});
                        }
                        // Read only standardized report metadata; do not read arbitrary vendor values.
                        if(cid.StartsWith("00002a4b-")) {
                            var map=await Await(ch.ReadValueAsync(BluetoothCacheMode.Uncached));
                            log.Write("report_map",new {status=map.Status.ToString(),hex=map.Status==GattCommunicationStatus.Success ? BitConverter.ToString(Bytes(map.Value)) : null});
                        }
                        if(cid.StartsWith("00002a4d-")) {
                            var descriptors=await Await(ch.GetDescriptorsForUuidAsync(new Guid("00002908-0000-1000-8000-00805f9b34fb"),BluetoothCacheMode.Uncached));
                            log.Write("report_reference_query",new {handle=handle,status=descriptors.Status.ToString()});
                            foreach(var descriptor in descriptors.Descriptors) {
                                var value=await Await(descriptor.ReadValueAsync(BluetoothCacheMode.Uncached));
                                log.Write("report_reference",new {handle=handle,status=value.Status.ToString(),hex=value.Status==GattCommunicationStatus.Success ? BitConverter.ToString(Bytes(value.Value)) : null});
                            }
                        }
                        var mode=Mode(ch.CharacteristicProperties);
                        if(mode==GattClientCharacteristicConfigurationDescriptorValue.None) continue;
                        // Never subscribe to known voice payloads or start ATVV transmission.
                        if(cid=="ab5e0003-5a21-4f05-bc7d-af01f617b664") { log.Write("gatt_skip_audio",new {handle=handle}); continue; }
                        var before=await Await(ch.ReadClientCharacteristicConfigurationDescriptorAsync());
                        if(before.Status!=GattCommunicationStatus.Success) { log.Write("gatt_skip_unknown_cccd",new {handle=handle,status=before.Status.ToString()}); continue; }
                        var sub=new Subscription {Characteristic=ch,Previous=before.ClientCharacteristicConfigurationDescriptor};
                        sub.Handler=(sender,args)=> {
                            try {
                                byte[] b=Bytes(args.CharacteristicValue); Interlocked.Increment(ref Events);
                                log.Write("gatt_notify",new {service=sid,characteristic=cid,handle=handle,length=b.Length,hex=BitConverter.ToString(b.Take(256).ToArray()),truncated=b.Length>256});
                            } catch(Exception e) { log.Error("gatt_callback",e); }
                        };
                        ch.ValueChanged+=sub.Handler;
                        subscriptions.Add(sub); // Retain even on timeout: cleanup must restore the previous CCCD.
                        var status=await Await(ch.WriteClientCharacteristicConfigurationDescriptorAsync(mode));
                        if(status==GattCommunicationStatus.Success) Subscribed++;
                        log.Write("gatt_subscribe",new {service=sid,characteristic=cid,handle=handle,mode=mode.ToString(),previous=sub.Previous.ToString(),status=status.ToString()});
                    } catch(OperationCanceledException) { throw; }
                    catch(Exception e) { log.Error("gatt_characteristic",e); }
                }
            } catch(OperationCanceledException) { throw; }
            catch(Exception e) { log.Error("gatt_service",e); }
        }
    }
    public async Task Stop() {
        foreach(var sub in subscriptions) {
            try { sub.Characteristic.ValueChanged-=sub.Handler; } catch(Exception e) { log.Error("gatt_unsubscribe",e); }
        }
        // Restore concurrently so shutdown is bounded by one timeout, not N timeouts.
        await Task.WhenAll(subscriptions.Select(async sub=> {
            try { var status=await Wait(sub.Characteristic.WriteClientCharacteristicConfigurationDescriptorAsync(sub.Previous)); if(status!=GattCommunicationStatus.Success) CleanupFailed=true; log.Write("gatt_restore",new {handle=sub.Characteristic.AttributeHandle,status=status.ToString()}); }
            catch(Exception e) { CleanupFailed=true; log.Error("gatt_restore",e); }
        }));
        subscriptions.Clear();
        foreach(var service in services) { try { service.Dispose(); } catch(Exception e) { log.Error("gatt_dispose",e); } }
        services.Clear();
        if(device!=null) { device.Dispose(); device=null; }
    }
}

internal sealed class Diagnostic : Form {
    readonly ComboBox devices=new ComboBox {Left=20,Top=45,Width=650,DropDownStyle=ComboBoxStyle.DropDownList};
    readonly Button scan=new Button {Left=20,Top=85,Width=160,Text="刷新蓝牙设备"};
    readonly Button start=new Button {Left=200,Top=85,Width=210,Text="开始带对照检查的采集"};
    readonly Button stop=new Button {Left=440,Top=85,Width=160,Text="停止并清理",Enabled=false};
    readonly Label status=new Label {Left=20,Top=135,Width=650,Height=130,Text="请选择这台 RC001。开始前正常退出 SayAll，并只保留一台小米遥控器连接。"};
    readonly List<Native.Target> paired=new List<Native.Target>();
    Log log; RawCapture raw; GattCapture gatt; KeyboardCapture keyboard; bool busy; bool finished;
    string output;
    CancellationTokenSource cancellation;
    string invalidReason;
    static readonly string[] Steps={"back_tap","volume_up_tap","volume_down_tap"};
    static readonly string[] Prompts={"按返回键 3 次","按音量＋ 3 次","按音量－ 3 次"};
    public Diagnostic(bool rendering=false) {
        Text="RC001 / RC003 按键诊断 v3 · 对照检查"; Width=710; Height=350; FormBorderStyle=FormBorderStyle.FixedDialog; MaximizeBox=false;
        Controls.Add(new Label {Left=20,Top=15,Width=650,Text="本轮验证 LL / Raw Input；保持本窗口前台，不操作普通键盘。日志保存在 captures。"});
        Controls.AddRange(new Control[]{devices,scan,start,stop,status});
        scan.Click+=async (s,e)=>await Scan(); start.Click+=async (s,e)=>await Run();
        stop.Click+=(s,e)=> { if(cancellation!=null) cancellation.Cancel(); stop.Enabled=false; status.Text="正在停止采集并恢复通知配置，请稍候。"; };
        if(!rendering) Shown+=async (s,e)=>await Scan();
        FormClosing+=(s,e)=> { if(busy) { e.Cancel=true; if(cancellation!=null) cancellation.Cancel(); status.Text="正在停止并清理连接，完成后可关闭。"; } };
    }
    async Task Scan() {
        scan.Enabled=false; start.Enabled=false;
        try {
            var found=await Task.Run(()=>Native.BluetoothTargets());
            paired.Clear(); devices.Items.Clear();
            foreach(var d in found) { paired.Add(d); devices.Items.Add(String.IsNullOrWhiteSpace(d.Label)?"未命名蓝牙设备":d.Label); }
            if(paired.Count>0) devices.SelectedIndex=0;
            status.Text=paired.Count==0?"未发现与小米 HID 接口关联的蓝牙遥控器。请先在 Windows 中连接小米遥控器，再刷新。":"请选择这台 RC001；工具只在本机显示设备名称，日志不保存名称、地址或设备路径。";
        } catch(Exception e) { status.Text="设备枚举失败："+e.GetType().Name+" ("+e.HResult+")"; }
        finally { scan.Enabled=true; start.Enabled=paired.Count>0; }
    }
    async Task Run() {
        if(devices.SelectedIndex<0 || busy) return;
        if(Process.GetProcessesByName("sayall-windows-app").Length>0) { status.Text="检测到 SayAll 正在运行。请从托盘正常退出后，再点击开始；工具不会强制结束它。"; return; }
        busy=true; invalidReason=null; start.Enabled=false; scan.Enabled=false; devices.Enabled=false; stop.Enabled=true;
        cancellation=new CancellationTokenSource(90000);
        string directory=Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"captures");
        try {
            Directory.CreateDirectory(directory); output=Path.Combine(directory,"rc003-"+DateTime.Now.ToString("yyyyMMdd-HHmmss")+"-"+Guid.NewGuid().ToString("N").Substring(0,6)+".jsonl");
            log=new Log(output); log.Write("session",new {version=3,os=Environment.OSVersion.Version.ToString(),pointerBytes=IntPtr.Size,scope="raw_xiaomi_only_ll_candidate_keys_unattributed",audio="known_ATVV_audio_excluded",keyInjection=false});
            raw=new RawCapture(log,true); gatt=new GattCapture(log);
            status.Text="正在准备双通道对照检查。";
            log.Write("gatt_skipped",new {reason="v3_checks_LL_validity_only_reuse_v2_GATT_evidence"});
            cancellation.Token.ThrowIfCancellationRequested(); cancellation.CancelAfter(Timeout.Infinite);
            // Install after GATT setup, keeping the global observation window short.
            keyboard=new KeyboardCapture(log);
            log.Write("capture_ready",new {hidInterfaces=raw.Interfaces,gattSubscriptions=gatt.Subscribed});
            Activate();
            for(int i=0;i<Steps.Length;i++) {
                await ControlCheck(Steps[i]+"_before");
                log.Step=Steps[i]; int r=raw.Events,g=gatt.Events,k=keyboard.Events;
                log.Write("step_start",new {seconds=6});
                for(int remaining=6;remaining>0;remaining--) {
                    status.Text=Prompts[i]+"\n\n剩余 "+remaining+" 秒；请保持此窗口在前台。Raw "+raw.Events+"，LL "+keyboard.Events;
                    for(int t=0;t<10;t++) { CheckForeground(); await Task.Delay(100,cancellation.Token); }
                }
                log.Write("step_end",new {rawEvents=raw.Events-r,llEvents=keyboard.Events-k,gattEvents=gatt.Events-g,note="LL has no device identity; correlation only, not proof of successful mapping"});
                await ControlCheck(Steps[i]+"_after");
            }
            log.Step="cleanup"; finished=true;
        } catch(Exception e) { if(log!=null) log.Error("session",e); status.Text="采集异常："+e.GetType().Name+"。正在清理，详细状态见日志。"; }
        try { if(keyboard!=null) keyboard.Dispose(); if(gatt!=null) await gatt.Stop(); }
        catch(Exception e) { if(log!=null) log.Error("cleanup",e); }
        finally {
            bool cleanupFailed=gatt!=null && gatt.CleanupFailed;
            if(raw!=null) { try { raw.Dispose(); } catch(Exception e) { if(log!=null) log.Error("raw_cleanup",e); } }
            if(log!=null) { log.Write("summary",new {completed=finished,rawEvents=raw==null?0:raw.Events,llEvents=keyboard==null?0:keyboard.Events,gattEvents=gatt==null?0:gatt.Events,mappingVerified=false}); log.Dispose(); }
            raw=null; gatt=null; keyboard=null; log=null; busy=false; stop.Enabled=false;
            cancellation.Dispose(); cancellation=null;
            start.Enabled=true; scan.Enabled=true; devices.Enabled=true;
            status.Text=(finished?"采集完成。":(invalidReason??"采集未完成。"))+"\n日志："+output+"\n"+(cleanupFailed?"部分通知恢复失败，已关闭连接；日志保留了失败状态。":"已完成清理，可重新启动 SayAll。");
            finished=false;
        }
    }
    void CheckForeground() {
        if(Native.GetForegroundWindow()!=Handle) {
            invalidReason="已停止：诊断窗口失去前台，不能据此判断目标键没有事件。";
            log.Write("validation_failed",new {reason="foreground_changed"}); throw new InvalidOperationException("foreground_changed");
        }
    }
    internal static bool PairedControl(int rawDown,int rawUp,int llDown,int llUp) { return rawDown>0 && rawUp>0 && llDown>0 && llUp>0; }
    async Task ControlCheck(string step) {
        log.Step=step; int rd=raw.UpDown,ru=raw.UpUp,kd=keyboard.UpDown,ku=keyboard.UpUp;
        log.Write("control_start",new {timeoutSeconds=15});
        for(int tick=0;tick<150;tick++) {
            CheckForeground();
            status.Text="请按一次遥控器【上方向键】并松开\n两个通道都收到后自动进入下一步；请勿切换窗口。\nRaw="+(raw.UpDown-rd)+"/"+(raw.UpUp-ru)+"，LL="+(keyboard.UpDown-kd)+"/"+(keyboard.UpUp-ku);
            if(PairedControl(raw.UpDown-rd,raw.UpUp-ru,keyboard.UpDown-kd,keyboard.UpUp-ku)) { log.Write("control_passed",new {rawDown=raw.UpDown-rd,rawUp=raw.UpUp-ru,llDown=keyboard.UpDown-kd,llUp=keyboard.UpUp-ku}); await Task.Delay(300,cancellation.Token); return; }
            await Task.Delay(100,cancellation.Token);
        }
        invalidReason="已停止：对照键未在两个通道完整到达，本轮不能用于判定三键不可见。";
        log.Write("validation_failed",new {reason="control_missing",rawDown=raw.UpDown-rd,rawUp=raw.UpUp-ru,llDown=keyboard.UpDown-kd,llUp=keyboard.UpUp-ku});
        throw new InvalidOperationException("control_missing");
    }
    [STAThread] static int Main(string[] args) {
        try {
            if(args.Length>0 && args[0]=="--self-test") { SelfTest(); return 0; }
            if(args.Length==2 && args[0]=="--paired-smoke") {
                using(var l=new Log(args[1])) {
                    var targets=Native.BluetoothTargets(); l.Write("paired_enumeration",new {count=targets.Count}); return 0;
                }
            }
            if(args.Length==2 && args[0]=="--capture-smoke") {
                using(var l=new Log(args[1])) using(var r=new RawCapture(l,true)) using(var k=new KeyboardCapture(l)) {
                    var host=new Form {ShowInTaskbar=false,Opacity=0}; var timer=new System.Windows.Forms.Timer {Interval=2000};
                    timer.Tick+=(s,e)=>host.Close(); host.Shown+=(s,e)=>timer.Start();
                    Application.Run(host); timer.Dispose(); host.Dispose(); l.Write("capture_smoke",new {events=r.Events}); return 0;
                }
            }
            if(args.Length==2 && args[0]=="--render-smoke") {
                Application.EnableVisualStyles(); using(var form=new Diagnostic(true)) {
                    form.devices.Items.Add("RC003（界面排版自检）"); form.devices.SelectedIndex=0;
                    form.StartPosition=FormStartPosition.Manual; form.Left=-10000; form.Show(); Application.DoEvents();
                    using(var bitmap=new System.Drawing.Bitmap(form.Width,form.Height)) { form.DrawToBitmap(bitmap,new System.Drawing.Rectangle(0,0,form.Width,form.Height)); bitmap.Save(args[1]); }
                } return 0;
            }
            if(args.Length==2 && args[0]=="--inventory") { using(var l=new Log(args[1])) using(var r=new RawCapture(l,false)) { l.Write("inventory_complete",new {interfaces=r.Interfaces}); } return 0; }
            Application.EnableVisualStyles(); Application.Run(new Diagnostic()); return 0;
        } catch(Exception e) { if(args.Length==0) MessageBox.Show(e.GetType().Name+" "+e.HResult,"诊断工具错误"); return 1; }
    }
    static void Check(bool value) { if(!value) throw new InvalidOperationException("self test failed"); }
    static void SelfTest() {
        Check(Marshal.SizeOf(typeof(Native.Header))==24); Check(Marshal.SizeOf(typeof(Native.Caps))==64);
        Check(Marshal.SizeOf(typeof(KeyboardCapture.Edge))==24); Check(KeyboardCapture.Candidate(0xFF)); Check(!KeyboardCapture.Candidate(0x41));
        Check(!PairedControl(3,3,0,0)); Check(!PairedControl(1,1,1,0)); Check(PairedControl(1,1,1,1));
        Check(Native.Matches("HID#Dev_VID&012717_PID&32B8_x")); Check(!Native.Matches("HID#VID_1234_PID_32B8"));
        Check(GattCapture.Mode(GattCharacteristicProperties.Indicate)==GattClientCharacteristicConfigurationDescriptorValue.Indicate);
        Check(GattCapture.Mode(GattCharacteristicProperties.Read)==GattClientCharacteristicConfigurationDescriptorValue.None);
        Check(GattCapture.Mode(GattCharacteristicProperties.Notify|GattCharacteristicProperties.Indicate)==GattClientCharacteristicConfigurationDescriptorValue.Notify);
        var reports=RawCapture.SplitReports(new byte[]{2,0,0,0,2,0,0,0,1,2,3,4}); Check(reports.Count==2 && reports[1][1]==4);
        bool rejected=false; try { RawCapture.SplitReports(new byte[]{255,255,255,255,255,255,255,255}); } catch(InvalidDataException) { rejected=true; } Check(rejected);
        foreach(var invalid in new[]{new byte[0],new byte[7],new byte[8],new byte[]{2,0,0,0,1,0,0,0,1}}) { rejected=false; try {RawCapture.SplitReports(invalid);} catch(InvalidDataException) {rejected=true;} Check(rejected); }
        using(var writer=new DataWriter()) { writer.WriteBytes(new byte[]{0xF1,0x80,0x81}); Check(GattCapture.Bytes(writer.DetachBuffer()).SequenceEqual(new byte[]{0xF1,0x80,0x81})); }
    }
}

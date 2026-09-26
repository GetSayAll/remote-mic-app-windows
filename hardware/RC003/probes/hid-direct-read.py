"""RC003 免驱动通道探针 A'：直接打开 HID 顶层集合（TLC）读原始输入报告。

动机
----
2026-09-22 的 E1 判定「Raw Input 对该设备零 HID 报文」。但 Raw Input 只是
kbdhid -> kbdclass -> RIM 这条链的**末端消费点**；它零报文只说明"报告没走到
RIM"，不说明"报告不存在"。

本探针走的是**另一条完全独立**的路径：用 SetupAPI 找到该设备的 HID 设备接口，
直接 CreateFile + ReadFile 从 HIDClass 的输入报告环形缓冲取报告。这条路径
不经过 kbdhid 的按键解码，因此理论上仍可能看到被 kbdhid 丢弃的 usage。

（对应第二轮选型文档里"按设备源头捕获"的**最轻**形态：不需要内核驱动、
不需要 TESTSIGNING、不需要管理员权限——前提是 CreateFile 能成功。）

回答三个问题
------------
 1. 普通用户态进程能否打开该 TLC？（CreateFile 返回 ACCESS_DENIED 则路线终止）
 2. 能否读到 HID 报告描述符？（它直接决定三键的 usage 是否被声明）
 3. 按 返回 / 音量± 时，ReadFile 能否收到输入报告？

安全性
------
全程只读：CreateFile 只请求 GENERIC_READ，仅使用
IOCTL_HID_GET_REPORT_DESCRIPTOR（只读）与 ReadFile。不写任何报告、不改注册表。
设备路径与蓝牙地址一律掩码输出，不落盘真实地址。

用法
----
  python hid-direct-read.py list                      # 只枚举，不打开
  python hid-direct-read.py listen --seconds 120      # 打开并采集
所有输出同时打印到 stdout 与 --out 指定的文件。
"""
import ctypes
import ctypes.wintypes as wt
import os
import re
import sys
import time

# ---------- 常量 ----------

GUID_DEVINTERFACE_HID = "{4d1e55b2-f16f-11cf-88cb-001111000030}"
GUID_DEVINTERFACE_KEYBOARD = "{884b96c3-56ef-11d1-bc8c-00a0c91405dd}"

DIGCF_PRESENT = 0x00000002
DIGCF_DEVICEINTERFACE = 0x00000010

GENERIC_READ = 0x80000000
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
OPEN_EXISTING = 3
FILE_FLAG_OVERLAPPED = 0x40000000

ERROR_IO_PENDING = 997
WAIT_OBJECT_0 = 0
WAIT_TIMEOUT = 258

IOCTL_HID_GET_REPORT_DESCRIPTOR = 0x000B0190

# 关注项：小米 VID / RC003 PID
VID_MATCH = "vid&012717"
PID_MATCH = "pid&32b8"

# 相关 HID usage（键盘页 0x07；消费类页 0x0C 仅作参考）
KEYBOARD_USAGES = {
    0x35: "`~ / Live(TV)",
    0x3E: "F5 / Voice",
    0x49: "Insert",
    0x4A: "Home",
    0x65: "Application(Menu)",
    0x66: "Power",
    0x75: "Help",
    0x80: "Volume Up  <<< 本调查关注",
    0x81: "Volume Down <<< 本调查关注",
    0xF1: "Back  <<< 本调查关注",
}
CONSUMER_USAGES = {
    0xE9: "Volume Increment",
    0xEA: "Volume Decrement",
    0xCD: "Play/Pause",
    0x226: "AL Consumer Control Config",
    0x221: "AC Search",
    0x223: "AC Home",
    0x224: "AC Back",
}

# ---------- Win32 绑定 ----------

setupapi = ctypes.WinDLL("setupapi", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
hid = ctypes.WinDLL("hid", use_last_error=True)


class GUID(ctypes.Structure):
    _fields_ = [
        ("Data1", ctypes.c_ulong),
        ("Data2", ctypes.c_ushort),
        ("Data3", ctypes.c_ushort),
        ("Data4", ctypes.c_ubyte * 8),
    ]

    @staticmethod
    def parse(text):
        g = GUID()
        ok = ctypes.windll.ole32.CLSIDFromString(ctypes.c_wchar_p(text), ctypes.byref(g))
        if ok != 0:
            raise OSError("CLSIDFromString failed: " + text)
        return g


class SP_DEVICE_INTERFACE_DATA(ctypes.Structure):
    _fields_ = [
        ("cbSize", wt.DWORD),
        ("InterfaceClassGuid", GUID),
        ("Flags", wt.DWORD),
        ("Reserved", ctypes.POINTER(ctypes.c_ulong)),
    ]


class SP_DEVINFO_DATA(ctypes.Structure):
    _fields_ = [
        ("cbSize", wt.DWORD),
        ("ClassGuid", GUID),
        ("DevInst", wt.DWORD),
        ("Reserved", ctypes.POINTER(ctypes.c_ulong)),
    ]


class HIDD_ATTRIBUTES(ctypes.Structure):
    _fields_ = [
        ("Size", ctypes.c_ulong),
        ("VendorID", ctypes.c_ushort),
        ("ProductID", ctypes.c_ushort),
        ("VersionNumber", ctypes.c_ushort),
    ]


class HIDP_CAPS(ctypes.Structure):
    _fields_ = [
        ("Usage", ctypes.c_ushort),
        ("UsagePage", ctypes.c_ushort),
        ("InputReportByteLength", ctypes.c_ushort),
        ("OutputReportByteLength", ctypes.c_ushort),
        ("FeatureReportByteLength", ctypes.c_ushort),
        ("Reserved", ctypes.c_ushort * 17),
        ("NumberLinkCollectionNodes", ctypes.c_ushort),
        ("NumberInputButtonCaps", ctypes.c_ushort),
        ("NumberInputValueCaps", ctypes.c_ushort),
        ("NumberInputDataIndices", ctypes.c_ushort),
        ("NumberOutputButtonCaps", ctypes.c_ushort),
        ("NumberOutputValueCaps", ctypes.c_ushort),
        ("NumberOutputDataIndices", ctypes.c_ushort),
        ("NumberFeatureButtonCaps", ctypes.c_ushort),
        ("NumberFeatureValueCaps", ctypes.c_ushort),
        ("NumberFeatureDataIndices", ctypes.c_ushort),
    ]


class HIDP_BUTTON_CAPS_RANGE(ctypes.Structure):
    _fields_ = [
        ("UsageMin", ctypes.c_ushort), ("UsageMax", ctypes.c_ushort),
        ("StringMin", ctypes.c_ushort), ("StringMax", ctypes.c_ushort),
        ("DesignatorMin", ctypes.c_ushort), ("DesignatorMax", ctypes.c_ushort),
        ("DataIndexMin", ctypes.c_ushort), ("DataIndexMax", ctypes.c_ushort),
    ]


class HIDP_BUTTON_CAPS_NOTRANGE(ctypes.Structure):
    _fields_ = [
        ("Usage", ctypes.c_ushort), ("Reserved1", ctypes.c_ushort),
        ("StringIndex", ctypes.c_ushort), ("Reserved2", ctypes.c_ushort),
        ("DesignatorIndex", ctypes.c_ushort), ("Reserved3", ctypes.c_ushort),
        ("DataIndex", ctypes.c_ushort), ("Reserved4", ctypes.c_ushort),
    ]


class HIDP_BUTTON_CAPS_UNION(ctypes.Union):
    _fields_ = [("Range", HIDP_BUTTON_CAPS_RANGE), ("NotRange", HIDP_BUTTON_CAPS_NOTRANGE)]


class HIDP_BUTTON_CAPS(ctypes.Structure):
    _fields_ = [
        ("UsagePage", ctypes.c_ushort),
        ("ReportID", ctypes.c_ubyte),
        ("IsAlias", ctypes.c_ubyte),
        ("BitField", ctypes.c_ushort),
        ("LinkCollection", ctypes.c_ushort),
        ("LinkUsage", ctypes.c_ushort),
        ("LinkUsagePage", ctypes.c_ushort),
        ("IsRange", ctypes.c_ubyte),
        ("IsStringRange", ctypes.c_ubyte),
        ("IsDesignatorRange", ctypes.c_ubyte),
        ("IsAbsolute", ctypes.c_ubyte),
        ("Reserved", ctypes.c_ulong * 10),
        ("u", HIDP_BUTTON_CAPS_UNION),
    ]


class HIDP_LINK_COLLECTION_NODE(ctypes.Structure):
    _fields_ = [
        ("LinkUsage", ctypes.c_ushort),
        ("LinkUsagePage", ctypes.c_ushort),
        ("Parent", ctypes.c_ushort),
        ("NumberOfChildren", ctypes.c_ushort),
        ("NextSibling", ctypes.c_ushort),
        ("FirstChild", ctypes.c_ushort),
        ("Reserved", ctypes.c_ulong * 8),
        ("UserContext", ctypes.c_void_p),
    ]


class OVERLAPPED(ctypes.Structure):
    _fields_ = [
        ("Internal", ctypes.POINTER(ctypes.c_ulong)),
        ("InternalHigh", ctypes.POINTER(ctypes.c_ulong)),
        ("Offset", wt.DWORD),
        ("OffsetHigh", wt.DWORD),
        ("hEvent", wt.HANDLE),
    ]


setupapi.SetupDiGetClassDevsW.restype = wt.HANDLE
setupapi.SetupDiGetClassDevsW.argtypes = [ctypes.POINTER(GUID), wt.LPCWSTR, wt.HWND, wt.DWORD]
setupapi.SetupDiEnumDeviceInterfaces.argtypes = [
    wt.HANDLE, ctypes.POINTER(SP_DEVINFO_DATA), ctypes.POINTER(GUID), wt.DWORD,
    ctypes.POINTER(SP_DEVICE_INTERFACE_DATA),
]
setupapi.SetupDiGetDeviceInterfaceDetailW.argtypes = [
    wt.HANDLE, ctypes.POINTER(SP_DEVICE_INTERFACE_DATA), ctypes.c_void_p, wt.DWORD,
    ctypes.POINTER(wt.DWORD), ctypes.POINTER(SP_DEVINFO_DATA),
]
setupapi.SetupDiDestroyDeviceInfoList.argtypes = [wt.HANDLE]

kernel32.CreateFileW.restype = wt.HANDLE
kernel32.CreateFileW.argtypes = [
    wt.LPCWSTR, wt.DWORD, wt.DWORD, ctypes.c_void_p, wt.DWORD, wt.DWORD, wt.HANDLE,
]
kernel32.ReadFile.argtypes = [
    wt.HANDLE, ctypes.c_void_p, wt.DWORD, ctypes.POINTER(wt.DWORD), ctypes.POINTER(OVERLAPPED),
]
kernel32.DeviceIoControl.argtypes = [
    wt.HANDLE, wt.DWORD, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, wt.DWORD,
    ctypes.POINTER(wt.DWORD), ctypes.POINTER(OVERLAPPED),
]
kernel32.CreateEventW.restype = wt.HANDLE
kernel32.CreateEventW.argtypes = [ctypes.c_void_p, wt.BOOL, wt.BOOL, wt.LPCWSTR]
kernel32.WaitForSingleObject.argtypes = [wt.HANDLE, wt.DWORD]
kernel32.GetOverlappedResult.argtypes = [
    wt.HANDLE, ctypes.POINTER(OVERLAPPED), ctypes.POINTER(wt.DWORD), wt.BOOL,
]
kernel32.CancelIoEx.argtypes = [wt.HANDLE, ctypes.POINTER(OVERLAPPED)]

hid.HidD_GetAttributes.argtypes = [wt.HANDLE, ctypes.POINTER(HIDD_ATTRIBUTES)]
hid.HidD_GetPreparsedData.argtypes = [wt.HANDLE, ctypes.POINTER(ctypes.c_void_p)]
hid.HidD_FreePreparsedData.argtypes = [ctypes.c_void_p]
hid.HidP_GetCaps.argtypes = [ctypes.c_void_p, ctypes.POINTER(HIDP_CAPS)]
hid.HidD_GetProductString.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_ulong]
hid.HidP_GetButtonCaps.argtypes = [
    ctypes.c_int, ctypes.POINTER(HIDP_BUTTON_CAPS), ctypes.POINTER(ctypes.c_ushort), ctypes.c_void_p,
]
hid.HidP_GetLinkCollectionNodes.argtypes = [
    ctypes.POINTER(HIDP_LINK_COLLECTION_NODE), ctypes.POINTER(ctypes.c_ulong), ctypes.c_void_p,
]

HIDP_INPUT = 0
HIDP_STATUS_SUCCESS = 0x00110000

INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

# ---------- 输出 ----------

_out = []


def log(line=""):
    text = str(line)
    _out.append(text)
    print(text, flush=True)


def mask_path(path):
    """掩码蓝牙地址与实例后缀，只保留可复现的结构信息。"""
    masked = re.sub(r"[0-9a-fA-F]{12}", "<BT-ADDR>", path)
    return masked


def hexs(data, limit=None):
    chunk = data if limit is None else data[:limit]
    return " ".join(f"{b:02X}" for b in chunk)


# ---------- 枚举 ----------

def enumerate_interfaces(guid_text):
    """返回 (原始路径, 掩码路径) 列表。"""
    guid = GUID.parse(guid_text)
    result = []
    handle = setupapi.SetupDiGetClassDevsW(
        ctypes.byref(guid), None, None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE
    )
    if handle == INVALID_HANDLE_VALUE:
        log(f"  SetupDiGetClassDevs 失败 err={ctypes.get_last_error()}")
        return result
    try:
        index = 0
        while True:
            data = SP_DEVICE_INTERFACE_DATA()
            data.cbSize = ctypes.sizeof(SP_DEVICE_INTERFACE_DATA)
            ok = setupapi.SetupDiEnumDeviceInterfaces(
                handle, None, ctypes.byref(guid), index, ctypes.byref(data)
            )
            if not ok:
                break
            needed = wt.DWORD(0)
            setupapi.SetupDiGetDeviceInterfaceDetailW(
                handle, ctypes.byref(data), None, 0, ctypes.byref(needed), None
            )
            buffer = ctypes.create_string_buffer(needed.value)
            # cbSize 是"固定部分"大小：x64 上 8 字节（含 4 字节 cbSize + 对齐）
            ctypes.memmove(buffer, ctypes.byref(wt.DWORD(8)), 4)
            info = SP_DEVINFO_DATA()
            info.cbSize = ctypes.sizeof(SP_DEVINFO_DATA)
            ok = setupapi.SetupDiGetDeviceInterfaceDetailW(
                handle, ctypes.byref(data), buffer, needed.value,
                ctypes.byref(needed), ctypes.byref(info),
            )
            if ok:
                path = ctypes.wstring_at(ctypes.addressof(buffer) + 4)
                result.append(path)
            index += 1
    finally:
        setupapi.SetupDiDestroyDeviceInfoList(handle)
    return result


def registry_paths():
    """从注册表构造设备接口路径（枚举被系统隐藏时的兜底）。"""
    import winreg
    found = []
    root = r"SYSTEM\CurrentControlSet\Enum\HID"
    try:
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, root, 0, winreg.KEY_READ)
    except OSError:
        return found
    try:
        i = 0
        while True:
            try:
                name = winreg.EnumKey(key, i)
            except OSError:
                break
            i += 1
            low = name.lower()
            if VID_MATCH not in low or PID_MATCH not in low:
                continue
            sub = winreg.OpenKey(key, name, 0, winreg.KEY_READ)
            j = 0
            while True:
                try:
                    inst = winreg.EnumKey(sub, j)
                except OSError:
                    break
                j += 1
                for cls in (GUID_DEVINTERFACE_HID, GUID_DEVINTERFACE_KEYBOARD):
                    found.append(f"\\\\?\\hid#{low}#{inst.lower()}#{cls}")
    finally:
        winreg.CloseKey(key)
    return found


# ---------- HID 打开与信息 ----------

def open_device(path):
    handle = kernel32.CreateFileW(
        path, GENERIC_READ | 0x80000000, FILE_SHARE_READ | FILE_SHARE_WRITE,
        None, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, None,
    )
    if not handle or handle == INVALID_HANDLE_VALUE:
        return None, ctypes.get_last_error()
    return handle, 0


def describe_device(handle):
    attributes = HIDD_ATTRIBUTES()
    attributes.Size = ctypes.sizeof(HIDD_ATTRIBUTES)
    if hid.HidD_GetAttributes(handle, ctypes.byref(attributes)):
        log(f"    HidD_GetAttributes: VID=0x{attributes.VendorID:04X} "
            f"PID=0x{attributes.ProductID:04X} ver=0x{attributes.VersionNumber:04X}")

    product = ctypes.create_unicode_buffer(128)
    if hid.HidD_GetProductString(handle, product, ctypes.sizeof(product)):
        log(f"    HidD_GetProductString: \"{product.value}\"")

    preparsed = ctypes.c_void_p()
    if not hid.HidD_GetPreparsedData(handle, ctypes.byref(preparsed)):
        log(f"    HidD_GetPreparsedData 失败 err={ctypes.get_last_error()}")
        return None
    try:
        caps = HIDP_CAPS()
        status = hid.HidP_GetCaps(preparsed, ctypes.byref(caps))
        if status != 0x00110000:  # HIDP_STATUS_SUCCESS
            log(f"    HidP_GetCaps 失败 status=0x{status:08X}")
            return None
        log(f"    HidP_GetCaps: UsagePage=0x{caps.UsagePage:04X} Usage=0x{caps.Usage:04X}")
        log(f"      InputReportByteLength={caps.InputReportByteLength} "
            f"Output={caps.OutputReportByteLength} Feature={caps.FeatureReportByteLength}")
        log(f"      LinkCollectionNodes={caps.NumberLinkCollectionNodes} "
            f"InputButtonCaps={caps.NumberInputButtonCaps} InputValueCaps={caps.NumberInputValueCaps}")
        return caps
    finally:
        hid.HidD_FreePreparsedData(preparsed)


def read_report_descriptor(handle):
    buffer = ctypes.create_string_buffer(4096)
    returned = wt.DWORD(0)
    ok = kernel32.DeviceIoControl(
        handle, IOCTL_HID_GET_REPORT_DESCRIPTOR, None, 0,
        buffer, ctypes.sizeof(buffer), ctypes.byref(returned), None,
    )
    if not ok:
        log(f"    报告描述符读取失败 err={ctypes.get_last_error()}")
        return None
    size = returned.value
    data = bytes(buffer.raw[:size])
    log(f"    报告描述符: {size} 字节")
    for offset in range(0, size, 16):
        log(f"      {offset:04X}  {hexs(data[offset:offset + 16])}")
    return data


def describe_report_map(data):
    """最小 HID 报告描述符解析：只抽取 usage / usage page / report 结构。"""
    index = 0
    usage_page = 0
    report_id = 0
    report_size = 0
    report_count = 0
    usages = []
    within_usage_page = 0

    def value_from(size, raw):
        if size == 0:
            return 0
        if size == 1:
            return raw
        if size == 2:
            return int.from_bytes(raw.to_bytes(2, "little"), "little")
        return int.from_bytes(raw.to_bytes(4, "little"), "little")

    while index < len(data):
        prefix = data[index]
        index += 1
        if prefix == 0xFE:  # long item
            if index + 1 >= len(data):
                break
            data_size = data[index]
            index += 2 + data_size
            continue
        size = prefix & 0x03
        size = 4 if size == 3 else size
        kind = (prefix >> 2) & 0x03
        tag = (prefix >> 4) & 0x0F
        raw = 0
        for shift in range(size):
            if index < len(data):
                raw |= data[index] << (8 * shift)
            index += 1
        value = value_from(size, raw)

        if kind == 1:  # Global
            if tag == 0x0:
                usage_page = value & 0xFFFF
                log(f"  [Global] Usage Page = 0x{usage_page:04X}")
            elif tag == 0x8:
                report_id = value & 0xFF
                log(f"  [Global] Report ID = 0x{report_id:02X}")
            elif tag == 0x7:
                report_size = value
            elif tag == 0x9:
                report_count = value
        elif kind == 2:  # Local
            if tag in (0x0, 0x1, 0x2):
                if size == 4:
                    within_usage_page = (value >> 16) & 0xFFFF
                    usages.append(value & 0xFFFF)
                else:
                    usages.append(value & 0xFFFF)
        elif kind == 0:  # Main
            if tag == 0x8:
                log(f"  [Main] Input  report_id=0x{report_id:02X} "
                    f"size={report_size} count={report_count}")
                dump_usages(usages)
                usages = []
            elif tag == 0x9:
                log(f"  [Main] Output report_id=0x{report_id:02X} "
                    f"size={report_size} count={report_count}")
                usages = []
            elif tag == 0xA:
                log(f"  [Main] Feature report_id=0x{report_id:02X} "
                    f"size={report_size} count={report_count}")
                usages = []
            elif tag == 0xB:
                page = within_usage_page or usage_page
                label = ""
                if page == 0x07 and usages:
                    label = KEYBOARD_USAGES.get(usages[-1], "")
                log(f"  [Main] Collection type=0x{value:02X} page=0x{page:04X} {label}")
                usages = []
            elif tag == 0xC:
                log("  [Main] End Collection")


def usage_name(page, usage):
    if page == 0x07:
        return KEYBOARD_USAGES.get(usage, "")
    if page == 0x0C:
        return CONSUMER_USAGES.get(usage, "")
    return ""


def dump_declared_usages(preparsed, caps):
    """从 preparsed data 枚举**声明**在报告里的 usage —— 不需要读报告本身。"""
    count = caps.NumberInputButtonCaps
    log(f"      声明为「按钮数组」的 caps 数: {count}")
    if count:
        array = (HIDP_BUTTON_CAPS * count)()
        length = ctypes.c_ushort(count)
        status = hid.HidP_GetButtonCaps(HIDP_INPUT, array, ctypes.byref(length), preparsed)
        if status != HIDP_STATUS_SUCCESS:
            log(f"      HidP_GetButtonCaps 失败 status=0x{status:08X}")
        else:
            for index in range(length.value):
                entry = array[index]
                page = entry.UsagePage
                if entry.IsRange:
                    low = entry.u.Range.UsageMin
                    high = entry.u.Range.UsageMax
                    log(f"        [caps {index}] page=0x{page:04X} report_id=0x{entry.ReportID:02X} "
                        f"link={entry.LinkCollection} range=0x{low:04X}-0x{high:04X}")
                    hits = [u for u in KEYBOARD_USAGES if low <= u <= high] if page == 0x07 else []
                    hits += [u for u in CONSUMER_USAGES if low <= u <= high] if page == 0x0C else []
                    for usage in sorted(hits):
                        log(f"            包含 usage=0x{usage:04X} {usage_name(page, usage)}")
                else:
                    usage = entry.u.NotRange.Usage
                    log(f"        [caps {index}] page=0x{page:04X} report_id=0x{entry.ReportID:02X} "
                        f"link={entry.LinkCollection} usage=0x{usage:04X} {usage_name(page, usage)}")

    nodes = caps.NumberLinkCollectionNodes
    log(f"      LinkCollection 节点数: {nodes}")
    if nodes:
        buffer = (HIDP_LINK_COLLECTION_NODE * nodes)()
        length = ctypes.c_ulong(nodes)
        status = hid.HidP_GetLinkCollectionNodes(buffer, ctypes.byref(length), preparsed)
        if status != HIDP_STATUS_SUCCESS:
            log(f"      HidP_GetLinkCollectionNodes 失败 status=0x{status:08X}")
        else:
            for index in range(length.value):
                node = buffer[index]
                label = usage_name(node.LinkUsagePage, node.LinkUsage)
                log(f"        [link {index}] page=0x{node.LinkUsagePage:04X} "
                    f"usage=0x{node.LinkUsage:04X} parent={node.Parent} "
                    f"children={node.NumberOfChildren} {label}")


def dump_usages(usages):
    for usage in usages:
        name = ""
        if usage in KEYBOARD_USAGES:
            name = KEYBOARD_USAGES[usage]
        elif usage in CONSUMER_USAGES:
            name = CONSUMER_USAGES[usage]
        marker = " <<<" if name.endswith("本调查关注") else ""
        log(f"         usage=0x{usage:04X} {name}{marker}")


# ---------- 采集 ----------

def listen(handle, seconds, origin):
    buffer_size = 4096
    buffer = ctypes.create_string_buffer(buffer_size)
    deadline = time.time() + seconds
    reports = 0
    while time.time() < deadline:
        event = kernel32.CreateEventW(None, True, False, None)
        overlapped = OVERLAPPED()
        overlapped.hEvent = event
        read = wt.DWORD(0)
        ok = kernel32.ReadFile(handle, buffer, buffer_size, ctypes.byref(read), ctypes.byref(overlapped))
        if not ok:
            error = ctypes.get_last_error()
            if error != ERROR_IO_PENDING:
                log(f"ReadFile 失败 err={error}")
                kernel32.CloseHandle(event)
                return reports
            wait = kernel32.WaitForSingleObject(event, 250)
            if wait != WAIT_OBJECT_0:
                kernel32.CancelIoEx(handle, ctypes.byref(overlapped))
                kernel32.WaitForSingleObject(event, 1000)
                kernel32.CloseHandle(event)
                continue
            transferred = wt.DWORD(0)
            if not kernel32.GetOverlappedResult(handle, ctypes.byref(overlapped), ctypes.byref(transferred), False):
                kernel32.CloseHandle(event)
                continue
            read = transferred
        elapsed = time.time() - origin
        data = bytes(buffer.raw[:read.value])
        reports += 1
        log(f"HID-REPORT t={elapsed:>7.3f}s len={len(data)} b=[{hexs(data, 32)}]")
        kernel32.CloseHandle(event)
    return reports


# ---------- 主流程 ----------

def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "list"
    seconds = 120
    out_path = None
    index = 2
    while index < len(sys.argv):
        if sys.argv[index] == "--seconds" and index + 1 < len(sys.argv):
            seconds = int(sys.argv[index + 1])
            index += 2
        elif sys.argv[index] == "--out" and index + 1 < len(sys.argv):
            out_path = sys.argv[index + 1]
            index += 2
        else:
            index += 1

    log(f"=== hid-direct-read mode={mode} 开始 {time.strftime('%Y-%m-%dT%H:%M:%S')} ===")

    log("\n[1] 按 GUID_DEVINTERFACE_HID 枚举设备接口:")
    hid_paths = enumerate_interfaces(GUID_DEVINTERFACE_HID)
    log(f"    共 {len(hid_paths)} 个 HID 设备接口")
    log("\n[2] 按 GUID_DEVINTERFACE_KEYBOARD 枚举:")
    keyboard_paths = enumerate_interfaces(GUID_DEVINTERFACE_KEYBOARD)
    log(f"    共 {len(keyboard_paths)} 个键盘设备接口")

    candidates = []
    for path in hid_paths + keyboard_paths:
        low = path.lower()
        if VID_MATCH in low and PID_MATCH in low:
            if path not in candidates:
                candidates.append(path)

    log("\n[3] 匹配 RC003 的接口:")
    for path in candidates:
        log(f"    {mask_path(path)}")

    if not candidates:
        log("    枚举未返回 RC003 接口（键盘类 TLC 可能被系统从枚举中隐藏）")
        log("\n[4] 改用注册表构造路径兜底:")
        candidates = registry_paths()
        for path in candidates:
            log(f"    {mask_path(path)}")

    if not candidates:
        log("结论: 无法定位 RC003 的 HID 设备接口路径")
        return

    if mode == "list":
        log("\n[4] 阳性对照：逐个尝试以只读方式打开全部 HID 接口")
        log("    目的：证明 CreateFile 失败是「键盘类 TLC 被系统独占」，而不是探针写错了。")
        log("    只做 CreateFile + HidD_GetAttributes + HidP_GetCaps，不做任何写入。")
        opened = 0
        denied = 0
        for path in hid_paths:
            handle, error = open_device(path)
            tail = path.rsplit("#", 1)[-1]
            if handle is None:
                denied += 1
                log(f"    [拒绝] err={error} {tail[:40]}")
                continue
            opened += 1
            try:
                attributes = HIDD_ATTRIBUTES()
                attributes.Size = ctypes.sizeof(HIDD_ATTRIBUTES)
                ok = hid.HidD_GetAttributes(handle, ctypes.byref(attributes))
                vid = f"{attributes.VendorID:04X}" if ok else "????"
                pid = f"{attributes.ProductID:04X}" if ok else "????"
                preparsed = ctypes.c_void_p()
                usage = ""
                if hid.HidD_GetPreparsedData(handle, ctypes.byref(preparsed)):
                    caps = HIDP_CAPS()
                    if hid.HidP_GetCaps(preparsed, ctypes.byref(caps)) == 0x00110000:
                        kind = ""
                        if caps.UsagePage == 0x01 and caps.Usage == 0x06:
                            kind = " <-- 键盘类"
                        elif caps.UsagePage == 0x01 and caps.Usage == 0x02:
                            kind = " <-- 鼠标类"
                        elif caps.UsagePage == 0x0C:
                            kind = " <-- 消费类"
                        usage = (f" UP=0x{caps.UsagePage:04X} U=0x{caps.Usage:04X} "
                                 f"InLen={caps.InputReportByteLength}{kind}")
                    hid.HidD_FreePreparsedData(preparsed)
                log(f"    [打开] VID={vid} PID={pid}{usage}")
            finally:
                kernel32.CloseHandle(handle)
        log(f"\n    汇总: 打开成功 {opened} 个 / 拒绝 {denied} 个")

        log("\n[5] 零权限打开测试（CreateFile dwDesiredAccess=0）")
        log("    微软文档：RIM 以独占方式打开键盘/鼠标 TLC 后，用户态仍可以"
            "「不请求读写权限」的方式打开接口，")
        log("    并可用 HidD_GetXxx 读取设备信息；但不能收发报告。验证 RC003 是否走这条路：")
        for path in (candidates or registry_paths()):
            handle = kernel32.CreateFileW(
                path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, 0, None
            )
            if not handle or handle == INVALID_HANDLE_VALUE:
                log(f"    [零权限拒绝] err={ctypes.get_last_error()} {mask_path(path)[-24:]}")
                continue
            try:
                attributes = HIDD_ATTRIBUTES()
                attributes.Size = ctypes.sizeof(HIDD_ATTRIBUTES)
                ok = hid.HidD_GetAttributes(handle, ctypes.byref(attributes))
                product = ctypes.create_unicode_buffer(128)
                hid.HidD_GetProductString(handle, product, ctypes.sizeof(product))
                log(f"    [零权限成功] VID=0x{attributes.VendorID:04X} "
                    f"PID=0x{attributes.ProductID:04X} "
                    f"ver=0x{attributes.VersionNumber:04X} product=\"{product.value}\"" if ok
                    else "    [零权限成功] HidD_GetAttributes 失败")
                preparsed = ctypes.c_void_p()
                if hid.HidD_GetPreparsedData(handle, ctypes.byref(preparsed)):
                    caps = HIDP_CAPS()
                    if hid.HidP_GetCaps(preparsed, ctypes.byref(caps)) == 0x00110000:
                        log(f"      HidP_GetCaps: UP=0x{caps.UsagePage:04X} U=0x{caps.Usage:04X} "
                            f"InLen={caps.InputReportByteLength} "
                            f"InputButtonCaps={caps.NumberInputButtonCaps} "
                            f"InputValueCaps={caps.NumberInputValueCaps} "
                            f"LinkCollectionNodes={caps.NumberLinkCollectionNodes}")
                        dump_declared_usages(preparsed, caps)
                    else:
                        log("      HidP_GetCaps 失败")
                    hid.HidD_FreePreparsedData(preparsed)
                else:
                    log("      HidD_GetPreparsedData 失败")
                buffer = ctypes.create_string_buffer(4096)
                returned = wt.DWORD(0)
                ok = kernel32.DeviceIoControl(
                    handle, IOCTL_HID_GET_REPORT_DESCRIPTOR, None, 0,
                    buffer, ctypes.sizeof(buffer), ctypes.byref(returned), None,
                )
                if ok and returned.value:
                    log(f"      报告描述符可达: {returned.value} 字节")
                else:
                    log(f"      报告描述符不可达（IOCTL_HID_GET_REPORT_DESCRIPTOR err="
                        f"{ctypes.get_last_error()}）—— 与「零权限仅 HidD_GetXxx」一致")
            finally:
                kernel32.CloseHandle(handle)

        log("\n=== list 模式结束 ===")
        if out_path:
            with open(out_path, "w", encoding="utf-8") as file:
                file.write("\n".join(_out) + "\n")
        return

    log("\n[5] 尝试打开每个候选接口（GENERIC_READ + FILE_FLAG_OVERLAPPED）:")
    origin = time.time()
    for path in candidates:
        log(f"\n  --- {mask_path(path)}")
        handle, error = open_device(path)
        if handle is None:
            log(f"    CreateFile 失败 err={error} "
                f"({ctypes.FormatError(error).strip() if error else '未知'})")
            continue
        log("    CreateFile 成功")
        try:
            caps = describe_device(handle)
            descriptor = read_report_descriptor(handle)
            if descriptor:
                log("    ---- 报告描述符解析 ----")
                describe_report_map(descriptor)
                log("    ---- 解析结束 ----")
            if caps is None:
                log("    无 caps，跳过采集")
                continue
            log(f"\n[6] 开始采集 {seconds} 秒：请按【返回】1 次、【音量+】2 次、"
                f"【音量-】3 次、【确定】2 次（每段间隔 3 秒以上）")
            reports = listen(handle, seconds, origin)
            log(f"=== 采集结束，收到 {reports} 条输入报告 ===")
        finally:
            kernel32.CloseHandle(handle)

    log(f"\n总用时 {time.time() - origin:.1f}s")

    if out_path:
        with open(out_path, "w", encoding="utf-8") as file:
            file.write("\n".join(_out) + "\n")


if __name__ == "__main__":
    main()

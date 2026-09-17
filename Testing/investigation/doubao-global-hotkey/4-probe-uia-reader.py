"""读取第三方（豆包输入法）设置窗口的 UI 控件树，用于核对界面文案与选项。

为什么需要它：跨应用调研中「配置键名 ≠ 界面文案」。本脚本用**公开 UI
Automation**（COM）读取活着的窗口，逐字拿到用户实际看到的文本，
避免靠截图 OCR 或 DLL 字符串推断（2026-09-17 曾因此把「免按模式」误读成
「免提模式」，并据此写错文档与代码）。

边界（AGENTS.md）：只读 UIA 控件树，**不读取也不修改**第三方私有配置、
内部数据库或内存。

本机注意：
- **没有 `comtypes`**，因此用 ctypes 直接调原始 COM 虚表。
- cmdlet 式 `Add-Type` 被安全策略拦，所以用 Python。

虚表索引（实测确认，0-2 为 IUnknown）：
  IUIAutomation:            ElementFromHandle = 6, CreateTrueCondition = 21
  IUIAutomationElement:     FindAll           = 6, GetCurrentPropertyValue = 10
  IUIAutomationElementArray: get_Length       = 3, GetElement = 4

用法：
    python -u read_uia_tree.py                      # 自动找豆包设置窗口
    python -u read_uia_tree.py --title 豆包          # 指定标题子串
    python -u read_uia_tree.py --class DoubaoIme     # 指定类名子串
"""

import argparse
import ctypes
from ctypes import POINTER, c_int, c_void_p, c_ushort, c_ulong, c_ulonglong
from ctypes import wintypes

ole32 = ctypes.windll.ole32
oleaut32 = ctypes.windll.oleaut32
u32 = ctypes.windll.user32

# 属性 ID（UIAutomationClient.h）
UIA_BoundingRectanglePropertyId = 30001
UIA_ControlTypePropertyId = 30003
UIA_NamePropertyId = 30005
UIA_IsEnabledPropertyId = 30010
UIA_IsOffscreenPropertyId = 30009

CONTROL_TYPES = {
    50000: "Button", 50001: "Calendar", 50002: "CheckBox", 50003: "ComboBox",
    50004: "Edit", 50005: "Hyperlink", 50006: "Image", 50007: "ListItem",
    50008: "List", 50009: "Menu", 50010: "MenuBar", 50011: "MenuItem",
    50012: "ProgressBar", 50013: "RadioButton", 50014: "ScrollBar",
    50015: "Slider", 50016: "Spinner", 50017: "StatusBar", 50018: "Tab",
    50019: "TabItem", 50020: "Text", 50021: "ToolBar", 50022: "ToolTip",
    50023: "Tree", 50024: "TreeItem", 50025: "Custom", 50026: "Group",
    50027: "Thumb", 50028: "DataGrid", 50029: "DataItem", 50030: "Document",
    50031: "SplitButton", 50032: "Window", 50033: "Pane", 50034: "Header",
    50035: "HeaderItem", 50036: "Table", 50037: "TitleBar",
    50038: "Separator", 50039: "SemanticZoom", 50040: "AppBar",
}

VT_BSTR, VT_I4, VT_BOOL = 8, 3, 11


class GUID(ctypes.Structure):
    _fields_ = [
        ("Data1", c_ulong),
        ("Data2", c_ushort),
        ("Data3", c_ushort),
        ("Data4", ctypes.c_ubyte * 8),
    ]


def make_guid(text):
    g = GUID()
    ole32.CLSIDFromString(text, ctypes.byref(g))
    return g


CLSID_CUIAUTOMATION = make_guid("{FF48DBA4-60EF-4201-AA87-54103EEF594E}")
IID_IUIAUTOMATION = make_guid("{30CBE57D-D9D0-452A-AB13-7AC5AC4825EE}")


def vcall(ptr, index, restype, *argtypes):
    """按虚表索引调用 COM 方法。"""
    vtable = ctypes.cast(ptr, POINTER(POINTER(c_void_p)))[0]
    proto = ctypes.WINFUNCTYPE(restype, c_void_p, *argtypes)
    return proto(vtable[index])


class VARIANT(ctypes.Structure):
    _fields_ = [
        ("vt", c_ushort), ("r1", c_ushort), ("r2", c_ushort), ("r3", c_ushort),
        ("val", c_ulonglong), ("pad", c_ulonglong),
    ]


def read_property(element, prop_id):
    v = VARIANT()
    if vcall(element, 10, ctypes.c_long, c_int, POINTER(VARIANT))(
            element, prop_id, ctypes.byref(v)) != 0:
        return None
    if v.vt == VT_BSTR and v.val:
        text = ctypes.cast(v.val, ctypes.c_wchar_p).value or ""
        oleaut32.SysFreeString(c_void_p(v.val))
        return text
    if v.vt == VT_I4:
        return ctypes.c_long(v.val & 0xFFFFFFFF).value
    if v.vt == VT_BOOL:
        return bool(v.val)
    return None


def find_windows(class_substr, title_substr):
    hits = []

    def callback(hwnd, _):
        cls = ctypes.create_unicode_buffer(512)
        u32.GetClassNameW(hwnd, cls, 512)
        title = ctypes.create_unicode_buffer(512)
        u32.GetWindowTextW(hwnd, title, 512)
        if not u32.IsWindowVisible(hwnd):
            return True
        if class_substr and class_substr not in cls.value:
            return True
        if title_substr and title_substr not in title.value:
            return True
        hits.append((hwnd, cls.value, title.value))
        return True

    proto = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    u32.EnumWindows(proto(callback), 0)
    return hits


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--class-substr", default="DoubaoIme",
                    help="类名子串（默认 DoubaoIme）")
    ap.add_argument("--title-substr", default="",
                    help="标题子串（默认不限）")
    args = ap.parse_args()

    ole32.CoInitializeEx(None, 2)  # APARTMENTTHREADED

    hits = find_windows(args.class_substr, args.title_substr)
    print(f"匹配到 {len(hits)} 个可见窗口：")
    for hwnd, cls, title in hits:
        print(f"  hwnd={hwnd} class={cls!r} title={title!r}")
    if not hits:
        print("\n⚠️ 目标窗口不可见（可能已最小化或关闭）。请先打开它再重跑。")
        return 1

    automation = c_void_p()
    hr = ole32.CoCreateInstance(
        ctypes.byref(CLSID_CUIAUTOMATION), None, 1,
        ctypes.byref(IID_IUIAUTOMATION), ctypes.byref(automation))
    if hr != 0:
        print(f"CoCreateInstance 失败 hr=0x{hr & 0xFFFFFFFF:08X}")
        return 1

    root = c_void_p()
    hr = vcall(automation, 6, ctypes.c_long, wintypes.HWND, POINTER(c_void_p))(
        automation, hits[0][0], ctypes.byref(root))
    if hr != 0 or not root:
        print(f"ElementFromHandle 失败 hr=0x{hr & 0xFFFFFFFF:08X}")
        return 1

    condition = c_void_p()
    vcall(automation, 21, ctypes.c_long, POINTER(c_void_p))(
        automation, ctypes.byref(condition))

    array = c_void_p()
    # TreeScope_Descendants = 4
    hr = vcall(root, 6, ctypes.c_long, c_int, c_void_p, POINTER(c_void_p))(
        root, 4, condition, ctypes.byref(array))
    if hr != 0 or not array:
        print(f"FindAll 失败 hr=0x{hr & 0xFFFFFFFF:08X}")
        return 1

    total = c_int()
    vcall(array, 3, ctypes.c_long, POINTER(c_int))(array, ctypes.byref(total))

    print(f"\n=== 控件树（共 {total.value} 个元素）===")
    shown = 0
    for i in range(total.value):
        element = c_void_p()
        if vcall(array, 4, ctypes.c_long, c_int, POINTER(c_void_p))(
                array, i, ctypes.byref(element)) != 0 or not element:
            continue
        name = read_property(element, UIA_NamePropertyId)
        ctype = read_property(element, UIA_ControlTypePropertyId)
        enabled = read_property(element, UIA_IsEnabledPropertyId)
        offscreen = read_property(element, UIA_IsOffscreenPropertyId)
        label = CONTROL_TYPES.get(ctype, f"CT{ctype}")
        if name and str(name).strip():
            shown += 1
            print(f"  #{i:3d} [{label:<12}] enabled={enabled} "
                  f"offscreen={offscreen} | {name!r}")
    print(f"\n有可见文本的控件 {shown} 个")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

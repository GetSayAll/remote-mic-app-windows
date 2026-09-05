# RC003 返回/音量± 三键在 Windows 上不可见（调查归档，2026-09-05）

## 现象

RC003 遥控器：确认/方向/电源/主页/菜单/TV 键全部正常（卡片高亮、锚点、
手势、注入均工作），唯独 **返回、音量+、音量−** 三键在任何已注册输入
通道中零事件——无高亮、无锚点、`返回→退格` 映射因此不生效。

## 证据链（全部真机实测）

1. **引擎日志（map_edges，4f5270b 新增）**：用户逐键测试，上/下/左/右/
   电源/主页/菜单/TV 边沿全部到达引擎；返回/音量+/音量− 三键零边沿。
   全程零 map_fire（这三键的手势从未触发，退格未生效的直接原因）。
2. **Raw Input 键盘捕获器**（Testing/capture-rawkeys.ps1，page 1/6 +
   page 0x0C/1 双注册）：三键零事件（连注入的合成键都能捕获，工具已
   自测校准：RAWINPUTHEADER 结构体 x64 为 24 字节，dwType/dwSize 是
   DWORD 不是指针——首版工具的教训）。
3. **全量 Raw Input 设备枚举**（Testing/list-all-rawinput.ps1）：RC003
   在 Windows 上只暴露 **一个 TYPE=1（键盘）设备**
   `HID#{00001812-...}_Dev_VID&012717_PID&32b8...`。没有任何 TYPE=2
   （HID）设备——即厂商/消费 collection 没有独立的可注册接口。
4. **工作按键的到达路径**：全部走键盘 VK（VK_UP/DOWN/LEFT/RIGHT、
   VK_RETURN(确认)、VK_HOME、VK_APPS、VK_SLEEP、VK_OEM_3）。macOS 侧
   的 usage 表（0x4A/0x65/0x66/0x35/0x80/0x81/0xF1）在 Windows RC003
   上对应不到任何到达事件。
5. **厂商 GATT 探测**（Testing/probe-rc003-vendor-gatt.ps1，WinRT）：
   设备共 9 个 GATT 服务；厂商服务 **8A7A0001-2c42-c2a2-0f36-41928c259b78**
   含三个可通知特征值（0102/0103/0112），订阅成功但按键期间零通知。
   ATVV（AB5E0001，语音）特征值被应用会话占用，第二会话枚举不到，
   未获得阳性对照；语音按键期间应用日志正常收到 ATVV 通知（session
   94），证明设备→系统的通知链路本身正常。
   注意：第二 GATT 会话的订阅对 GetCharacteristicsAsync 结果有缓存
   干扰（同特征值被前一探测会话占用后，新会话枚举为空；清理进程后
   恢复）。探测必须独占运行。

## 结论与假设

- 三键不进 Windows HID 输入栈（无键盘 VK、无消费页事件、无厂商
  collection 的 TYPE=2 设备）。macOS 能拿到 0x80/0x81/0xF1 是因为
  IOHIDManager 直接读全部 collection 的 usage。
- 最可能通道：小米厂商 GATT 服务（8A7A0001 或 01BF/FE59 之一）的
  通知，需满足特定前置条件（如先写某个特征值开启按键上报，或仅在
  特定配对/会话模式下上报）。ATVV 服务模式（transmit 写入开启会话）
  是已知先例。
- 2026-09-05 用户决定：暂不继续追查此三键，保持搁置。`返回→退格`
  映射在解决前无法生效（保留用户配置，不删）。

## 若重启调查，下一步建议

1. 用独占 GATT 会话写 8A7A0001/0102（write 特征值）试探是否开启
   按键上报（注意：厂商协议未知，写入前先 dump 特征值描述符/属性，
   保守起见优先读 0x180A 设备信息服务确认协议版本）。
2. 对照小米电视/盒子生态的开源逆向资料（ATVV 已有先例），查
   8A7A0001 服务的按键上报协议。
3. 备选：通过 HID.dll 的 HidD_GetPreparsedData 读 0x1812 服务的完整
   报告描述符，确认三键 usage 是否真的在 HID 描述符内（若在，走
   CreateFile+ReadFile 直读设备接口；枚举显示无 TYPE=2 接口，可能性
   较低，但值得一次性确认）。

## 相关工件

- 引擎日志：`%USERPROFILE%\sayall-diag.log`（SAYALL_GATT_LOG，map_*
  标记由 4f5270b 引入）
- 键盘捕获器：`Testing/capture-rawkeys.ps1`
- GATT 探测器：`Testing/probe-rc003-vendor-gatt.ps1`
- 设备枚举器：`Testing/list-all-rawinput.ps1`、`Testing/list-xiaomi-rawinput.ps1`
- 事件管线自测：`Testing/event-pipeline-test.ps1`

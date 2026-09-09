# WeType ConsentStore 历史记录修复测试手册

## 测试对象

- 修复 PR：[#55](https://github.com/GetSayAll/remote-mic-app-windows/pull/55)
- Source commit：`6b4d8b64d0711b4e1eda21b3fa81da2653c665bd`
- 本地测试包：`artifacts/windows-preview/SayAll-Windows-0.2.5-pr55-6b4d8b6-x64-setup.exe`
- SHA-256：`37dfadeb78bbe87a34d27712ec59c4e49b51ff75525412efc97b5954ec0cffc4`
- 适用环境：Windows 10 1809+ / Windows 11 x64、WeType、RC003；RC001 需单独记录

本包仅供本地验收。它没有 Windows Authenticode 签名，可能触发 SmartScreen；
本地构建使用的一次性 updater 签名不属于公开更新信任链，不得上传或公开分发。

## 测试目标

验证系统存在一条或多条 WeType 麦克风访问历史时，SayAll 能识别当前正在录音或
本次按键后新开始的录音，不会在用户持续按住语音键期间误触发输入法恢复、释放快捷键
或重复注入快捷键。

## 安装前准备

1. 记录 Windows 版本、WeType 版本、遥控器型号和当前 SayAll 版本。
2. 确认 WeType 的语音快捷键与 SayAll“按住说话快捷键”一致。
3. 保存现有设置和按键映射；升级后应保持不变。
4. 在系统托盘中选择 SayAll“退出”，确认应用正常退出。不要用任务管理器强杀正式安装版。
5. 在 PowerShell 中校验安装包：

   ```powershell
   Get-FileHash -Algorithm SHA256 -LiteralPath ".\artifacts\windows-preview\SayAll-Windows-0.2.5-pr55-6b4d8b6-x64-setup.exe"
   ```

6. 运行安装包完成当前用户覆盖安装，然后从开始菜单启动 SayAll。

## 用例一：升级与启动

1. 打开连接、音频端点、按住说话快捷键和按键映射页面。
2. 对照安装前记录，确认配置没有被重置。
3. 连接遥控器并等待 BLE / ATVV 就绪。

预期：只存在一个 SayAll 安装身份；程序正常启动，原设置、映射和音频端点选择保留。

## 用例二：持续按住不被错误恢复

1. 在可输入文本的窗口中激活 WeType。
2. 按住 RC003 语音键并连续说话 15 秒，然后释放。
3. 重复 5 次，每次间隔至少 10 秒。

预期：每次只启动一次听写；持续按住期间不停止、不重新开始、不重复注入快捷键；
只在物理释放后结束。SayAll 的音频会话和快捷键 DOWN/UP 必须严格成对。

## 用例三：闲置后首用

1. 保持 SayAll、WeType 和遥控器连接状态，至少 5 分钟不使用语音键。
2. 闲置后的第一次按住持续说话 15 秒，然后释放。
3. 再连续执行两次普通语音会话。

预期：闲置后第一次即成功，期间不触发错误恢复；后续会话不受上一会话影响。

## 用例四：快速连续会话

1. 连续完成 20 次约 1 秒的按下、说话、释放。
2. 再完成 3 次 10～15 秒持续按住。

预期：没有粘键、漏释放、跨会话重试或旧检查线程干扰新会话；每次听写只对应一个
物理按下/释放周期。

## 用例五：多历史记录边界

如果当前 Windows 自然存在多个 WeType 版本或安装路径留下的麦克风历史，直接执行
上述用例并记录结果。不得为了制造场景而编辑 ConsentStore 注册表、WeType 私有配置
或内部数据库。没有自然多记录环境时，本项记为 `deferred`，由自动化回归覆盖枚举顺序、
活动记录、已完成新记录、未变化记录以及观测缺失/回退五类判定。

## 日志判据

默认诊断日志位于 `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`。健康会话通常包含：

```text
wetype_check armed attempt=0 ...
wetype_check reacted=true attempt=0 ...
```

持续按住且 WeType 已开始录音时，不应出现同一会话的：

```text
wetype_revive result=...
chord_retry result=ok ...
```

若观测不可用，允许出现 `reason=observation_unavailable` 或
`reason=mic_active_or_unknown`，但不得因此释放并重注入当前和弦。

提交日志前应删除任何意外出现的个人路径或用户内容；不要提交语音、转写正文、设备
身份、蓝牙地址、HID 路径或音频端点身份。

## 结果记录模板

```text
Source commit: 6b4d8b64d0711b4e1eda21b3fa81da2653c665bd
Windows / WeType:
Remote: RC001 | RC003
升级与配置保留: passed | failed
持续按住 5 次: passed | failed
闲置 5 分钟后首用: passed | failed
快速会话 20 次: passed | failed
自然多历史记录环境: passed | failed | deferred
失败发生时间:
对应日志片段:
```

只有实际执行并观察满足预期才记录 `passed`；当前未提供真实硬件或自然多历史记录环境
的项目必须记录为 `deferred`。

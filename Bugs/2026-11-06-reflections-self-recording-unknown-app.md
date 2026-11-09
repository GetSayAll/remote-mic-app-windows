# 回眸把「前台是无线麦自己」的录音显示为未知应用

- 发现日期：2026-11-06
- 状态：已修复（自动化 passed；安装包复验 deferred；历史 7 条未改写）
- 影响范围：无线麦 SayAll Windows 1.8.3 x64；回眸「仅录音」；与遥控器型号无关
- 功能点：回眸 · 录音 manifest 应用身份
- 现象：首次设置向导第⑤步语音测试完成后，回眸「仅录音」新增一条「未知应用」录音
- 复现条件：`保存原始录音` 开启；在无线麦自己的窗口处于前台时完成一次语音会话
  （首次设置向导语音测试、设置页试音）；会话结束后查看回眸
- 正常预期：应用名显示「SayAll」，与个人中心统计的既有口径一致；不应落到「未知应用」
- 证据：
  - 录音 manifest 缺 `applicationKey` / `applicationName`；2026-11-06 23:09:55 的会话
    时长 1575 ms、50,444 字节
  - 同一时刻诊断日志：`frontend event=onboarding phase=voice_attempt ... step=voice_test`；
    `voice_attempt ... fg_self=true fg_pid=<本应用>`；`transcript_recording action=start ...`
  - 本机 212 条录音中 7 条缺应用身份；7 条全部是前台为本应用的会话
    （2026-11-05 六条、2026-11-06 一条），不是旧版 manifest 迁移数据
- 根因：`foreground_application_identity()` 在前台进程等于本进程时直接返回 `None`
  （`transcript_capture_windows.rs`），录音因此没有应用身份，回眸只能显示「未知应用」；
  个人中心统计走的 `foreground_application_name()` 已按 2026-10-25 的产品规则
  把同一情形显示为 `SayAll`，两条路径不一致
- 修复：抽出 `application_identity_for_pid(pid)`；`pid == 0` 仍返回 `None`，
  `pid == std::process::id()` 返回 `("SayAll", "SayAll")`；
  未知进程仍按原有可执行文件名/产品名映射
- 验证：
  - 新增回归 `self_foreground_recordings_are_labelled_sayall`：修复前失败
    （`None` ≠ `Some(("SayAll", "SayAll"))`），修复后 passed
  - 新增回归 `missing_foreground_pid_has_no_identity`：passed
  - `cargo fmt --all -- --check` passed
  - `cargo test --workspace` passed（sayall-windows 525 passed / 16 ignored、
    sayall-windows-app 207 passed / 1 ignored）
  - `cargo check -p sayall-windows-app --features runtime-simulation` passed
  - 安装版新录音复验：deferred（需安装本次测试包后重做一次向导语音测试）
- 边界：历史 7 条录音没有应用身份，不回写磁盘，仍显示「未知应用」；可在回眸中
  按「未知应用」整组删除（移到回收站）
- 隐私检查：未包含设备身份、语音内容、个人路径或凭据

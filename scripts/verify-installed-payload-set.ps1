param(
    [Parameter(Mandatory = $true)][string]$InstallDirectory,
    [string]$StagedDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri")
)

# 安装目录载荷集合断言（2026-10-07，Issue #206）
#
# 解决什么：tauri 的 bundle.resources 最终打进 NSIS（LZMA 压缩），**光看 setup 的字节无法确认
# arm64 助手与 Gadget 到底有没有进包**；verify-windows-bundle.ps1 校验的是"暂存集合 + 覆盖配置
# 声明"，也不是安装结果。唯一直接判据是装完之后比对"出包暂存了什么 vs 安装目录里有什么"。
#
# 除哈希外还查两份助手各自的 PE machine（x64 = 0x8664，arm64 = 0xAA64）：这能挡住"把 x64 助手
# 复制成 arm64 名字"这类静态检查看不出来的静默错配——正是 Issue #206 的失败模式（架构不匹配）。
#
# 未暂存的载荷**不做**"必须不存在"的反向断言：升级安装不会删除上一版留下的文件，反向断言会误报。
#
# 用法（CI 的安装矩阵调用它；本地出包后也可手动跑）：
#   powershell -File scripts/verify-installed-payload-set.ps1 -InstallDirectory "<安装目录>"
# 失败时抛出异常（独立运行即退出码非零）。
$payloadNames = @(
    "sayall-helper.exe",
    "frida-gadget.dll",
    "sayall-helper-arm64.exe",
    "frida-gadget-arm64.dll"
)
# 只有助手有架构要求；Gadget 的架构由它自己的锁定文件摘要锁定。
$expectedMachines = @{
    "sayall-helper.exe"       = 0x8664
    "sayall-helper-arm64.exe" = 0xAA64
}

function Get-PeMachine([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $buffer = New-Object byte[] 0x400
        $read = $stream.Read($buffer, 0, $buffer.Length)
    } finally {
        $stream.Dispose()
    }
    if ($read -lt 0x40 -or $buffer[0] -ne 0x4D -or $buffer[1] -ne 0x5A) { return $null }
    $offset = [BitConverter]::ToInt32($buffer, 0x3C)
    if ($offset + 6 -gt $read) { return $null }
    if ($buffer[$offset] -ne 0x50 -or $buffer[$offset + 1] -ne 0x45) { return $null }
    return [BitConverter]::ToUInt16($buffer, $offset + 4)
}

$checked = 0
$failures = New-Object System.Collections.Generic.List[string]
foreach ($name in $payloadNames) {
    $staged = Join-Path $StagedDirectory $name
    if (-not (Test-Path -LiteralPath $staged -PathType Leaf)) {
        Write-Host "skip：$name 本次未暂存（本包不含它）"
        continue
    }
    $installed = Join-Path $InstallDirectory $name
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) {
        $failures.Add("缺少 $name：出包暂存了它，安装目录里却没有")
        continue
    }
    $stagedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $staged).Hash
    $installedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installed).Hash
    if ($stagedHash -ne $installedHash) {
        $failures.Add("$name 哈希不符：暂存 $stagedHash，安装 $installedHash")
        continue
    }
    if ($expectedMachines.ContainsKey($name)) {
        $machine = Get-PeMachine $installed
        $expected = $expectedMachines[$name]
        if ($null -eq $machine) {
            $failures.Add(("{0} 无法解析 PE 头（期望 machine=0x{1:X4}）" -f $name, $expected))
            continue
        }
        if ($machine -ne $expected) {
            $failures.Add(("{0} 架构不符：期望 machine=0x{1:X4}，实际 0x{2:X4}" -f $name, $expected, $machine))
            continue
        }
        Write-Host ("OK：{0}（SHA-256 一致，machine=0x{1:X4}）" -f $name, $machine)
    } else {
        Write-Host "OK：$name（SHA-256 一致）"
    }
    $checked++
}

if ($checked -eq 0) {
    Write-Host "warn：没有任何暂存载荷可比对（src-tauri 未暂存、或安装目录为空）"
}
if ($failures.Count -gt 0) {
    foreach ($item in $failures) { Write-Host "FAIL：$item" }
    throw "安装目录载荷集合不符（$($failures.Count) 项）——出包暂存了什么，安装目录里就必须有什么"
}
Write-Host "载荷集合断言通过（已核对 $checked 件）"

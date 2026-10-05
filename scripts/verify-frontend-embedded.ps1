# 出包防回归：校验主程序二进制确实“嵌入”了前端产物（防止界面变成目录列表）。
#
# 背景（2026-10-03 本机实证）：tauri 的 FrontendDist 是 untagged 枚举，反序列化时
# 先尝试按 URL 解析、再按目录解析。Windows 绝对路径（如 D:/x/dist-build）会被
# url crate 解析成“单字母 scheme 的 URL”（d:/x/dist-build），于是前端资源完全不
# 嵌入二进制；运行期窗口直接导航到该路径，界面变成一个目录列表——而哈希、签名、
# 内嵌修订号全部正常，不做“资源是否嵌入”这一项检查就发现不了。
#
# 判据：dist 目录下每个文件都以“相对路径（正斜杠）”形式出现在 exe 字节里
# （tauri 把资源键写成明文字符串），这是“资源真的进了二进制”的直接证据。
param(
    [string]$Executable = (Join-Path (Split-Path -Parent $PSScriptRoot) "target/release/sayall-windows-app.exe"),
    [string]$FrontendDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) "dist")
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "未找到主程序: $Executable"
}
if (-not (Test-Path -LiteralPath $FrontendDirectory -PathType Container)) {
    throw "未找到前端产物目录: $FrontendDirectory"
}

$root = (Resolve-Path -LiteralPath $FrontendDirectory).Path.TrimEnd('\', '/')
$files = @(Get-ChildItem -LiteralPath $root -File -Recurse)
if ($files.Count -eq 0) {
    throw "前端产物目录为空: $root"
}
$hasIndex = @($files | Where-Object { $_.Name -eq "index.html" }).Count -gt 0
if (-not $hasIndex) {
    throw "前端产物目录缺少 index.html: $root"
}

$binary = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($Executable))
$missing = @()
foreach ($file in $files) {
    $relative = $file.FullName.Substring($root.Length + 1).Replace('\', '/')
    if ($binary.IndexOf($relative, [StringComparison]::Ordinal) -lt 0) {
        $missing += $relative
    }
}

if ($missing.Count -gt 0) {
    Write-Host ("缺失 {0}/{1} 个前端资源键，例如：" -f $missing.Count, $files.Count)
    $missing | Select-Object -First 10 | ForEach-Object { Write-Host ("  - " + $_) }
    $hint = "前端资源未嵌入 {0}。若补丁过 build.frontendDist，必须使用相对路径（如 ../dist-build-x）；" -f $Executable
    $hint += "Windows 绝对路径会被解析为单字母 scheme 的 URL，嵌入会被静默跳过（界面将变成目录列表）。"
    throw $hint
}

Write-Host ("已校验：{0} 的 {1} 个前端资源全部嵌入" -f $Executable, $files.Count)

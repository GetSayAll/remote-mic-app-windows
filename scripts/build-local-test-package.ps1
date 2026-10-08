# 本地测试包：构建（助手 + 前端 + NSIS + updater 签名）并跑嵌入校验。
#
# 背景与坑（2026-10-03 全量实证；每一条都曾让出包静默出错）：
# 1) `--config '{...}'` 内联 JSON 在 PowerShell → pnpm shim 之间会被剥掉引号，
#    tauri 报 "key must be a string" ⇒ 统一走 src-tauri/tauri.local-build.conf.json 文件形态；
# 2) frontendDist 只能写相对 src-tauri 的路径；Windows 绝对路径会被 url crate 解析成
#    单字母 scheme（d:）→ 前端不嵌入、界面变目录列表，而哈希与签名全部正常；
# 3) staging 脚本按 hardware/RC003/helper/target/release 取助手 ⇒ 跑 staging 时不要设
#    CARGO_TARGET_DIR；应用构建可用 -CargoTargetDir 指向已有缓存的 target 目录；
# 4) TAURI_CONFIG 不进 cargo 指纹 ⇒ 构建前 touch src-tauri/src/lib.rs，避免复用旧内嵌；
# 5) 嵌入校验对象必须是**裸构建 exe**（NSIS setup 是压缩包装，明文路径查不到、否则必误报）；
# 6) 双载荷（2026-10-07）：arm64 两件**只有 staging 真落地**时才用 `--config
#    src-tauri/tauri.arm64-payload.conf.json` 覆盖式声明；`--config` 可重复，按
#    RFC 7396 顺序合并（数组整体覆盖），因此两份配置叠加是安全的。默认
#    src-tauri/tauri.conf.json 必须保持 x64 单载荷，理由见 RELEASING.md。
#
# 签名：默认读取本机测试密钥 %LOCALAPPDATA%\SayAll\local-test-updater.key；口令取
# -SigningKeyPassword → 环境变量 TAURI_SIGNING_PRIVATE_KEY_PASSWORD → 本机测试约定
# "sayall-local-test"。该密钥与生成的 .sig 只用于本机测试，**绝不可发布**；密钥不存在则跳过签名。
#
# 用法：
#   powershell -File scripts\build-local-test-package.ps1
#   powershell -File scripts\build-local-test-package.ps1 -CargoTargetDir D:\...\target
param(
    [string]$CargoTargetDir = "",
    [string]$SigningKeyPath = "",
    [string]$SigningKeyPassword = ""
)

$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch {}

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# ---- pnpm 解析（与 ci-preflight 同款）：PATH 上的 pnpm 优先，其次 corepack pnpm ----
$script:UseCorepack = $false
function Test-PnpmAvailable {
    if (Get-Command pnpm -ErrorAction SilentlyContinue) { return $true }
    if (Get-Command corepack -ErrorAction SilentlyContinue) {
        $script:UseCorepack = $true
        return $true
    }
    return $false
}
function Invoke-Pnpm {
    param([string[]]$PnpmArgs)
    if ($script:UseCorepack) { & corepack pnpm @PnpmArgs } else { & pnpm @PnpmArgs }
}
if (-not (Test-PnpmAvailable)) { throw "缺少 pnpm：请安装 pnpm 或启用 corepack（corepack enable）" }

# ---- MSVC 环境：link.exe 不在 PATH 时用 vswhere 找 VS 并导入 vcvars64 ----
function Import-MsvcEnvironment {
    if (Get-Command link.exe -ErrorAction SilentlyContinue) { return }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vsPath) {
            $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path $vcvars) {
                $dump = cmd /c "`"$vcvars`" >nul 2>&1 && set"
                foreach ($line in $dump) {
                    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
                }
                return
            }
        }
    }
    throw "找不到 link.exe，且无法通过 vswhere 导入 VS 构建环境：请改用 VS Developer PowerShell 运行本脚本"
}
Import-MsvcEnvironment
Write-Host ("[env] link.exe = " + (Get-Command link.exe).Source)

# ---- 1/6 依赖（锁文件为准） ----
Write-Host "[1/6] pnpm install --frozen-lockfile"
Invoke-Pnpm @("install", "--frozen-lockfile")
if ($LASTEXITCODE -ne 0) { throw "pnpm install 失败" }

# ---- 2/6 frida-gadget：按锁定文件校验（缺失则下载），绝不落未校验的 DLL ----
Write-Host "[2/6] 校验 frida-gadget（vendor 锁定文件）"
& python (Join-Path $repo "hardware\RC003\helper\vendor\fetch_frida_gadget.py")
if ($LASTEXITCODE -ne 0) { throw "frida-gadget 校验失败" }

# ---- 3/6 staging：构建助手并落地 src-tauri（注意：本步不要设 CARGO_TARGET_DIR） ----
Write-Host "[3/6] stage-bundle-inputs（助手 release + gadget 落地；arm64 载荷视本机工具链而定）"
Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
& node (Join-Path $repo "scripts\stage-bundle-inputs.cjs")
if ($LASTEXITCODE -ne 0) { throw "staging 失败" }

# ---- 4/6 前端到独立目录（避免 staging 内 vite 清 dist/ 撞 safe-delete 守卫） ----
Write-Host "[4/6] vite build -> dist-build"
& node (Join-Path $repo "node_modules\vite\bin\vite.js") build --outDir dist-build
if ($LASTEXITCODE -ne 0) { throw "前端构建失败" }

# ---- 5/6 tauri build（NSIS；配置覆盖走文件，见头部坑 1/2/4） ----
Write-Host "[5/6] tauri build --bundles nsis"
(Get-Item (Join-Path $repo "src-tauri\src\lib.rs")).LastWriteTime = Get-Date
if ($CargoTargetDir) { $env:CARGO_TARGET_DIR = $CargoTargetDir }
if (-not $SigningKeyPath) { $SigningKeyPath = Join-Path $env:LOCALAPPDATA "SayAll\local-test-updater.key" }
if (Test-Path $SigningKeyPath) {
    if (-not $SigningKeyPassword) {
        $SigningKeyPassword = if ($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD } else { "sayall-local-test" }
    }
    $env:TAURI_SIGNING_PRIVATE_KEY = ((Get-Content $SigningKeyPath -Raw) -replace "`r", "").TrimEnd("`n")
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $SigningKeyPassword
    Write-Host "[5/6] 使用本机测试密钥签名（与 CI pubkey 不匹配的警告是预期；产物与 .sig 绝不可发布）"
} else {
    Write-Host "[5/6] 未找到本机测试密钥，跳过 updater 签名"
}
# arm64 双载荷：只有 staging 真的把两件都落到 src-tauri 之后，才用 --config 覆盖式
# 声明它们（默认 src-tauri/tauri.conf.json 必须保持 x64 单载荷：tauri-build 在编译期
# 校验资源存在，而 arm64 助手在本机缺 VS ARM64 工具集时根本构建不出来——写进默认
# 配置会让全仓库的 cargo check / cargo test 直接失败）。理由与接口见 RELEASING.md。
$arm64PayloadNames = @("sayall-helper-arm64.exe", "frida-gadget-arm64.dll")
$arm64Missing = @($arm64PayloadNames | Where-Object { -not (Test-Path (Join-Path $repo ("src-tauri\" + $_))) })
$tauriArgs = @("tauri", "build", "--bundles", "nsis", "--ci", "--config", "src-tauri/tauri.local-build.conf.json")
if ($arm64Missing.Count -eq 0) {
    $tauriArgs += @("--config", "src-tauri/tauri.arm64-payload.conf.json")
    Write-Host "[5/6] 本包含 arm64 载荷（追加 --config src-tauri/tauri.arm64-payload.conf.json）"
} else {
    Write-Host ("[5/6] 警告：本包不含 arm64 载荷（缺 " + ($arm64Missing -join ", ") + "）")
    Write-Host "[5/6] 警告：ARM64 机器上安装后应用会明确提示不支持「全按键支持」；语音路径不受影响"
}
Invoke-Pnpm $tauriArgs
if ($LASTEXITCODE -ne 0) { throw "tauri build 失败" }

# ---- 6/6 嵌入校验（裸 exe！）与产物报告 ----
Write-Host "[6/6] 嵌入校验 + 产物报告"
$targetRoot = if ($CargoTargetDir) { $CargoTargetDir } else { Join-Path $repo "target" }
$rawExe = Join-Path $targetRoot "release\sayall-windows-app.exe"
if (-not (Test-Path $rawExe)) { throw "找不到裸构建产物：$rawExe" }
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $repo "scripts\verify-frontend-embedded.ps1") -Executable $rawExe -FrontendDirectory (Join-Path $repo "dist-build")
if ($LASTEXITCODE -ne 0) { throw "前端嵌入校验失败" }
$setup = Get-ChildItem (Join-Path $targetRoot "release\bundle\nsis") -Filter "*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $setup) { throw "找不到 NSIS 产物" }
$sha = (Get-FileHash $setup.FullName -Algorithm SHA256).Hash.ToLower()
$sig = Join-Path $setup.DirectoryName ($setup.Name + ".sig")
Write-Host ""
Write-Host ("SETUP       : " + $setup.FullName)
Write-Host ("SETUP_SHA256: " + $sha)
Write-Host ("SIGNATURE   : " + $(if (Test-Path $sig) { $sig } else { "(无)" }))
Write-Host ""
Write-Host "OK：本地测试包已生成（安装与静态校验：scripts\verify-local-test-install.ps1）"

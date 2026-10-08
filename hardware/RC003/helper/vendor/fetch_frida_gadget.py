"""获取并校验 RC003 助手所需的注入载体（Frida Gadget）。

版本与哈希的**唯一事实来源**是同目录的 ``frida-gadget.lock.json``。本脚本不信任
网络返回的任何内容：它先按锁定值校验下载产物的 SHA-256，再解压，再校验解压产物的
SHA-256 与 **PE machine**（架构）；任何一步不符都以非零退出码终止，绝不留下未校验
的 DLL 让助手去加载。

为什么按架构各一份：承载 RC003 的宿主 ``WUDFHost.exe`` 在 ARM64 系统上是原生 ARM64
进程，而 ARM64 进程不能加载 x64 镜像（Issue #206 现场：x64 助手对 ARM64 宿主，注入必然
失败）。锁定文件里每个架构一条，助手只认自己那一份。

用法
----
    python fetch_frida_gadget.py                # x86_64（默认，与历史行为一致）
    python fetch_frida_gadget.py --arch arm64   # 只处理 arm64
    python fetch_frida_gadget.py --all          # 两套都处理（出包/CI 用）
    python fetch_frida_gadget.py --verify-only  # 只校验本地已存在的文件，不联网

只依赖标准库（urllib / lzma / hashlib / json）。退出码：
    0 成功 / 2 锁定文件缺失或格式不符 / 3 校验不符 / 4 下载失败 / 5 解压失败
"""

from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import shutil
import sys
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOCK_PATH = HERE / "frida-gadget.lock.json"

# Windows 上 Python 的 stdout/stderr 默认跟随控制台代码页，CI runner 实测是 **cp1252**；
# 本脚本会打印中文进度（如「下载 …」），于是整个取件过程被 UnicodeEncodeError 打断
# （2026-09-27 PR #127 CI 实测）。显式把输出流设成 UTF-8，别让编码问题伪装成"下载失败"。
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

EXIT_OK = 0
EXIT_LOCK = 2
EXIT_MISMATCH = 3
EXIT_DOWNLOAD = 4
EXIT_EXTRACT = 5

# 锁定文件里的 arch 取值 → PE machine（校验解压产物用）。扩展名写死是为了让"期望值"
# 与被校验的东西一一对应；新增架构时必须同时改这里与锁定文件。
ARCH_MACHINES = {"x86_64": 0x8664, "arm64": 0xAA64}
# 允许的别名（命令行与锁定文件都容错），统一收敛到 canonical 名。
ARCH_ALIASES = {
    "x86_64": "x86_64",
    "x64": "x86_64",
    "amd64": "x86_64",
    "arm64": "arm64",
    "aarch64": "arm64",
}
DEFAULT_ARCH = "x86_64"


def say(message: str) -> None:
    print(message, flush=True)


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalize_arch(value: str) -> str | None:
    return ARCH_ALIASES.get(str(value).strip().lower())


def load_entries() -> list[dict]:
    if not LOCK_PATH.exists():
        say(f"[fail] 锁定文件缺失: {LOCK_PATH}")
        raise SystemExit(EXIT_LOCK)
    try:
        payload = json.loads(LOCK_PATH.read_text(encoding="utf-8"))
        entries = payload["entries"]
    except (KeyError, ValueError) as exc:
        say(f"[fail] 锁定文件格式不符: {exc}")
        raise SystemExit(EXIT_LOCK)
    if not isinstance(entries, list) or not entries:
        say("[fail] 锁定文件没有任何 entries")
        raise SystemExit(EXIT_LOCK)
    return entries


def select_entries(entries: list[dict], wanted: list[str]) -> list[dict]:
    by_arch: dict[str, dict] = {}
    for entry in entries:
        arch = normalize_arch(entry.get("arch", ""))
        if arch is None:
            say(f"[fail] 锁定文件里有无法识别的 arch: {entry.get('arch')!r}")
            raise SystemExit(EXIT_LOCK)
        by_arch.setdefault(arch, entry)
    missing = [arch for arch in wanted if arch not in by_arch]
    if missing:
        say(f"[fail] 锁定文件缺少架构条目: {', '.join(missing)}（现有: {', '.join(by_arch)}）")
        raise SystemExit(EXIT_LOCK)
    # 保持 wanted 的顺序，输出可预测
    return [by_arch[arch] for arch in wanted]


def verify(path: Path, expected: str, label: str) -> bool:
    if not path.exists():
        say(f"[fail] {label} 不存在: {path.name}")
        return False
    actual = sha256_of(path)
    if actual != expected:
        say(f"[fail] {label} SHA-256 不符")
        say(f"       期望 {expected}")
        say(f"       实际 {actual}")
        return False
    say(f"[ ok ] {label} SHA-256 匹配 ({actual[:16]}…)")
    return True


def download(url: str, target: Path) -> bool:
    say(f"[ .. ] 下载 {url}")
    tmp = target.with_suffix(target.suffix + ".part")
    try:
        with urllib.request.urlopen(url, timeout=60) as response, tmp.open("wb") as out:
            shutil.copyfileobj(response, out)
    except (urllib.error.URLError, OSError) as exc:
        say(f"[fail] 下载失败: {exc}")
        tmp.unlink(missing_ok=True)
        return False
    tmp.replace(target)
    say(f"[ ok ] 下载完成 {target.stat().st_size} 字节")
    return True


def extract(source: Path, target: Path) -> bool:
    say(f"[ .. ] 解压 {source.name} -> {target.name}")
    tmp = target.with_suffix(".part")
    try:
        with lzma.open(source, "rb") as handle, tmp.open("wb") as out:
            shutil.copyfileobj(handle, out)
    except (lzma.LZMAError, OSError) as exc:
        say(f"[fail] 解压失败: {exc}")
        tmp.unlink(missing_ok=True)
        return False
    tmp.replace(target)
    say(f"[ ok ] 解压完成 {target.stat().st_size} 字节")
    return True


def pe_machine_of(path: Path) -> int | None:
    """读 PE 头里的 machine（只读前 0x400 字节）。解析不了返回 None。"""
    with path.open("rb") as handle:
        header = handle.read(0x400)
    if len(header) < 0x40 or header[:2] != b"MZ":
        return None
    pe_offset = int.from_bytes(header[0x3C:0x40], "little")
    if pe_offset + 6 > len(header) or header[pe_offset : pe_offset + 4] != b"PE\0\0":
        return None
    return int.from_bytes(header[pe_offset + 4 : pe_offset + 6], "little")


def handle_entry(entry: dict, verify_only: bool) -> int:
    arch = normalize_arch(entry["arch"])
    assert arch is not None  # select_entries 已经校验过
    expected_machine = ARCH_MACHINES[arch]
    version = entry["version"]
    say("")
    say(f"=== Frida Gadget {version} ({arch}) ===")

    source = HERE / entry["compressed"]["name"]
    target = HERE / entry["target"]

    # 快路径：产物已存在且摘要+架构都对 → 一个字节都不写。出包会重复调用本脚本，
    # 每次都重解压 20 MB 既慢又会让"是否真的换过 DLL"更难判读。
    if target.exists():
        if verify(target, entry["extracted"]["sha256"], "解压产物") and (
            pe_machine_of(target) == expected_machine
        ):
            say(f"[ .. ] 已存在且校验通过，跳过下载与解压: {target.name}")
            return EXIT_OK

    if not verify_only:
        if not source.exists():
            if not download(entry["source"]["url"], source):
                return EXIT_DOWNLOAD
        else:
            say(f"[ .. ] 已存在 {source.name}，跳过下载")

    if not verify(source, entry["compressed"]["sha256"], "压缩包"):
        return EXIT_MISMATCH

    if not verify_only:
        if not extract(source, target):
            return EXIT_EXTRACT

    if not verify(target, entry["extracted"]["sha256"], "解压产物"):
        return EXIT_MISMATCH

    machine = pe_machine_of(target)
    if machine is None:
        say("[fail] 解压产物不是可解析的 PE 文件（缺少 MZ/PE 头）")
        return EXIT_MISMATCH
    if machine != expected_machine:
        say(
            f"[fail] 架构不符: machine=0x{machine:04X}，"
            f"期望 0x{expected_machine:04X} ({arch})"
        )
        return EXIT_MISMATCH
    say(f"[ ok ] PE 头有效，machine=0x{machine:04X} ({arch})")
    return EXIT_OK


def main() -> int:
    parser = argparse.ArgumentParser(description="获取并校验 Frida Gadget（按架构）")
    parser.add_argument(
        "--arch",
        default=DEFAULT_ARCH,
        help=f"要处理的架构（默认 {DEFAULT_ARCH}，与历史行为一致）",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="处理锁定文件里的全部架构（出包/CI 用）",
    )
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="只校验本地已存在的文件，不下载、不联网",
    )
    args = parser.parse_args()

    if args.all:
        wanted = ["x86_64", "arm64"]
    else:
        arch = normalize_arch(args.arch)
        if arch is None:
            say(f"[fail] 未知架构: {args.arch}（可选: {', '.join(sorted(set(ARCH_ALIASES)))})")
            return EXIT_LOCK
        wanted = [arch]

    entries = select_entries(load_entries(), wanted)
    for entry in entries:
        code = handle_entry(entry, args.verify_only)
        if code != EXIT_OK:
            return code

    say("")
    for entry in entries:
        say(f"就绪: {HERE / entry['target']}")
        say(f"版本由锁定文件固定: {entry['version']} / {entry['source']['tag']} ({entry['arch']})")
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())

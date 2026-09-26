"""获取并校验 RC003 助手所需的注入载体（Frida Gadget）。

版本与哈希的**唯一事实来源**是同目录的 ``frida-gadget.lock.json``。本脚本不信任
网络返回的任何内容：它先按锁定值校验下载产物的 SHA-256，再解压，再校验解压产物的
SHA-256；任何一步不符都以非零退出码终止，绝不留下未校验的 DLL 让助手去加载。

用法
----
    python fetch_frida_gadget.py                # 下载（若缺）+ 校验 + 解压
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


def say(message: str) -> None:
    print(message, flush=True)


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_lock() -> dict:
    if not LOCK_PATH.exists():
        say(f"[fail] 锁定文件缺失: {LOCK_PATH}")
        raise SystemExit(EXIT_LOCK)
    try:
        payload = json.loads(LOCK_PATH.read_text(encoding="utf-8"))
        return payload["entries"][0]
    except (KeyError, ValueError) as exc:
        say(f"[fail] 锁定文件格式不符: {exc}")
        raise SystemExit(EXIT_LOCK)


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


def main() -> int:
    parser = argparse.ArgumentParser(description="获取并校验 Frida Gadget")
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="只校验本地已存在的文件，不下载、不联网",
    )
    args = parser.parse_args()

    entry = load_lock()
    version = entry["version"]
    say(f"=== Frida Gadget {version} ({entry['arch']}) ===")

    source = HERE / entry["compressed"]["name"]
    target = HERE / entry["target"]

    if not args.verify_only:
        if not source.exists():
            if not download(entry["source"]["url"], source):
                return EXIT_DOWNLOAD
        else:
            say(f"[ .. ] 已存在 {source.name}，跳过下载")

    if not verify(source, entry["compressed"]["sha256"], "压缩包"):
        return EXIT_MISMATCH

    if not args.verify_only:
        if not extract(source, target):
            return EXIT_EXTRACT

    if not verify(target, entry["extracted"]["sha256"], "解压产物"):
        return EXIT_MISMATCH

    with target.open("rb") as handle:
        header = handle.read(0x200)
    if header[:2] != b"MZ":
        say("[fail] 解压产物不是 PE 文件（缺少 MZ 头）")
        return EXIT_MISMATCH
    pe_offset = int.from_bytes(header[0x3C:0x40], "little")
    machine = int.from_bytes(header[pe_offset + 4 : pe_offset + 6], "little")
    if machine != 0x8664:
        say(f"[fail] 架构不符: machine=0x{machine:04X}，期望 0x8664 (x86_64)")
        return EXIT_MISMATCH
    say("[ ok ] PE 头有效，machine=0x8664 (x86_64)")

    say("")
    say(f"就绪: {target}")
    say(f"版本由锁定文件固定: {version} / {entry['source']['tag']}")
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())

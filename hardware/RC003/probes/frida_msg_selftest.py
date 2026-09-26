"""Frida 消息通道自检（无需提权）：验证 send() 与 post()/recv() 是否都可用。

背景：2026-09-23 的只读 tap 实验（`wudf_ioctl_tap.py`）中，JS 的 `send()` 明显工作
（心跳与逐条记录都到了 Python 侧），但 Python 的 `script.post()` 似乎没有被 JS 的
`recv()` 收到——表现为阶段标签恒为 `pre`、收尾汇总为空。

写事件类探针的硬性要求（见项目备忘）：**先自检再采真机数据**。否则"没收到"无法
区分"对端没发"与"本端通道坏了"。本脚本就是这条要求的执行体。

做法：spawn 一个无窗口的短命进程，注入一个只做消息往返的脚本，比较
`send` 与 `post/recv` 两条通道。
"""

from __future__ import annotations

import subprocess
import sys
import time

try:
    import frida
except ImportError:
    print("frida 未安装：pip install frida")
    sys.exit(2)

PYTHON = r"C:\Users\hd838\.workbuddy\binaries\python\envs\default\Scripts\python.exe"

SCRIPT = r"""
var got = [];
recv(function (m) {
  got.push(m && m.type ? m.type : '?');
  send({ type: 'recv_echo', got: got.slice(), value: m && m.value });
});

send({ type: 'send_reachable', marker: 1 });

rpc.exports = { roundtrip: function () { return 'rpc_ok'; } };
"""


def probe(session, label: str) -> tuple[bool, bool]:  # noqa: ANN001
    """在给定 session 上测 send 与 post/recv 两条通道。"""
    seen: list[dict] = []
    script = session.create_script(SCRIPT)
    script.on("message", lambda m, _d: seen.append(m))
    script.load()
    time.sleep(1.0)

    send_ok = any(m.get("payload", {}).get("type") == "send_reachable" for m in seen)
    print(f"  [1] send (JS->PY): {'可用' if send_ok else '不可用'}  ({len(seen)} 条)")

    seen.clear()
    script.post({"type": "hello", "value": 42})
    time.sleep(1.0)
    echoes = [m for m in seen if m.get("payload", {}).get("type") == "recv_echo"]
    recv_ok = len(echoes) > 0 and echoes[0]["payload"].get("value") == 42
    print(f"  [2] post/recv (PY->JS): {'可用' if recv_ok else '不可用'}  ({len(echoes)} 条回显)")

    print("  [3] rpc.exports 代理: ", end="")
    try:
        print(f"roundtrip() -> {script.exports_sync.roundtrip()}")
    except Exception as exc:  # noqa: BLE001
        print(f"失败 {type(exc).__name__}: {exc}")

    try:
        script.unload()
    except Exception:  # noqa: BLE001
        pass
    print(f"  → {label}: send={'ok' if send_ok else 'broken'} recv={'ok' if recv_ok else 'broken'}")
    return send_ok, recv_ok


def main() -> int:
    print("=== Frida 消息通道自检 ===")
    print(f"frida {frida.__version__}")
    device = frida.get_local_device()

    print("\n--- 模式 1：spawn + post/recv ---")
    pid = device.spawn([PYTHON, "-c", "import time; time.sleep(15)"])
    session = device.attach(pid)
    device.resume(pid)
    spawn_res = probe(session, "spawn")
    try:
        session.detach()
        device.kill(pid)
    except Exception:  # noqa: BLE001
        pass

    print("\n--- 模式 2：attach（目标已在运行）+ post/recv ---")
    sleeper = subprocess.Popen(
        [PYTHON, "-c", "import time; time.sleep(25)"],
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    time.sleep(1.5)
    try:
        session2 = frida.attach(sleeper.pid)
        attach_res = probe(session2, "attach")
        session2.detach()
    except Exception as exc:  # noqa: BLE001
        print(f"  attach 失败: {type(exc).__name__}: {exc}")
        attach_res = (False, False)
    finally:
        sleeper.kill()

    print("\n=== 判定 ===")
    print(f"  spawn : send={spawn_res[0]} recv={spawn_res[1]}")
    print(f"  attach: send={attach_res[0]} recv={attach_res[1]}")
    if not (spawn_res[1] and attach_res[1]):
        print("  → post/recv 不可靠：阶段切换与汇总不能依赖它。改用")
        print("     「每条记录自带时间戳 + Python 侧按时间轴归属阶段」")
        print("     以及「周期性 send 携带累计统计」，只保留 send 一条通道。")
    return 0


if __name__ == "__main__":
    sys.exit(main())

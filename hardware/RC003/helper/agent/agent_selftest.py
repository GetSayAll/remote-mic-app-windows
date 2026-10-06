"""rc003_agent.js 的自检台（不需要提权、不需要 RC003 在线、不注入任何系统进程）。

做法：本脚本扮演"助手"（真实 loopback TCP 服务端），用 frida 的**开发期**绑定把 agent
加载进一个**哑进程**，然后逐条断言协议与租约语义。它验证的是 agent 的**生命周期与传输**，
不验证拦键效果（那需要真机 + 提权，属另一轮）。

为什么需要它
------------
Frida 17 的 Socket 语义与直觉相反（字符串写入被静默丢弃、connect 到关闭端口永不 settle、
挂起的 read 不因 EOF 结束）。这些坑不会抛错，只会让 agent **安静地不工作**。
没有这一层自检，"agent 没反应"就无法区分"宿主没有目标 IOCTL"与"传输写错了"。

用例
----
  A 无助手在监听      → init 必须在 CONNECT_TIMEOUT_MS 内 settle（绝不能挂住宿主）
  B 正常协议          → hello / hb / renew 后 lease_ok=true / 停续约后 lease_ok=false
  C 显式解除          → 收到 disarm 后 hb.disarmed=true
  D 重连只连一次      → 助手消失再上线后只接受 1 条连接（并发守卫生效）
  E targets 护栏      → 只许追加哨兵键，被拒的命令不得改动清空范围
  F 一次回调连写多行  → targets/synth/mode 的**每一行**上行都必须到达
                        （Frida 17 `output.write()` 单飞互斥回归项，2026-10-02）

退出码：0 全部通过 / 1 有用例失败 / 2 环境不可用（缺 frida）
"""

from __future__ import annotations

import json
import socket
import sys
import threading
import time
from pathlib import Path

try:
    import frida
except ImportError:  # pragma: no cover
    print("frida 未安装：pip install frida")
    raise SystemExit(2)

HERE = Path(__file__).resolve().parent
# 允许指定别的 agent 文件：用来做"把修复去掉，用例必须 FAIL"的阳性对照。
# 只断言"当前文件通过"而不验证"缺陷版本会失败"，等于没验证判据本身有没有分辨力。
AGENT = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else HERE / "rc003_agent.js"
PYTHON = sys.executable
LEASE_MS = 2000
RX_TIMEOUT_MS = 3000   # 与 agent 里的下行静默看门狗窗口一致


class Helper:
    """扮演助手：监听、接受连接、按脚本发命令、记录 agent 上行。"""

    def __init__(self) -> None:
        self.srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(("127.0.0.1", 0))
        self.srv.listen(4)
        self.port = self.srv.getsockname()[1]
        self.conn: socket.socket | None = None
        self.lines: list[dict] = []
        self._rx = b""
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def _serve(self) -> None:
        self.srv.settimeout(15.0)
        try:
            conn, _addr = self.srv.accept()
        except (socket.timeout, OSError):
            return
        self.conn = conn
        conn.settimeout(0.5)
        while not self._stop.is_set():
            try:
                chunk = conn.recv(4096)
            except socket.timeout:
                continue
            except OSError:
                break
            if not chunk:
                break
            self._rx += chunk
            while b"\n" in self._rx:
                raw, self._rx = self._rx.split(b"\n", 1)
                try:
                    self.lines.append(json.loads(raw.decode("utf-8", "replace")))
                except ValueError:
                    pass

    def send(self, obj: dict) -> None:
        if self.conn is None:
            return
        try:
            self.conn.sendall((json.dumps(obj) + "\n").encode("ascii"))
        except OSError:
            pass

    def of(self, kind: str) -> list[dict]:
        return [line for line in self.lines if line.get("type") == kind]

    def last(self, kind: str) -> dict | None:
        found = self.of(kind)
        return found[-1] if found else None

    def wait_for(self, kind: str, timeout: float, predicate=None) -> dict | None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            for line in reversed(self.lines):
                if line.get("type") == kind and (predicate is None or predicate(line)):
                    return line
            time.sleep(0.05)
        return None

    def close(self) -> None:
        self._stop.set()
        try:
            if self.conn is not None:
                self.conn.close()
        except OSError:
            pass
        try:
            self.srv.close()
        except OSError:
            pass


class ReconnectHarness:
    """可以**主动下线再上线**（同一个端口）的假助手，并统计上线后被连了几次。

    为什么需要它：2026-09-23 真机接管轮里，agent 一口气连了 3 次、其中 2 条被
    RST（助手侧 `10054`/`10053`）。成因是"助手缺席期间 connect 堆积"——
    `Socket.connect` 到已关闭端口要约 2.2s 才 reject，而心跳周期 1s，
    于是每次重连窗口里都会同时有 2~3 个 connect 在飞，全部成功后就互相覆盖 `sock`。
    能分辨"修复前/修复后"的判据只有一个：**重新上线后被接受的连接数**。
    """

    def __init__(self) -> None:
        self.port = self._free_port()
        self.srv: socket.socket | None = None
        self.conns: list[socket.socket] = []
        self.accept_total = 0
        self.hello_total = 0
        self.lines: list[dict] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._acc: threading.Thread | None = None

    @staticmethod
    def _free_port() -> int:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        return port

    def listen(self) -> None:
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(("127.0.0.1", self.port))
        srv.listen(16)
        self.srv = srv
        self._stop.clear()
        self._acc = threading.Thread(target=self._accept_loop, daemon=True)
        self._acc.start()

    def _accept_loop(self) -> None:
        srv = self.srv
        if srv is None:
            return
        srv.settimeout(0.2)
        while not self._stop.is_set():
            try:
                conn, _addr = srv.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            with self._lock:
                self.accept_total += 1
                self.conns.append(conn)
            threading.Thread(target=self._read_loop, args=(conn,), daemon=True).start()

    def _read_loop(self, conn: socket.socket) -> None:
        """读上行；收到 hello 就为该连接启动续约（模拟真助手，避免又触发看门狗）。"""
        conn.settimeout(0.3)
        buf = b""
        renewer_started = False
        while not self._stop.is_set():
            try:
                chunk = conn.recv(4096)
            except socket.timeout:
                continue
            except OSError:
                return
            if not chunk:
                return
            buf += chunk
            while b"\n" in buf:
                raw, buf = buf.split(b"\n", 1)
                try:
                    line = json.loads(raw.decode("utf-8", "replace"))
                except ValueError:
                    continue
                with self._lock:
                    self.lines.append(line)
                    if line.get("type") == "hello":
                        self.hello_total += 1
                if line.get("type") == "hello" and not renewer_started:
                    renewer_started = True
                    threading.Thread(target=self._renew_loop, args=(conn,), daemon=True).start()

    def _renew_loop(self, conn: socket.socket) -> None:
        while not self._stop.is_set():
            try:
                conn.sendall(b'{"type":"renew"}\n')
            except OSError:
                return
            time.sleep(0.5)

    def offline(self) -> None:
        """下线：关掉监听与**所有已接受连接**（等价于助手进程整体消失）。

        必须把已接受的连接也关掉，否则那条 TCP 仍然是 ESTABLISHED，
        与"助手死了"的真实状态不符。注意：agent 侧**看不到**这个关闭
        （对端关闭后 write 不抛错，且挂起的 read 不以 EOF 结束），
        它只能靠下行静默看门狗发现——这正是要测的那条路径。
        """
        self._stop.set()
        if self.srv is not None:
            try:
                self.srv.close()
            except OSError:
                pass
            self.srv = None
        with self._lock:
            conns = list(self.conns)
            self.conns = []
        for conn in conns:
            try:
                conn.close()
            except OSError:
                pass

    def wait_for_line(self, kind: str, timeout: float) -> dict | None:
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self._lock:
                for line in reversed(self.lines):
                    if line.get("type") == kind:
                        return line
            time.sleep(0.05)
        return None

    def close(self) -> None:
        self.offline()


class Sandbox:
    """一个哑进程 + 一份加载好的 agent。"""

    def __init__(self, port: int | None) -> None:
        self.device = frida.get_local_device()
        self.pid = self.device.spawn([PYTHON, "-c", "import time; time.sleep(60)"])
        self.session = self.device.attach(self.pid)
        source = AGENT.read_text(encoding="utf-8")
        self.script = self.session.create_script(source)
        self.errors: list[str] = []
        self.script.on("message", self._on_message)
        self.script.load()
        self.device.resume(self.pid)
        self.port = port

    def _on_message(self, message: dict, _data) -> None:
        if message.get("type") == "error":
            self.errors.append(str(message.get("description") or message))

    def init(self, timeout: float = 8.0) -> tuple[str, float]:
        """调用 rpc.exports.init，返回 (结果, 耗时秒)。"""
        params = {"port": self.port} if self.port else {}
        box: dict = {}

        def run() -> None:
            try:
                box["result"] = self.script.exports_sync.init("early", params)
            except Exception as exc:  # noqa: BLE001
                box["error"] = f"{type(exc).__name__}: {exc}"

        thread = threading.Thread(target=run, daemon=True)
        started = time.time()
        thread.start()
        thread.join(timeout)
        elapsed = time.time() - started
        if "error" in box:
            return f"error:{box['error']}", elapsed
        if "result" not in box:
            return "TIMEOUT_blocked", elapsed
        return f"settled:{json.dumps(box['result'])}", elapsed

    def close(self) -> None:
        try:
            self.session.detach()
        except Exception:  # noqa: BLE001
            pass
        try:
            self.device.kill(self.pid)
        except Exception:  # noqa: BLE001
            pass


def case_a_no_helper() -> tuple[bool, str]:
    """有端口但无人监听：init 必须靠超时兜底 settle，绝不能挂住宿主 entrypoint。

    注意不能省掉端口——省掉会让 connect 因参数缺失而**立刻**失败，
    那样测的是入口校验，不是"connect 永不 settle"这条真实风险路径
    （实测 Socket.connect 连到关闭端口既不 resolve 也不 reject）。
    """
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    probe.bind(("127.0.0.1", 0))
    dead_port = probe.getsockname()[1]
    probe.close()

    box = Sandbox(port=dead_port)
    try:
        result, elapsed = box.init(timeout=10.0)
        ok = result.startswith("settled:") and 2.0 <= elapsed < 8.0
        note = (
            f"init {result} 耗时 {elapsed:.2f}s"
            f"（期望 2.0-8.0s：必须由 CONNECT_TIMEOUT_MS 兜底，而不是立刻返回）"
        )
        return ok, note
    finally:
        box.close()


def case_b_protocol() -> tuple[bool, str]:
    helper = Helper()
    helper.start()
    box = Sandbox(port=helper.port)
    notes: list[str] = []
    try:
        result, elapsed = box.init(timeout=8.0)
        notes.append(f"init {result} 耗时 {elapsed:.2f}s")

        hello = helper.wait_for("hello", 4.0)
        if hello is None:
            return False, "；".join(notes + ["未收到 hello"])

        # 收到续约之前：lease_ok 必须为 false（未武装）
        hb0 = helper.wait_for("hb", 3.0, lambda l: l.get("lease_ok") is False)
        if hb0 is None:
            notes.append("未观察到 renew 前的 hb.lease_ok=false")
        else:
            notes.append(f"续约前 hb: lease_ok={hb0.get('lease_ok')} handshake={hb0.get('handshake')}")

        # 持续续约 → lease_ok=true
        for _ in range(6):
            helper.send({"type": "renew"})
            time.sleep(0.25)
        armed = helper.wait_for("hb", 3.0, lambda l: l.get("lease_ok") is True)
        if armed is None:
            return False, "；".join(notes + ["续约后仍未出现 hb.lease_ok=true"])
        notes.append(f"续约后 hb: lease_ok={armed.get('lease_ok')} handshake={armed.get('handshake')}")

        # 停止续约 → 最迟 LEASE_MS 后 lease_ok=false
        dropped = helper.wait_for(
            "hb", (LEASE_MS / 1000.0) + 4.0, lambda l: l.get("lease_ok") is False
        )
        if dropped is None:
            return False, "；".join(notes + ["停续约后 lease_ok 未回落"])
        notes.append(
            f"停续约后 hb: lease_ok={dropped.get('lease_ok')} "
            f"since_renew_ms={dropped.get('since_renew_ms')}"
        )
        stat = dropped.get("stat") or {}
        notes.append(f"计数: target_hits={stat.get('target_hits')} clears_ok={stat.get('clears_ok')}")
        return True, "；".join(notes)
    finally:
        box.close()
        helper.close()


def case_c_disarm() -> tuple[bool, str]:
    helper = Helper()
    helper.start()
    box = Sandbox(port=helper.port)
    try:
        box.init(timeout=8.0)
        hello = helper.wait_for("hello", 4.0)
        if hello is None:
            return False, "未收到 hello"
        for _ in range(6):
            helper.send({"type": "renew"})
            time.sleep(0.2)
        if helper.wait_for("hb", 3.0, lambda l: l.get("lease_ok") is True) is None:
            return False, "未进入已武装状态"
        helper.send({"type": "disarm", "instance": hello.get("instance"), "stop_id": "selftest-stop"})
        stopped = helper.wait_for("stopped", 3.0, lambda line:
            line.get("instance") == hello.get("instance") and line.get("stop_id") == "selftest-stop"
            and line.get("hook_detached") is True and line.get("released_all") is True)
        if stopped is None:
            return False, "未收到同 Agent 与本次请求的真实 stopped 回执"
        off = helper.wait_for("hb", 3.0, lambda l: l.get("disarmed") is True)
        if off is None:
            return False, "disarm 后 hb.disarmed 未置真"
        return True, f"disarm 后 hb: disarmed={off.get('disarmed')} lease_ok={off.get('lease_ok')}"
    finally:
        box.close()
        helper.close()


def case_d_reconnect() -> tuple[bool, str]:
    """助手消失一段时间后重新上线：agent 必须**只连一次**。

    这一条直接对应 2026-09-23 真机接管轮的现象（3 条连接、2 条 RST）。
    要点是让"端口关闭"的窗口足够长（这里 7s ≫ 一次 connect 的 settle 时间 ~2.2s），
    这样才能真造出"多个 connect 同时在飞"的局面——窗口太短的话，
    即使没有并发守卫也只会连一次，用例就失去分辨力（会假通过）。
    """
    harness = ReconnectHarness()
    harness.listen()
    box = Sandbox(port=harness.port)
    notes: list[str] = []
    try:
        result, elapsed = box.init(timeout=8.0)
        notes.append(f"init {result[:40]} 耗时 {elapsed:.2f}s")
        if not harness.wait_for_line("hello", 5.0):
            return False, "；".join(notes + ["首次未收到 hello"])

        # 下线，让端口进入"无人监听"状态。
        harness.offline()
        offline_s = 7.0
        time.sleep(offline_s)

        # 重新上线，然后看它一共连了几次。
        before_accept = harness.accept_total
        before_hello = harness.hello_total
        harness.listen()
        time.sleep(8.0)
        got_accept = harness.accept_total - before_accept
        got_hello = harness.hello_total - before_hello
        notes.append(
            f"下线 {offline_s:.0f}s 后重新上线：被连 {got_accept} 次 / 收到 hello {got_hello} 条"
        )
        notes.append(
            f"累计 accept={harness.accept_total} hello={harness.hello_total}"
        )
        stat = None
        for line in reversed(harness.lines):
            if line.get("type") == "hb" and isinstance(line.get("stat"), dict):
                stat = line["stat"]
                break
        lost = 0
        if stat is not None:
            lost = int(stat.get("rx_timeouts") or 0) + int(stat.get("read_errors") or 0)
            notes.append(
                f"agent 计数: 断线可见={lost}"
                f"(rx_timeouts={stat.get('rx_timeouts')} read_errors={stat.get('read_errors')}) "
                f"connect_raced={stat.get('connect_raced')} "
                f"send_dropped={stat.get('send_dropped')}"
            )
        else:
            notes.append("未读到带 stat 的 hb")
        # 两个判据缺一不可：
        #  1) 只连一次（并发守卫生效）；
        #  2) "断过线"这件事必须能在统计里看到（此前 read 失败那条路径不计数，
        #     重连确实发生了却 rx_timeouts=0/auth_rejected=0，排查时会读成"看门狗从未触发"）。
        return (got_accept == 1 and lost > 0), "；".join(notes)
    finally:
        box.close()
        harness.close()


def case_e_targets_guard() -> tuple[bool, str]:
    """`targets` 命令的两条护栏，外加「被拒的命令不得改动状态」。

    为什么值得单独立一条：清空范围是 agent 唯一的行为开关。若远端能借一条畸形
    targets 把范围改坏，真机表现是最难查的一类 —— 命令日志显示"已处理"、握手全绿，
    但按键永久不可见（上报被关）或永久漏清（clear 缺了目标键）。所以这里既验护栏，
    也验拒绝之后状态没变。

    2026-10-02 同步现状（此前两条断言已过期，会在最新 agent 上恒 FAIL）：
      - 清空范围不再有"默认三键"：自 6a4aaf8 起完全由助手动态下发（agent 内默认空集），
        所以基线断言改成"未下发时为空"，再下发合法 targets 建立基线；
      - 白名单护栏的拒绝文案是 `targets:rejected_outside_whitelist`
        （旧文案 `..._report_not_targets` 已不存在）。
    """
    helper = Helper()
    helper.start()
    box = Sandbox(port=helper.port)
    notes: list[str] = []
    three = [0x00F1, 0x0080, 0x0081]
    try:
        box.init(timeout=8.0)
        if helper.wait_for("hello", 4.0) is None:
            return False, "未收到 hello"
        for _ in range(4):
            helper.send({"type": "renew"})
            time.sleep(0.2)

        def clear_now(timeout: float = 3.0):
            hb = helper.wait_for("hb", timeout)
            return None if hb is None else hb.get("clear_usages")

        base = clear_now()
        notes.append(f"未下发 targets 时 clear_usages={base!r}")
        if base != "":
            return False, "；".join(notes + ["未下发 targets 时清空集合应为空（动态下发设计）"])

        # 1) 合法：三键 + 哨兵键 0x4A（主页）——这正是验收要用的形状
        #    （generation 是必填：缺了会被 targets:rejected_bad_generation 拒掉，
        #     这正是此前的用例 E 恒 FAIL 的第二个原因）
        helper.send({"type": "targets", "report": three,
                     "clear": three + [0x004A], "generation": 3})
        applied = helper.wait_for(
            "log", 3.0, lambda l: "targets:applied" in str(l.get("msg", ""))
        )
        after = None
        for _ in range(12):
            helper.send({"type": "renew"})
            after = clear_now(2.0)
            if after == "0x00f1,0x0080,0x0081,0x004a":
                break
            time.sleep(0.2)
        notes.append(f"追加哨兵后 clear_usages={after}")
        if applied is None or after != "0x00f1,0x0080,0x0081,0x004a":
            return False, "；".join(notes + ["合法 targets 未生效"])

        # 2) 非法三连：上报集合含白名单外 usage / clear 缺 report / 数组含 0
        bad = [
            (
                {"type": "targets", "report": [0x1234], "clear": [0x1234], "generation": 4},
                "targets:rejected_outside_whitelist",
            ),
            (
                {"type": "targets", "report": three, "clear": [0x00F1], "generation": 5},
                "targets:rejected_clear_lacks_report",
            ),
            (
                {"type": "targets", "report": three,
                 "clear": [0, 0x00F1, 0x0080, 0x0081], "generation": 6},
                "targets:rejected_bad_array",
            ),
        ]
        for cmd, expect in bad:
            helper.send(cmd)
            got = helper.wait_for(
                "log", 3.0, lambda l, e=expect: e in str(l.get("msg", ""))
            )
            if got is None:
                notes.append(f"缺少拒绝日志 {expect}")
                return False, "；".join(notes)

        # 3) 三次被拒之后，清空范围必须仍是「三键 + 哨兵」，一个都不许少
        helper.send({"type": "renew"})
        time.sleep(0.4)
        final = clear_now(3.0)
        notes.append(f"三次被拒之后 clear_usages={final}")
        if final != "0x00f1,0x0080,0x0081,0x004a":
            return False, "；".join(notes + ["被拒命令改动了清空范围"])
        return True, "；".join(notes)
    finally:
        box.close()
        helper.close()


def case_f_burst_writes() -> tuple[bool, str]:
    """一次回调里连写的上行必须**全部**到达（Frida 17 `output.write()` 单飞互斥回归项）。

    2026-10-02 真机现象：助手 `[TARGETS-RESEND] gen=2 attempt=1..72` 每 10s 一次无限增长，
    而 `targets_ack` / `synth_ack` 一次都没到；agent 侧只有每个回调的**第一条**
    `logLine` 到得了助手（`cmd:targets` 806 条，`targets:applied` 0 条），
    但定时器每 tick 只写一行的 `hb` 一直正常。

    根因（2026-10-02 本机 frida 17.18.0 实测，`output.write()` 的最小探针输出）：
        A1:write_ret=[object Promise] | A1:resolve=9
        A2:reject=Error: stream has outstanding operation
        A3:reject=Error: stream has outstanding operation
        到达服务端: A1-first / B1-first / C1-serial / C2-after-await   （A2/A3 零字节）
    即 `write()` 返回 Promise 且**同一时刻只允许一个在飞**：上一次未 settle 时的
    第二次 write 立刻 reject、零字节送达、**不抛错**；`sendLine` 火并忘 ⇒ 整类静默丢失。
    修法见 `rc003_agent.js` 的「出站串行化」（队列 + settle 驱动逐行冲刷）。

    判据：targets / synth / mode 三条命令的**每一行**上行都必须到达，且到达顺序与发出
    顺序一致；另断言 `tx_busy_drop == 0`、`write_fail == 0`、`tx_pending_peak >= 2`
    （后者证明本用例确实制造了"同一回调连写多行"的形状）。

    边界：`synth:frame` 只能在真机的 IOCTL 回调里产生（沙箱里没有目标 IOCTL），
    这里覆盖的是同一出口（sendLine → 出站队列）的同类形状；真机 `synth:frame`
    需在真机验收时另行确认（deferred）。
    """
    helper = Helper()
    helper.start()
    box = Sandbox(port=helper.port)
    notes: list[str] = []
    three = [0x00F1, 0x0080, 0x0081]
    try:
        box.init(timeout=8.0)
        if helper.wait_for("hello", 4.0) is None:
            return False, "未收到 hello"
        for _ in range(6):
            helper.send({"type": "renew"})
            time.sleep(0.2)

        def mark() -> int:
            return len(helper.lines)

        def view(start: int) -> list[str]:
            out = []
            for line in helper.lines[start:]:
                if line.get("type") == "log":
                    out.append("log:" + str(line.get("msg", ""))[:32])
                else:
                    out.append(str(line.get("type")))
            return out

        def idx_of(start: int, pred) -> int:
            for i, line in enumerate(helper.lines[start:]):
                if pred(line):
                    return i
            return -1

        def is_log(msg: str):
            return lambda l: l.get("type") == "log" and str(l.get("msg", "")) == msg

        def has_log(sub: str):
            return lambda l: l.get("type") == "log" and sub in str(l.get("msg", ""))

        def is_type(kind: str):
            return lambda l: l.get("type") == kind

        def wait_until(pred, timeout: float) -> bool:
            deadline = time.time() + timeout
            while time.time() < deadline:
                if pred():
                    return True
                time.sleep(0.05)
            return False

        def wait_new_hb(start: int, timeout: float):
            """等一条**新到**的 hb。

            不能用 Helper.wait_for：它会立刻返回缓冲区里已有的旧 hb（"命令之前"的快照），
            用它读计数会得到假结论——2026-10-02 实测就踩过：整条用例只跑 1.3s，
            旧 hb 里 tx_lines=3 / peak=1，看起来像"修复没生效"。
            """
            deadline = time.time() + timeout
            while time.time() < deadline:
                for line in helper.lines[start:]:
                    if line.get("type") == "hb":
                        return line
                helper.send({"type": "renew"})
                time.sleep(0.1)
            return None

        # 0) 基线：突发前的计数快照（必须是新到的 hb）
        m0 = mark()
        helper.send({"type": "renew"})
        hb0 = wait_new_hb(m0, 3.0)
        if hb0 is None:
            return False, "；".join(notes + ["未等到基线 hb"])
        st0 = hb0.get("stat") or {}
        notes.append("基线: tx_lines={} peak={}".format(st0.get("tx_lines"), st0.get("tx_pending_peak")))

        # 1) targets：同一回调连写 3 行（cmd:targets / targets_ack / targets:applied）
        start = mark()
        helper.send({"type": "targets", "report": three,
                     "clear": three + [0x004A], "generation": 7})
        if not wait_until(lambda: idx_of(start, is_type("targets_ack")) >= 0
                          and idx_of(start, has_log("targets:applied")) >= 0, 4.0):
            notes.append("targets 后收到: " + " | ".join(view(start)))
            return False, "；".join(notes + ["targets_ack / targets:applied 未全部到达（多行上行被丢）"])
        i_cmd = idx_of(start, is_log("cmd:targets"))
        i_ack = idx_of(start, is_type("targets_ack"))
        i_app = idx_of(start, has_log("targets:applied"))
        notes.append(f"targets 三行序号 cmd={i_cmd} ack={i_ack} applied={i_app}")
        if not (0 <= i_cmd < i_ack < i_app):
            return False, "；".join(notes + ["到达顺序与发出顺序不一致（ack 必须在应用之后）"])
        ack = helper.lines[start + i_ack]
        if int(ack.get("generation", -1)) != 7:
            return False, "；".join(notes + [f"targets_ack.generation={ack.get('generation')}（应为 7）"])

        # 2) synth：同为同一回调 3 行
        start = mark()
        helper.send({"type": "synth", "from": 0x003E, "to": 0x00E6})
        if not wait_until(lambda: idx_of(start, is_type("synth_ack")) >= 0
                          and idx_of(start, has_log("synth:applied")) >= 0, 4.0):
            notes.append("synth 后收到: " + " | ".join(view(start)))
            return False, "；".join(notes + ["synth_ack / synth:applied 未全部到达（多行上行被丢）"])
        notes.append("synth 三行序号 cmd={} ack={} applied={}".format(
            idx_of(start, is_log("cmd:synth")), idx_of(start, is_type("synth_ack")),
            idx_of(start, has_log("synth:applied"))))

        # 3) mode：同一回调连写 2 行（纯日志形状，与 synth:frame 同出口）
        start = mark()
        helper.send({"type": "mode", "clear": True})
        if not wait_until(lambda: idx_of(start, is_log("mode:clear")) >= 0, 4.0):
            notes.append("mode 后收到: " + " | ".join(view(start)))
            return False, "；".join(notes + ["mode:clear 未到达（同一回调的第二行被丢）"])
        notes.append("mode 两行序号 cmd={} clear={}".format(
            idx_of(start, is_log("cmd:mode")), idx_of(start, is_log("mode:clear"))))

        # 4) 计数面：突发前后必须能看出"连写确实发生了"，且不许再出现单飞互斥
        m1 = mark()
        helper.send({"type": "renew"})
        hb1 = wait_new_hb(m1, 3.0)
        if hb1 is None:
            return False, "；".join(notes + ["突发后未等到新 hb"])
        st1 = hb1.get("stat") or {}
        d_lines = int(st1.get("tx_lines") or 0) - int(st0.get("tx_lines") or 0)
        notes.append(
            "突发后: tx_lines {}->{} (delta={}) tx_pending_peak={} tx_busy_drop={} write_fail={}".format(
                st0.get("tx_lines"), st1.get("tx_lines"), d_lines,
                st1.get("tx_pending_peak"), st1.get("tx_busy_drop"), st1.get("write_fail")))
        if int(st1.get("tx_busy_drop") or 0) != 0:
            return False, "；".join(notes + ["仍出现 stream has outstanding operation（修复失效）"])
        if int(st1.get("write_fail") or 0) != 0:
            return False, "；".join(notes + ["write_fail>0：出站写有失败"])
        # 本用例共发出 3(targets)+3(synth)+2(mode) = 8 行命令上行，一行都不许少
        if d_lines < 8:
            return False, "；".join(notes + [f"tx_lines 只涨了 {d_lines}（本用例应为 8 行）"])
        if int(st1.get("tx_pending_peak") or 0) < 2:
            return False, "；".join(notes + ["tx_pending_peak<2：本用例没有真的制造连写形状"])
        if box.errors:
            return False, "；".join(notes + [f"agent 抛出了脚本错误: {box.errors[:2]}"])
        return True, "；".join(notes)
    finally:
        box.close()
        helper.close()


def main() -> int:
    print("=== RC003 agent 自检台（无需提权 / 无需设备）===")
    print(f"frida {frida.__version__} | agent {AGENT.name} ({len(AGENT.read_text(encoding='utf-8').splitlines())} 行)")
    print()

    results = [
        ("A 无助手在监听：init 必须超时兜底", case_a_no_helper),
        ("B 正常协议：hello/hb/续约与租约过期", case_b_protocol),
        ("C 显式解除：disarm", case_c_disarm),
        ("D 助手消失后重新上线：只允许连一次（连接不得堆积）", case_d_reconnect),
        ("E targets 护栏：只许追加哨兵键，被拒的命令不得改动清空范围", case_e_targets_guard),
        ("F 一次回调连写多行：每一行上行都必须到达（write 单飞互斥回归）", case_f_burst_writes),
    ]

    all_ok = True
    for label, fn in results:
        try:
            ok, note = fn()
        except Exception as exc:  # noqa: BLE001
            ok, note = False, f"异常 {type(exc).__name__}: {exc}"
        all_ok = all_ok and ok
        print(f"[{'PASS' if ok else 'FAIL'}] {label}")
        print(f"       {note}")

    print()
    print(f"结论: {'全部通过' if all_ok else '存在失败用例'}")
    return 0 if all_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())

"""启动含 E1 全量采集的 debug 构建。

要点：
- 用 SAYALL_GATT_LOG 指向独立日志，避免与已退出的旧实例、以及另一会话的
  worktree 争用同一文件（两边都是 default 路径时会混在一起，没法归因）。
- 启动后打印 pid 与日志路径，便于取证时确认"这一条来自哪个进程"。
"""
import os
import subprocess
import datetime
import shutil

EXE = r"<USER-HOME>\WorkBuddy\Worktrees\remote-mic-app-windows\origin-main-01fa85f8\target\debug\sayall-windows-app.exe"
E1LOG = r"<PROBE-DIR>\e1-capture.log"
OUT = r"<PROBE-DIR>\e1-launch.txt"


def log(msg):
    print(msg, flush=True)
    with open(OUT, "a", encoding="utf-8") as f:
        f.write(msg + "\n")


open(OUT, "w", encoding="utf-8").close()

if not os.path.exists(EXE):
    log("FATAL: exe 不存在: %s" % EXE)
    raise SystemExit(2)

st = os.stat(EXE)
log("exe      : %s" % EXE)
log("size     : %d" % st.st_size)
log("built    : %s" % datetime.datetime.fromtimestamp(st.st_mtime))
log("log path : %s" % E1LOG)
log("free GB  : %.3f" % (shutil.disk_usage("C:\\").free / 1e9))

env = dict(os.environ)
env["SAYALL_GATT_LOG"] = E1LOG

p = subprocess.Popen([EXE], env=env,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     creationflags=0x00000008)  # DETACHED_PROCESS
log("launched pid = %d" % p.pid)
log("已启动。请按压遥控器实体键（TV / 主页 / 菜单 / 返回 / 音量± / 语音 / 确定 / 方向）。")

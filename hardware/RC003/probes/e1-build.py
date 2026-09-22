"""E1-a' 采集构建：安装前端依赖 -> cargo build debug 应用 exe。

设计要点：
- 显式把 PortableGit\cmd 加进 PATH：build.rs 靠 `git rev-parse HEAD` 生成
  SAYALL_SOURCE_REVISION，必须是 40 位十六进制，否则回落 "unknown"，
  就没法从日志确认跑的是哪个构建。
- 全程写日志到文件（本环境 stdout 不可见）。
- 只做 debug 构建，不做 release/打包：E1-a' 只需要 Raw Input 收到 WM_INPUT。
"""
import os, sys, subprocess, pathlib, datetime, shutil

REPO = r"<USER-HOME>\WorkBuddy\Worktrees\remote-mic-app-windows\origin-main-01fa85f8"
GIT_CMD = r"<USER-HOME>\.workbuddy\binaries\PortableGit\versions\1.2.0\cmd"
GIT_MINGW = r"<USER-HOME>\.workbuddy\binaries\PortableGit\versions\1.2.0\mingw64\bin"
GIT_USR = r"<USER-HOME>\.workbuddy\binaries\PortableGit\versions\1.2.0\usr\bin"
CARGO_BIN = r"<USER-HOME>\.cargo\bin"
NODE_BIN = r"<USER-HOME>\.workbuddy\binaries\node\versions\22.22.2-3"
LOG = r"<PROBE-DIR>\e1-build.log"

env = dict(os.environ)
env["PATH"] = os.pathsep.join([GIT_CMD, GIT_MINGW, GIT_USR, CARGO_BIN, NODE_BIN, env.get("PATH", "")])
env["CARGO_TERM_COLOR"] = "never"
env["CI"] = "1"          # pnpm 非交互
env["PNPM_HOME"] = r"<USER-HOME>\.workbuddy\binaries\node\versions\22.22.2-3"


def log(msg):
    line = f"[{datetime.datetime.now().strftime('%H:%M:%S')}] {msg}"
    print(line, flush=True)
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(line + "\n")


def run(args, cwd=REPO, timeout=3600, label=""):
    log(f"RUN {label or ' '.join(args)}")
    r = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True,
                       encoding="utf-8", errors="replace", timeout=timeout)
    log(f"  exit={r.returncode}")
    out = (r.stdout or "").strip()
    err = (r.stderr or "").strip()
    if out:
        log("  STDOUT tail:\n" + "\n".join("    " + l for l in out.splitlines()[-25:]))
    if err:
        log("  STDERR tail:\n" + "\n".join("    " + l for l in err.splitlines()[-25:]))
    return r.returncode, out, err


def main():
    open(LOG, "w", encoding="utf-8").close()
    log("=== E1-a' build start ===")
    log("free GB " + str(round(shutil.disk_usage("C:").free / 1e9, 2)))

    # 0. 前置：git 可见性（决定 source_revision）
    rc, out, _ = run([os.path.join(GIT_CMD, "git.exe"), "rev-parse", "HEAD"], label="git rev-parse HEAD")
    log(f"HEAD = {out.strip()!r} len={len(out.strip())}")
    if rc != 0 or len(out.strip()) != 40:
        log("FATAL: git rev-parse 拿不到 40 位 SHA -> source_revision 会是 unknown")
        return 2

    # 1. 工作树是否真的带着 E1-a' 代码
    probe = pathlib.Path(REPO) / "crates" / "sayall-windows" / "src" / "raw_input_windows.rs"
    text = probe.read_text(encoding="utf-8")
    for key in ("hid_usage_unmapped", "hid_report_shape_unmapped"):
        if key not in text:
            log(f"FATAL: 工作树缺 {key}")
            return 3
    log("E1-a' 代码在树: passed")

    # 2. 前端依赖
    node_modules = pathlib.Path(REPO) / "node_modules"
    if node_modules.exists():
        log("node_modules 已存在，跳过 install")
    else:
        rc, _, _ = run([os.path.join(NODE_BIN, "pnpm.CMD"), "install", "--frozen-lockfile"],
                       timeout=1800, label="pnpm install")
        if rc != 0:
            log("pnpm install failed -> 试不带 frozen-lockfile")
            rc, _, _ = run([os.path.join(NODE_BIN, "pnpm.CMD"), "install"],
                           timeout=1800, label="pnpm install (loose)")
            if rc != 0:
                log("FAILED: 前端依赖装不上")
                return 4

    # 3. debug 构建应用 exe
    rc, _, _ = run([os.path.join(CARGO_BIN, "cargo.exe"), "build", "-p", "sayall-windows-app"],
                   timeout=5400, label="cargo build debug app")
    if rc != 0:
        log("FAILED: cargo build")
        return 5

    # 4. 找产物
    hits = []
    for root, dirs, files in os.walk(os.path.join(REPO, "target")):
        for f in files:
            if f.endswith(".exe") and "sayall" in f.lower():
                p = os.path.join(root, f)
                st = os.stat(p)
                hits.append((st.st_mtime, p, st.st_size))
    hits.sort(reverse=True)
    log("--- sayall exe candidates ---")
    for mt, p, sz in hits[:10]:
        log(f"  {datetime.datetime.fromtimestamp(mt).strftime('%m-%d %H:%M')} {sz:>12} {p}")
    if not hits:
        log("FAILED: 没找到应用 exe")
        return 6
    log(f"BUILD PASSED: {hits[0][1]}")
    log("=== E1-a' build done ===")
    return 0


if __name__ == "__main__":
    sys.exit(main())

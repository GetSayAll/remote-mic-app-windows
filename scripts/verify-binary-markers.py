# 二进制标记校验小工具（供 verify-local-test-install.ps1 调用，也可独立使用）。
#   diff  <a> <b>      输出两文件长度与差异字节数（判定安装期补丁差等）
#   count <file> <子串> 输出子串在文件字节中的出现次数（判定行为标记/修订号是否嵌入）
#   sha   <file>      输出 SHA-256
# 仅用标准库；参数子串请避免经由 PowerShell 引号传递（PS 5.1 原生参数引号会被剥掉），
# 含引号的子串请改为在该文件内定义新子命令。
import hashlib
import sys


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    mode = sys.argv[1]
    if mode == "diff":
        a = open(sys.argv[2], "rb").read()
        b = open(sys.argv[3], "rb").read()
        n = sum(1 for x, y in zip(a, b) if x != y) + abs(len(a) - len(b))
        print(f"len_a={len(a)} len_b={len(b)} differing_bytes={n}")
    elif mode == "count":
        data = open(sys.argv[2], "rb").read()
        print(data.count(sys.argv[3].encode()))
    elif mode == "sha":
        print(hashlib.sha256(open(sys.argv[2], "rb").read()).hexdigest())
    else:
        print("unknown mode")
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""E1-a' 采集读取器：从诊断日志抽取按键 usage 证据。

用法：
  python e1-read.py            # 全量统计 E1-a' 命中
  python e1-read.py --new      # 只报上次读取之后的增量（配合按压取证）

判定三件事（对应选型文档第 6 节 E1）：
 1. hid_usage_unmapped 行 -> 未归因 usage 的真实值
 2. hid_report_shape_unmapped 行 -> 报告长度是否只有 9
 3. known_voice_key=true 的 usage=0x003E 是否出现（语音键走 HID 而非单独通道）
"""
import os, sys, re, json, datetime

LOG = r"<USER-HOME>\AppData\Local\SayAll\Logs\sayall-diagnostic.log"
STATE = r"<PROBE-DIR>\e1-read-state.json"

PREFIX = ("hid_usage_unmapped", "hid_report_shape_unmapped")


def main():
    incremental = "--new" in sys.argv
    offset = 0
    if incremental and os.path.exists(STATE):
        offset = json.load(open(STATE, encoding="utf-8")).get("offset", 0)

    size = os.path.getsize(LOG)
    if offset > size:
        offset = 0  # 日志被轮转/重写

    unmapped = []      # (ts, usage, known_voice, report_len, raw)
    shapes = []        # (ts, len, raw, phase)
    starts = []        # 本次新增的 process_start
    cur_rev = None

    with open(LOG, "r", encoding="utf-8", errors="replace") as f:
        f.seek(offset)
        for line in f:
            if "app_lifecycle event=process_start" in line:
                m = re.search(r"source_revision=(\S+)", line)
                if m:
                    cur_rev = m.group(1)
                    starts.append(line.rstrip())
            if "hid_usage_unmapped" in line:
                m = re.search(r"usage=(0x[0-9A-Fa-f]+) known_voice_key=(\w+) report_len=(\d+) raw=(\S*)", line)
                ts = line[:24]
                if m:
                    unmapped.append((ts, m.group(1), m.group(2), m.group(3), m.group(4)))
                else:
                    unmapped.append((ts, "?", "?", "?", line.strip()[:200]))
            if "hid_report_shape_unmapped" in line:
                m = re.search(r"len=(\d+) raw=(\S*) phase=(\S+)", line)
                ts = line[:24]
                if m:
                    shapes.append((ts, m.group(1), m.group(2), m.group(3)))
                else:
                    shapes.append((ts, "?", "?", line.strip()[:200]))

    new_offset = os.path.getsize(LOG)
    json.dump({"offset": new_offset, "at": datetime.datetime.now().isoformat()},
              open(STATE, "w", encoding="utf-8"))

    print(f"log size = {size}  read from = {offset}  ({'增量' if incremental else '全量'})")
    print(f"当前 source_revision = {cur_rev}")
    print()
    print(f"hid_usage_unmapped 命中       = {len(unmapped)}")
    print(f"hid_report_shape_unmapped 命中 = {len(shapes)}")
    print()

    if unmapped:
        print("=== 未归因 usage（每个只记一次） ===")
        print(f"{'时间':<24} {'usage':<8} {'voice':<6} {'len':<4} raw")
        for ts, u, v, l, raw in unmapped:
            print(f"{ts:<24} {u:<8} {v:<6} {l:<4} {raw}")
        print()
    if shapes:
        print("=== 未识别报告形状 ===")
        print(f"{'时间':<24} {'len':<5} {'phase':<10} raw")
        for ts, l, raw, ph in shapes:
            print(f"{ts:<24} {l:<5} {ph:<10} {raw}")
        print()
    if starts:
        print("=== 本次新增 process_start ===")
        for s in starts:
            print(" ", s)
        print()

    if not unmapped and not shapes:
        print(">> 无 E1-a' 采集记录。若你刚按过实体键，说明该构建不含采集代码，")
        print("   或按键全部落在 button_for_usage 已映射表内（那样就不会有 unmapped 行）。")


if __name__ == "__main__":
    main()

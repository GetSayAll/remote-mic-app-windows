"""E1 采集读取器：从诊断日志抽取按键 usage 证据（v2，含全量采集）。

用法：
  python e1-read2.py            # 全量统计
  python e1-read2.py --new      # 只报上次读取之后的增量（配合按压取证）

日志行（sayall-diagnostic.log）：
  hid_report_seen len=9 report_id=0x01 raw=<HEX> known_shape=true
  hid_usage_seen  usage=0x0035 button=Some(Tv) report_len=9 report_id=0x01 raw=<HEX>
  hid_usage_unmapped usage=0x00XX known_voice_key=false report_len=9 raw=<HEX>
  hid_report_shape_unmapped len=N raw=<HEX> phase=...

回答选型文档第 6 节 E1：
 1. TV 按压对应的完整 report 清单      -> hid_usage_seen / hid_report_seen
 2. 是否只有 report id 0x01            -> hid_report_seen 的 report_id 集合
 3. 有无未归因 usage                   -> hid_usage_unmapped
"""
import os
import re
import sys
import json
import datetime

LOG = r"<USER-HOME>\AppData\Local\SayAll\Logs\sayall-diagnostic.log"
STATE = r"<PROBE-DIR>\e1-read-state2.json"


def main():
    incremental = "--new" in sys.argv
    offset = 0
    if incremental and os.path.exists(STATE):
        offset = json.load(open(STATE, encoding="utf-8")).get("offset", 0)

    size = os.path.getsize(LOG)
    if offset > size:
        offset = 0

    report_ids = {}    # report_id -> (len, raw, ts)
    usages = {}        # usage -> (button, report_len, report_id, raw, ts)
    unmapped = []      # (ts, usage, voice, len, raw)
    shapes = []        # (ts, len, raw, phase)
    starts = []
    cur_rev = None

    with open(LOG, "r", encoding="utf-8", errors="replace") as f:
        f.seek(offset)
        for line in f:
            if "app_lifecycle event=process_start" in line:
                m = re.search(r"source_revision=(\S+)", line)
                if m:
                    cur_rev = m.group(1)
                    starts.append(line.rstrip())

            if "hid_report_seen" in line:
                m = re.search(r"len=(\d+) report_id=(0x[0-9A-Fa-f]+) raw=(\S+)", line)
                if m:
                    rid = m.group(2)
                    report_ids.setdefault(rid, (m.group(1), m.group(3), line[:24]))

            if "hid_usage_seen" in line:
                m = re.search(
                    r"usage=(0x[0-9A-Fa-f]+) button=(\S+) report_len=(\d+) "
                    r"report_id=(0x[0-9A-Fa-f]+) raw=(\S+)", line)
                if m:
                    usages.setdefault(m.group(1),
                                      (m.group(2), m.group(3), m.group(4), m.group(5), line[:24]))

            if "hid_usage_unmapped" in line:
                m = re.search(
                    r"usage=(0x[0-9A-Fa-f]+) known_voice_key=(\w+) report_len=(\d+) raw=(\S*)", line)
                if m:
                    unmapped.append((line[:24], m.group(1), m.group(2), m.group(3), m.group(4)))

            if "hid_report_shape_unmapped" in line:
                m = re.search(r"len=(\d+) raw=(\S*) phase=(\S+)", line)
                if m:
                    shapes.append((line[:24], m.group(1), m.group(2), m.group(3)))

    new_offset = os.path.getsize(LOG)
    json.dump({"offset": new_offset, "at": datetime.datetime.now().isoformat()},
              open(STATE, "w", encoding="utf-8"))

    print("log size = %d  read from = %d  (%s)" % (size, offset, "增量" if incremental else "全量"))
    print("当前 source_revision = %s" % cur_rev)
    print()
    print("hid_report_seen  (report id 种类) = %d" % len(report_ids))
    print("hid_usage_seen   (usage 种类)     = %d" % len(usages))
    print("hid_usage_unmapped                = %d" % len(unmapped))
    print("hid_report_shape_unmapped         = %d" % len(shapes))
    print()

    if report_ids:
        print("=== 出现过的 report id（E1：是否只有 0x01）===")
        for rid, (ln, raw, ts) in sorted(report_ids.items()):
            print("  report_id=%s  len=%s  raw=%s  (%s)" % (rid, ln, raw, ts))
        print()
    if usages:
        print("=== 出现过的 usage（E1：各键真实 usage）===")
        print("  %-8s %-16s %-6s %-10s %s" % ("usage", "button", "len", "report_id", "raw"))
        for u, (btn, ln, rid, raw, ts) in sorted(usages.items()):
            print("  %-8s %-16s %-6s %-10s %s" % (u, btn, ln, rid, raw))
        print()
    if unmapped:
        print("=== 未归因 usage ===")
        for ts, u, v, ln, raw in unmapped:
            print("  %s usage=%s voice=%s len=%s raw=%s" % (ts, u, v, ln, raw))
        print()
    if shapes:
        print("=== 未识别报告形状 ===")
        for ts, ln, raw, ph in shapes:
            print("  %s len=%s phase=%s raw=%s" % (ts, ln, ph, raw))
        print()
    if starts:
        print("=== 本次新增 process_start ===")
        for s in starts:
            print("  " + s)
        print()

    if not (report_ids or usages or unmapped or shapes):
        print(">> 无任何采集记录。检查：① 该构建是否含采集代码（看 source_revision）"
              "② 遥控器是否已连接并被 Raw Input 枚举到 ③ 是否按过实体键")


if __name__ == "__main__":
    main()

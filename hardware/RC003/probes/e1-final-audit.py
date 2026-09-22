"""采集会话终局审计：确认 e1-capture3.log 里 HID 相关行确实为 0。

目的：报告的结论（"该设备 HID 通道零报文"）不能只靠一次抽取，
必须在会话完整结束后做一次全文件终审，且列出每一种被检查的行模式。

只读文件，不写产品代码。
"""
import os, re, collections

LOG = r"<PROBE-DIR>\e1-capture3.log"
OUT = r"<PROBE-DIR>\e1-final-audit.out"

out = []
def p(s=""):
    out.append(str(s))

if not os.path.exists(LOG):
    p("日志不存在: %s" % LOG)
    open(OUT, "w", encoding="utf-8").write("\n".join(out))
    print("WROTE")
    raise SystemExit(1)

size = os.path.getsize(LOG)
raw = open(LOG, "rb").read()
text = raw.decode("utf-8", errors="replace")
lines = text.splitlines()

p("日志: %s" % LOG)
p("大小: %d bytes, 行数: %d" % (size, len(lines)))
p()

# --- 1. 会话边界 ---
def find_all(pat):
    return [l for l in lines if re.search(pat, l)]

p("=== 1. 会话边界 ===")
starts = find_all(r"process_start|single_instance|action=start")
for l in starts[:12]:
    p("  %s" % l.strip()[:200])
p()

# --- 2. HID 相关行：逐模式计数 ---
p("=== 2. HID 相关行逐模式计数 ===")
patterns = {
    "hid_report_seen":              r"hid_report_seen",
    "hid_usage_seen":               r"hid_usage_seen",
    "hid_usage_unmapped":           r"hid_usage_unmapped",
    "hid_report_shape_unmapped":    r"hid_report_shape_unmapped",
    "任何 hid_ 前缀的 note":         r"\bhid_\w+",
    "decode/decode_report 相关":     r"decode_report|UnsupportedReportShape|unsupported_report",
    "RIM_TYPEHID / 类型 2 字样":      r"RIM_TYPEHID|type=hid|dwType=2",
}
counts = {}
for name, pat in patterns.items():
    hits = find_all(pat)
    counts[name] = len(hits)
    p("  %-28s %d" % (name, len(hits)))
    for h in hits[:6]:
        p("        %s" % h.strip()[:190])
    if len(hits) > 6:
        p("        ...（共 %d 条，已截断显示）" % len(hits))
p()

# --- 3. 键盘通道证据：按键边沿全量 ---
p("=== 3. 键盘通道证据（map_edges / map_fire / raw_input）===")
edges = find_all(r"map_edges")
fires = find_all(r"map_fire")
p("  map_edges 行数: %d" % len(edges))
p("  map_fire  行数: %d" % len(fires))
p()
p("  --- 去重后的按键（从 map_edges 抽 button=）---")
btns = collections.OrderedDict()
for l in edges:
    m = re.search(r"button=(\w+)", l)
    if m:
        btns[m.group(1)] = btns.get(m.group(1), 0) + 1
for k, v in btns.items():
    p("    %-14s %d 次" % (k, v))
p()
p("  --- map_fire 明细 ---")
for l in fires:
    p("    %s" % l.strip()[:200])
p()

# --- 4. 就绪证据 ---
p("=== 4. 监听器就绪 / 绑定证据 ===")
for pat in (r"raw_input_listener", r"binding_initial", r"matched_device_count"):
    hits = find_all(pat)
    p("  [%s] %d 条" % (pat, len(hits)))
    for h in hits[:4]:
        p("      %s" % h.strip()[:210])
p()

# --- 5. 版本/构建指纹 ---
p("=== 5. 构建指纹（确认跑的是哪个二进制）===")
for pat in (r"source_revision", r"build_channel", r"release_tag"):
    hits = find_all(pat)
    if hits:
        p("  %s" % hits[0].strip()[:220])
p()

# --- 6. 结论 ---
p()
p("=== 6. 终审结论 ===")
hid_total = counts["hid_report_seen"] + counts["hid_usage_seen"] + \
            counts["hid_usage_unmapped"] + counts["hid_report_shape_unmapped"]
p("  HID 四类采集行合计: %d" % hid_total)
p("  键盘通道按键种类: %d" % len(btns))
p("  按键边沿总条数: %d" % len(edges))
if hid_total == 0 and len(edges) > 0:
    p("  => 终审 passed：HID 通道零报文，键盘通道有事件。结论与 §7.4 一致。")
elif hid_total > 0:
    p("  => 终审 failed 预期：出现 HID 行，需重新评估 §7.4！")
else:
    p("  => 无按键边沿，样本不足，不能下结论。")

open(OUT, "w", encoding="utf-8").write("\n".join(out))
print("WROTE %s (%d lines)" % (OUT, len(out)))

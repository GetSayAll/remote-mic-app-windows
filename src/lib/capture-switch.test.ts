import { describe, expect, it } from "vitest";
import {
  captureFailureText,
  captureInactiveText,
  captureSwitchState,
  normalizeCaptureHealth,
  shouldShowCaptureInactive,
} from "./capture-switch";

describe("captureSwitchState（2026-10-10 开关三态契约）", () => {
  it("本机不可用优先于一切：即使正在操作或读到开启也报 unavailable", () => {
    expect(captureSwitchState({ enabled: true, busy: true, unsupported: true })).toBe("unavailable");
    expect(captureSwitchState({ enabled: true, busy: false, unsupported: true })).toBe("unavailable");
  });

  it("正在操作优先于读到的开关值：进行中不对外报 on/off", () => {
    expect(captureSwitchState({ enabled: true, busy: true, unsupported: false })).toBe("busy");
    expect(captureSwitchState({ enabled: false, busy: true, unsupported: false })).toBe("busy");
  });

  it("未读到（null）按关闭处理：占位符阶段不是开关态", () => {
    expect(captureSwitchState({ enabled: null, busy: false, unsupported: false })).toBe("off");
    expect(captureSwitchState({ enabled: true, busy: false, unsupported: false })).toBe("on");
    expect(captureSwitchState({ enabled: false, busy: false, unsupported: false })).toBe("off");
  });
});

describe("captureFailureText（失败文案面向普通用户）", () => {
  const NAME = "全按键支持";

  it("没有失败原文时不产出任何文案（其余情况一律不显示）", () => {
    expect(captureFailureText(null, NAME)).toBeNull();
    expect(captureFailureText(undefined, NAME)).toBeNull();
    expect(captureFailureText("", NAME)).toBeNull();
    expect(captureFailureText("   ", NAME)).toBeNull();
  });

  it("授权被取消：说清结果是保持关闭，可以再试", () => {
    const raw = "启用全按键支持失败：授权未完成（UAC 被取消）。三键捕获保持关闭，可再次打开重试。";
    expect(captureFailureText(raw, NAME)).toBe("没有完成系统授权，全按键支持保持关闭。");
  });

  it("授权流程里其它失败（提权/计划任务退出码）同样归到系统授权一句", () => {
    expect(captureFailureText("授权未完成（助手注册计划任务失败，退出码 1）。可再次打开重试。", NAME)).toBe(
      "没有完成系统授权，全按键支持保持关闭。",
    );
    expect(captureFailureText("提权安装失败：系统找不到指定的文件。", NAME)).toBe(
      "没有完成系统授权，全按键支持保持关闭。",
    );
  });

  it("本机不支持与载荷缺失照旧直说（这两句本来就是用户语言）", () => {
    expect(captureFailureText("这台电脑暂不支持全按键支持。", NAME)).toBe("这台电脑暂不支持全按键支持。");
    expect(
      captureFailureText("安装包缺少本机需要的组件，全按键支持暂不可用；请重新安装本版本。", NAME),
    ).toBe("安装包缺少本机需要的组件，全按键支持暂不可用；请重新安装本版本。");
  });

  it("关闭失败：说清功能仍是开启状态", () => {
    expect(captureFailureText("停用全按键支持失败：拒绝访问。", NAME)).toBe(
      "关闭没有完成，全按键支持仍是开启状态。",
    );
  });

  it("映射不到的原文回落通用句；任何输出都不得带内部术语", () => {
    const raw = "启用全按键支持失败：shell_execute_failed hresult=0x80070005";
    const text = captureFailureText(raw, NAME);
    expect(text).toBe("开启没有成功，全按键支持保持关闭。");
    for (const bad of ["UAC", "计划任务", "hresult", "退出码", "helper", "Gadget", "令牌", "提权"]) {
      expect(text).not.toContain(bad);
    }
  });
});

describe("「开着但没生效」提示（2026-10-13 定稿）", () => {
  it("显示条件：开关开启 + 后端说没生效 + 遥控器链路正常，三者缺一不可", () => {
    const base = { switchState: "on", health: "inactive", linkReady: true } as const;
    expect(shouldShowCaptureInactive(base)).toBe(true);
    // 开关状态不对：关着 / 正在操作 / 本机不可用都不显示。
    for (const switchState of ["off", "busy", "unavailable"] as const) {
      expect(shouldShowCaptureInactive({ ...base, switchState })).toBe(false);
    }
    // 后端说还在启动（助手正在连）或已经生效：都不显示。
    for (const health of ["off", "starting", "active"] as const) {
      expect(shouldShowCaptureInactive({ ...base, health })).toBe(false);
    }
    // 遥控器链路本身断掉：由连接区既有文案说明，这里不重复、不把用户往遥控器侧引。
    expect(shouldShowCaptureInactive({ ...base, linkReady: false })).toBe(false);
  });

  it("提示句说清「现状 + 下一步」，且不得出现内部术语或遥控器指令", () => {
    const text = captureInactiveText("全按键支持");
    expect(text).toBe(
      "全按键支持还没有生效，正在自动重试；如果一直不恢复，请把开关关掉再打开。",
    );
    // 名称随入口变化（连接页写“支持更多输入工具”）。
    expect(captureInactiveText("“支持更多输入工具”")).toBe(
      "“支持更多输入工具”还没有生效，正在自动重试；如果一直不恢复，请把开关关掉再打开。",
    );
    for (const bad of [
      "UAC",
      "计划任务",
      "退出码",
      "helper",
      "Gadget",
      "令牌",
      "提权",
      "按一下遥控器", // 2026-10-13 现场证伪：这条线断在 PC 内部，按遥控器无效。
      "唤醒",
    ]) {
      expect(text).not.toContain(bad);
    }
  });
});

describe("normalizeCaptureHealth（2026-10-14：捕获状态载荷规范化）", () => {
  it("只接受四个稳定 token；缺失 / 未知值一律 unknown（不猜测）", () => {
    expect(normalizeCaptureHealth("off")).toBe("off");
    expect(normalizeCaptureHealth("starting")).toBe("starting");
    expect(normalizeCaptureHealth("active")).toBe("active");
    expect(normalizeCaptureHealth("inactive")).toBe("inactive");
    expect(normalizeCaptureHealth(undefined)).toBe("unknown");
    expect(normalizeCaptureHealth("")).toBe("unknown");
    expect(normalizeCaptureHealth("Active")).toBe("unknown"); // 逐字比较，不做大小写宽松
    expect(normalizeCaptureHealth("nonsense")).toBe("unknown");
  });
});

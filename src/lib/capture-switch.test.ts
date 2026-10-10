import { describe, expect, it } from "vitest";
import { captureFailureText, captureSwitchState } from "./capture-switch";

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

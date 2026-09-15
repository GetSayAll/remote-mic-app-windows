import { describe, expect, it } from "vitest";
import { POINTER_MODALITY_CLASS, installFocusModalityTracking } from "./focus-modality";

function createTarget() {
  const listeners = new Map<string, Set<(event: Event) => void>>();
  return {
    target: {
      addEventListener(type: string, listener: (event: Event) => void) {
        const bucket = listeners.get(type) ?? new Set<(event: Event) => void>();
        bucket.add(listener);
        listeners.set(type, bucket);
      },
    },
    dispatch(type: string, event: Event) {
      listeners.get(type)?.forEach((listener) => listener(event));
    },
  };
}

function createBody() {
  const classes = new Set<string>();
  return {
    body: {
      classList: {
        add: (name: string) => void classes.add(name),
        remove: (name: string) => void classes.delete(name),
      },
    },
    classes,
  };
}

function keydown(key: string): Event {
  return { key } as unknown as Event;
}

describe("焦点环模态跟踪", () => {
  it("初始不抑制焦点环，使纯键盘环境仍有焦点指示", () => {
    const { target } = createTarget();
    const { body, classes } = createBody();

    installFocusModalityTracking(target as unknown as Window, body as unknown as HTMLElement);

    expect(classes.has(POINTER_MODALITY_CLASS)).toBe(false);
  });

  it("指针按下后抑制焦点环", () => {
    const { target, dispatch } = createTarget();
    const { body, classes } = createBody();
    installFocusModalityTracking(target as unknown as Window, body as unknown as HTMLElement);

    dispatch("mousedown", new Event("mousedown"));

    expect(classes.has(POINTER_MODALITY_CLASS)).toBe(true);
  });

  it("指针按下后遥控器按键（非 Tab）仍保持抑制，不点亮幽灵焦点环", () => {
    const { target, dispatch } = createTarget();
    const { body, classes } = createBody();
    installFocusModalityTracking(target as unknown as Window, body as unknown as HTMLElement);

    dispatch("mousedown", new Event("mousedown"));
    dispatch("keydown", keydown("ArrowRight"));
    dispatch("keydown", keydown("Home"));
    dispatch("keydown", keydown("Enter"));

    expect(classes.has(POINTER_MODALITY_CLASS)).toBe(true);
  });

  it("Tab 恢复焦点环显示", () => {
    const { target, dispatch } = createTarget();
    const { body, classes } = createBody();
    installFocusModalityTracking(target as unknown as Window, body as unknown as HTMLElement);

    dispatch("mousedown", new Event("mousedown"));
    dispatch("keydown", keydown("Tab"));

    expect(classes.has(POINTER_MODALITY_CLASS)).toBe(false);
  });
});

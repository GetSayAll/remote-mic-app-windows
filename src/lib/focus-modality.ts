/**
 * 焦点环输入模态跟踪（配合 styles.css 的 `body.pointer-nav` 规则）。
 *
 * 默认保留浏览器原生 `:focus-visible` 行为——纯键盘、辅助技术或从未使用指针
 * 的环境都能看到焦点指示；仅在“最近一次交互是指针”时抑制焦点环。
 *
 * 之所以要抑制：遥控器注入的按键会把 WebView 的最近输入模态切为键盘，使鼠标
 * 点过的控件凭空命中 `:focus-visible` 并亮出焦点环（配置为“打开应用”的按键不
 * 触发，因为启动目标应用会让本窗口失焦）。这类幽灵焦点环的成因必然伴随一次
 * 指针交互，因此用指针模态而非“是否按过 Tab”来判定，既能消除幽灵环，又不会
 * 剥夺非 Tab 用户的焦点指示。
 *
 * - `mousedown` 打开指针模态（抑制焦点环）
 * - `Tab` 关闭指针模态（用户明确开始键盘导航）
 */

export const POINTER_MODALITY_CLASS = "pointer-nav";

export function installFocusModalityTracking(
  target: Window = window,
  body: HTMLElement = document.body,
): void {
  target.addEventListener(
    "mousedown",
    () => body.classList.add(POINTER_MODALITY_CLASS),
    true,
  );
  target.addEventListener(
    "keydown",
    (event) => {
      if (event.key === "Tab") body.classList.remove(POINTER_MODALITY_CLASS);
    },
    true,
  );
}

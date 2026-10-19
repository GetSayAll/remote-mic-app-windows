// 常用语浮层窗口的入口：只挂载面板组件，不加载主界面（窗口由宿主创建并加载本页）。
//
// 未注入私有 UI（公开构建）时 `panelComponent()` 返回 null：页面渲染空容器即可，
// 宿主也不会打开这个窗口（接缝返回「能力不可用」）。
import { createApp, h } from "vue";

import { panelComponent } from "../private/windows-common-phrases";
import { reportFrontendEvent } from "../lib/frontend-diagnostics";
import "../styles.css";
// 浮层窗口专用覆盖：透明背景 + 去掉主界面的 940px 最小宽度（见该文件注释）。
import "./panel-window.css";

const component = panelComponent();
const root = document.querySelector<HTMLElement>("#common-phrases-panel");

if (root && component) {
  createApp(component).mount(root);
  reportFrontendEvent({
    event: "common_phrases_panel_window",
    phase: "completed",
    result: "passed",
    reason: "component_mounted",
  });
} else {
  reportFrontendEvent({
    event: "common_phrases_panel_window",
    phase: "completed",
    result: "failed",
    reason: "private_component_absent",
  });
}

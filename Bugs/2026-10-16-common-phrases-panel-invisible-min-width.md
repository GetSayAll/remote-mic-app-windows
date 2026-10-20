# 常用语面板显示空白——浮层页 html 层未覆盖主样式表的 min-width，面板被推出 300px 视口

- 状态：已修复并验证（2026-10-16）
- 影响版本：含常用语走查构建的全部本机测试包（6dcd362 起的走查包；正式发布版无此问题——正式构建不注入浮层 UI）
- 涉及文件：`src/panels/panel-window.css`（修复点）、`src/styles.css`（`html, body, #app { min-width: 940px }` 来源）

## 现象

按 Home 打开常用语面板后，窗口出现但**内容一片空白**（浅灰）。用户视角："面板出来了但上面啥都没有"。
- 日志无异常：`map_fire … result=opened`、`PANEL phase=opened/phase=shown result=visible`、`frontend event=common_phrases_panel_window … component_mounted`（组件挂载成功）。
- 按方向键的插入照常工作（`INSERT … paste submitted`）——数据链路正常，仅"看不见"。

## 根因

面板窗口视口逻辑尺寸 300×372；主样式表 `src/styles.css` 有 `html, body, #app { min-width: 940px }`。
`src/panels/panel-window.css` 当初只覆盖了 `body`（`.common-phrases-panel-body`）与 `#common-phrases-panel`，**漏掉 `html` 元素本身**：

- `html` 被撑到 940px 宽 → 文档滚动宽 940 → `#common-phrases-panel`（flex 居中容器）宽 940；
- 300 宽的视口里，300 宽的面板被居中到 `x = (940-300)/2 = 320`，**整块落在视口右侧之外**；
- 屏幕上只剩 `html` 背景色 `#f4f5f8`（= `--surface-canvas`），看起来"空白"。

### 证据（2026-10-16，pid 8756 调试实例，CDP 读取面板窗口页面）

```
vw=300 vh=372 dpr=1.5
htmlBg=rgb(244,245,248)  bodyBg=rgba(0,0,0,0)
rootRect={x:0,y:0,w:940,h:360}   panelRect={x:320,y:0,w:300,h:360}
docScroll={w:940,h:372}
```
用户截图与窗口截屏的像素均为 `(244,245,248)`（= html 背景），组件深色面板完全不可见。

### 为什么之前没发现

本地浏览器验证时默认视口宽（1600+），940 宽文档也在视口内、面板能居中显示，**只有 300px 宽的浮层窗口才会暴露**。回归验证必须在 300×372 视口下做（见下"验证"）。

## 修复

`src/panels/panel-window.css` 增加 html 层覆盖：

```css
html {
  min-width: 0;
}
```

（该文件只在浮层页面加载，覆盖不影响主窗口。选择器特异性与主表相同、加载顺序在后，必定生效。）

## 验证

- 本地浏览器（CDP 视口 300×372）：修复前 `panelRect.x=320`（视口外），修复后 `panelRect={x:0,y:0,w:300,h:360}`、`inViewport:true`、`docScroll.w=300`。
- 真机（pid 8756，走查构建）：CDP 注入同一覆盖后 `panelRect={x:0,y:0,w:300,h:360}`，外部显示窗口抓屏可见完整面板（深色圆盘 + 各条目文字），PrintWindow 同。（正式修复随新包复验。）
- 修复后重出的走查包：真机外部显示抓屏验证通过（面板完整可见，含条目文字）；修复提交见 `git log --oneline -- src/panels/panel-window.css`。

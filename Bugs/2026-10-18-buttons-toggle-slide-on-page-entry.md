# 按键页开关每次进页面都会滑动（进页过渡动画）

- 发现日期：2026-10-18（用户报障）
- 状态：修复已提交；真机零过渡对照待安静窗口补测（见「验证」）
- 影响范围：1.0.0 及此前构建；Windows 10/11；与遥控器型号、第三方工具无关。
- 功能点：按键页「启用自定义按键功能」（页头）与「锁定当前按键」（页脚）两个开关
- 现象：每次进入按键页，开关先以默认「开」出现，随后滑动到持久化的「关」
  （背景渐变 + 圆点滑动约 0.15s）。用户原话："每次进入页面，这个开关都会滑动，我不需要滑动。"
- 复现条件：① 把「启用自定义按键功能」和/或「锁定当前按键」关掉（落盘为关）；
  ② 切到其他页面再切回（或重启应用后进入按键页）→ 可见上述滑动。
  注：只有当持久化值与渲染默认值（「开」）不同时才会出现；两值相同时无可观察差异。
- 正常预期：开关从初始渲染起就呈现持久化状态，不产生过渡动画。参考实现 = 设置页
  「登录时自动启动」与同页「全按键支持」：就绪前用同尺寸占位符顶位、就绪后才创建
  开关本体（`SettingsPage.vue` 2026-09-28 定稿注释：元素"创建即带正确 checked"，
  从不存在属性变更；只禁用过渡不行——Vue 的 DOM 更新与 nextTick 同批早于绘制，
  浏览器看不到中间帧，过渡照常触发）。
- 证据：
  - 代码路径：两个开关此前都是"以默认值先渲染、异步读取返回后再赋真值"；
    `src/styles.css` 的 `.toggle-input` 带 `transition: background 0.15s` 与
    `::after { transition: left 0.15s }` → 属性变更即产生可见滑动。
  - 真机阳性对照（旧构建）：向应用 WebView 注入 CDP 探针统计 `.toggle-input` 上的
    `transitionstart` 事件——切页往返一次捕获到 `enable:background-color` +
    `enable:left` 两个事件，重复两次均复现。
- 根因：开关在持久化值读回之前就渲染；读回后再设置 `checked`，构成一次真实属性
  变更，触发 CSS 过渡。
- 修复（`src/pages/ButtonsPage.vue`，最小改动）：
  - 两个开关改为与设置页同款范式：就绪前渲染同尺寸占位符（`.toggle-placeholder`，
    已有全局样式），就绪后才创建开关本体。
  - 就绪标志（`mappingsLoaded` / `lockSelectionReady`）与真值在同一渲染批次生效
    → 元素创建即带正确 `checked`，无属性变更、无过渡。
  - `锁定当前按键` 原 `lockSelectionTouched` 迟到保护随该门移除（开关就绪前不可交互，
    保护不再可达）。
- 验证：
  - 自动化（先红后绿）：新增两条用例——`creates the mapping master switch only once
    its value is final (no entry slide)`、`creates the lock switch only once the
    persisted value is loaded (no entry slide)`；vitest 全量 373 passed；vue-tsc / vite
    build passed。
  - 真机（仿真构建 + 隔离状态目录 + CDP 过渡计数探针）：阳性对照 passed（见上）；
    修复构建的"零过渡"阴性对照 deferred——补测时段另一会话的自动化在持续
    占用应用实例（反复安装/启动/操作窗口），单实例互斥下无法获得独占窗口，
    待安静时段补测（方法同阳性对照，期望探针计数为 0 且开关末端状态正确）。
- 隐私检查：本文档与探针仅使用稳定 token 与布尔值；无个人路径、设备身份、语音内容
  或凭据。

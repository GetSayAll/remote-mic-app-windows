# 私有组件接入点（公开宿主侧）

2026-10-15。本文定义**公开仓库为私有组件预留的接口**，以及私有构建必须遵守的约束。
公开构建不含任何私有代码，也不会访问会员服务。

## 为什么要有这层

会员页面、账号客户端和远端连接实现保留在私有平台仓库；公开仓库只提供**接入点**。
这样公开仓库保持可编译、可测试、可运行，而私有能力通过构建期注入装配进来。
同一条边界在 macOS 侧已有先例：公开宿主在缺少私有包时整条链路显示「正式服务尚未配置」，
但仍能编译和测试。

硬性约束：

- 公开 `Cargo.toml` / `package.json` **不得声明任何私有依赖**。
- 私有组件**不得修改**公开仓库中本文件列出的接入点以外的文件。
- 私有构建失败或缺失时，公开构建必须仍然完整可用（只是相关入口不出现）。

## Rust 侧接入点

| 位置 | 作用 |
| --- | --- |
| `src-tauri/src/membership/runtime.rs` | `MembershipRuntime` 接口、`MembershipSnapshot`、`EmailCodeDelivery`、`MembershipCatalog`、`MembershipPurchase`、`MembershipError` 与公开空实现 `UnavailableMembershipRuntime` |
| `src-tauri/src/membership/hook.rs` | **注入点**。公开版本返回 `None`；私有构建用同名文件替换其内容 |
| `src-tauri/src/membership/mod.rs` | `install(usage_statistics_path)`：拿到 `hook` 的运行时，拿不到就用空实现；路径是公开宿主的 `usage-statistics.json`，仅供私有上传器读取 |
| `src-tauri/src/lib.rs` | 把运行时放进 `AppState`；生命周期接线：`get_membership_snapshot`、`refresh_membership`、`start_membership_email_login`、`verify_membership_email_login`、`logout_membership`、`get_membership_catalog`、`purchase_membership`、`refresh_membership_purchase`、`clear_membership_purchase`、`open_membership_checkout`、窗口获得焦点、`RunEvent::ExitRequested` |

接口契约：

- `snapshot()` 必须**非阻塞**（界面按秒轮询）。
- 所有返回 `Result` 的操作失败时只回**稳定错误码**，不把服务端文案或不透明标识带到前端。
- 登录相关方法都有默认实现（返回「不可用」），公开构建因此无需改动即可满足接口。
- `on_foreground()` / `shutdown()` 成对存在；公开实现是 no-op。
- `MembershipSnapshot` 是隐私边界：只放展示所需的派生值（账号显示名、是否已开通、
  可用截止时刻）。**凭据、令牌、邮箱原文、安装标识、设备标识一律不越过 Rust 边界。**
- `catalog()` / `purchase()` / `refresh_purchase()` 会走网络，宿主用
  `spawn_blocking` 调用，不得占用窗口线程。
- 商品与报价只用于展示；`purchase()` 只接受页面传来的 `product_id`，金额、币种与
  销售修订号由私有实现从服务端方案回填。
- 收银台 URL 只由私有实现保存，`open_membership_checkout` 只打开这个值；页面不能
  传入任意 URL。打开前仍强制 `https://`。
- 订单状态 `pending` / `paid` / `failed` / `refunded` /
  `refund_pending_settlement` / `closed` 原样交给页面；只有 `paid` 伴随一次签名
  权益刷新。

私有构建要做的事：

1. 用私有内容替换 `hook.rs`，在其中构造私有运行时并返回；
2. 把私有 Rust 代码与依赖以私有构建自己的方式装配（例如在私有构建脚本里补依赖条目），
   不修改公开清单；
3. 私有实现自行负责：网络、凭据加密存储（DPAPI）、权益验签、刷新串行化与代次丢弃。
4. 匿名用量上传也只存在于私有 `hook.rs`：公开宿主只传本地统计文件路径，不新增
   上传接口、域名、协议或权限。

## 前端接入点

| 位置 | 作用 |
| --- | --- |
| `src/private/feature-pages.ts` | **注入点**。公开版本导出空数组；私有构建用同名文件替换其内容 |
| `src/navigation.ts` | `mergeNavigationItems()` 把私有页面追加到公开页面之后 |
| `src/App.vue` | 把私有页面组件并入 `activePage` 的组件表 |

约束：

- 私有页面的 `id` 不得与公开页面（`buttons` / `connection` / `permissions` / `settings`）冲突。
- 公开构建空注册表时，侧栏与页面表**逐项不变**（仓库规则：未实现的页面不显示空入口）。
  `src/navigation-seam.test.ts` 钉住这条。
- 私有页面自己负责 IPC、文案、加载态与失败态；公开代码不感知其存在。
- 文案仍需遵守 [docs/product-copy.md](../product-copy.md)。

## 验证

公开构建（本仓库 CI 即覆盖）：

```sh
corepack pnpm test
corepack pnpm build
cargo test -p sayall-windows-app --lib
cargo check -p sayall-windows-app --features runtime-simulation
```

私有构建需要额外证明：**去掉私有组件后公开构建仍完整通过**，以及注入后私有入口出现。
两者都要有可复现的执行记录。

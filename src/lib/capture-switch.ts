/**
 * 「支持更多输入工具」/「全按键支持」开关的状态与失败文案（2026-10-10 Andy 定稿）。
 *
 * 设计契约（写进 docs/product-copy.md，以后同类开关一律照此）：
 * - 开关与状态提示**合并成一个开关**：不再有独立的状态字、状态圆点或状态胶囊；
 * - 开关只有三种状态：关闭 / 开启 / 正在操作（进行中由开关自身的置灰 + 转圈表达，
 *   不配任何文字）；
 * - **只有开启失败（或关闭失败）才显示文案**，且必须是普通用户能读懂的原因：
 *   不出现 UAC、计划的 task、退出码、可执行文件名等内部细节（原文只进诊断日志）；
 * - 功能启动中（助手还在连）不显示文案——正在操作由开关态表达。
 */

import { t } from "./i18n";
import type {
  Rc003CaptureHealth,
  Rc003CaptureRevoked,
  Rc003CaptureUnavailable,
} from "./bridge";

/** 开关的可见状态：不可用 / 正在操作 / 开启 / 关闭。 */
export type CaptureSwitchState = "unavailable" | "busy" | "on" | "off";

/**
 * 开关状态判定（纯函数，单测钉住三态语义）。
 *
 * - `unavailable`：本机根本用不了（ARM64 无载荷，issue #206）——置灰；
 * - `busy`：正在操作（开启/关闭进行中）——置灰 + 转圈，无文字；
 * - `on` / `off`：可点，无文字。
 *
 * 未读到（`enabled === null`）按关闭处理：界面此时渲染同尺寸占位符，
 * 不是开关本体（2026-09-28 的无动画挂载约定）。
 */
export function captureSwitchState(input: {
  enabled: boolean | null;
  busy: boolean;
  unsupported: boolean;
}): CaptureSwitchState {
  if (input.unsupported) return "unavailable";
  if (input.busy) return "busy";
  return input.enabled === true ? "on" : "off";
}

/** 两个入口传本页/本开关名称：已本地化的英文名原样使用，中文名按当前语言翻译。 */
function localizedName(name: string): string {
  return /[\u4e00-\u9fff]/.test(name) ? t(name) : name;
}

/**
 * 「开关开着、但功能还没真的生效」的一句提示（2026-10-13 Andy 定稿，
 * 见 docs/product-copy.md §7 的同日决定）。
 *
 * 只在**遥控器链路正常**时显示这句：链路本身断掉时由连接区既有的文案说明
 * （「暂时没有找到小米语音遥控器…」，`ble.rs` 的 `Unreachable` 分支），这里不重复、
 * 也不把用户往遥控器侧引——这条链路断在 PC 内部（助手 ↔ 宿主内 agent），
 * 2026-10-13 现场已证伪「按遥控器就能恢复」。
 *
 * 与失败句同一位（开关旁唯一允许有文字的地方）：一次只显示一句，操作失败优先
 * （那是用户刚做的动作），其次才是这一句。
 *
 * `unavailable` 决定说哪一句（2026-10-27，issue #291）：
 * - `none`：过了宽限仍没接管，最常见的原因是慢启动 / 旧状态，**关掉再打开**确实
 *   是最有效的一招；
 * - `startup_failing`：助手反复启动不起来。这一档**不给"去找哪个程序"这种做不了的建议**
 *   （2026-10-27 Andy：用户无从定位是哪个程序）——端口这类外部占用现在由助手自己换端口
 *   解决，剩下能让用户做的是"关掉再打开一次"和"重启电脑"，两句都可执行。
 */
export function captureInactiveText(
  name: string,
  unavailable: Rc003CaptureUnavailable | "unknown" = "none",
): string {
  if (unavailable === "startup_failing") {
    return t(
      "{name}暂时没能启动，正在自动重试。如果一直不恢复，请把开关关掉再打开一次；还是不行就重启这台电脑。",
      { name: localizedName(name) },
    );
  }
  return t("{name}还没有生效，正在自动重试；如果一直不恢复，请把开关关掉再打开。", {
    name: localizedName(name),
  });
}

/**
 * 是否显示上面那句「开着但没生效」的提示（纯判定，两处入口共用，单测钉住）。
 *
 * 三个条件全满足才显示：
 * 1. 开关在**开启**态（关着、正在操作、本机不可用都不显示）；
 * 2. 后端发布的实际生效状态是 `inactive`（过了宽限仍没有所有权，见 Rust
 *    `CaptureHealth`）——`starting` 是助手还在连，属于正常过程；
 * 3. 遥控器链路正常（`linkReady`）：链路本身断掉时由连接区既有的文案说明
 *    （「暂时没有找到小米语音遥控器…」），不重复、也不把用户往遥控器侧引。
 */
export function shouldShowCaptureInactive(input: {
  switchState: CaptureSwitchState;
  health: Rc003CaptureHealth;
  linkReady: boolean;
}): boolean {
  return (
    input.switchState === "on" && input.health === "inactive" && input.linkReady
  );
}

/**
 * 失败原文 → 普通用户能读懂的一句话（`name` 由调用方按本页名称传入：
 * 按键页「全按键支持」、连接页“支持更多输入工具”）。
 *
 * 返回 `null` = 没有失败要显示（其余情况一律不显示文案）。原文映射不到的
 * 一律回落通用句——技术细节留在诊断日志，不摊给用户。
 */
export function captureFailureText(
  raw: string | null | undefined,
  name: string,
): string | null {
  const text = (raw ?? "").trim();
  if (!text) return null;
  const subject = localizedName(name);
  if (text.includes("授权") || text.includes("UAC") || text.includes("提权")) {
    return t("没有完成系统授权，{name}保持关闭。", { name: subject });
  }
  if (text.includes("暂不支持")) {
    return t("这台电脑暂不支持{name}。", { name: subject });
  }
  if (text.includes("缺少") && text.includes("组件")) {
    return t("安装包缺少本机需要的组件，{name}暂不可用；请重新安装本版本。", { name: subject });
  }
  if (text.includes("停用") || text.includes("关闭")) {
    return t("关闭没有完成，{name}仍是开启状态。", { name: subject });
  }
  return t("开启没有成功，{name}保持关闭。", { name: subject });
}

/**
 * 「开关被应用自己关掉了」的一句解释（2026-10-15，issue #264 附加观察甲）。
 *
 * 启动对账会因授权/任务形态变化把开关回落（升级后最常见），但回落是静默的：
 * 用户只看到开关自己关了、功能没了，然后得出结论"升级后更差"。这句话只说
 * **为什么**与**怎么办**，不提任务、UAC、架构这些机制词（规范见
 * `docs/product-copy.md` §3）。
 */
export function captureRevokedText(name: string): string {
  return t("{name}因为系统授权需要重新确认，已经被关掉了；重新打开一次就能继续使用。", {
    name: localizedName(name),
  });
}

/**
 * 是否显示上面那句「被关掉了」的解释：开关在**关闭**态、且后端确实报了回落原因。
 *
 * 开着的时候不显示（原因已被重新开启消解）；本机不可用（ARM64，switchState 为
 * `unavailable`）也不显示——那不是"重新打开一次"能解决的，本机不可用自有一句
 * （见 `captureFailureText`）。
 */
export function shouldShowCaptureRevoked(input: {
  switchState: CaptureSwitchState;
  revoked: Rc003CaptureRevoked | "unknown";
}): boolean {
  return (
    input.switchState === "off" &&
    input.revoked !== "none" &&
    input.revoked !== "unknown"
  );
}

/**
 * 回落原因的载荷规范化（2026-10-15）：只认六个稳定 token（与 Rust
 * `CaptureRevoke::as_str` 逐字对应）；缺失 / 未知值回落 `unknown`——界面按
 * "读不到"处理，不猜测原因、不显示文案。
 */
export function normalizeCaptureRevoked(
  raw: unknown,
): Rc003CaptureRevoked | "unknown" {
  return raw === "none" ||
    raw === "task_missing" ||
    raw === "reauth_marker_present" ||
    raw === "task_target_mismatch" ||
    raw === "task_power_policy_mismatch" ||
    raw === "unsupported"
    ? raw
    : "unknown";
}

/**
 * 「没生效的原因」载荷规范化（2026-10-27，issue #291）：只认两个稳定 token（与 Rust
 * `CaptureUnavailable::as_str` 逐字对应）；缺失 / 未知值回落 `unknown`——界面按
 * "读不到"处理，退回通用文案（不能因为字段缺失就把用户引向"别的程序占着"）。
 */
export function normalizeCaptureUnavailable(
  raw: unknown,
): Rc003CaptureUnavailable | "unknown" {
  return raw === "none" || raw === "startup_failing" ? raw : "unknown";
}

/**
 * 捕获状态的载荷规范化（2026-10-14）：桥快照里只认四个稳定 token（与 Rust
 * `CaptureHealth::as_str` 逐字对应）；缺失 / 未知值一律回落 `unknown`——界面按
 * "读不到"处理，不猜测、不显示文案。
 */
export function normalizeCaptureHealth(raw: unknown): Rc003CaptureHealth | "unknown" {
  return raw === "off" || raw === "starting" || raw === "active" || raw === "inactive"
    ? raw
    : "unknown";
}

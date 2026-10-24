import { ref } from "vue";

/** 推荐/界面实际使用的语言；`system` 在 language.ts 里解析成这两个之一。 */
export type EffectiveLanguage = "zh-Hans" | "en";

/**
 * 全局有效语言。
 *
 * 单独放在无依赖模块里，供 `language.ts`（IPC 控制）与 `i18n.ts`（文案）共享；
 * 这样 `bridge.ts` 也能安全 import `t`，不会形成 bridge ↔ language 的运行时环。
 */
export const effectiveLanguage = ref<EffectiveLanguage>("zh-Hans");

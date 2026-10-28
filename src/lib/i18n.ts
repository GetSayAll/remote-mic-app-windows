import { effectiveLanguage } from "./language-state";
import { en } from "./i18n/en";

const missing = new Set<string>();

function interpolate(
  text: string,
  params?: Record<string, string | number>,
): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (_match, key: string) =>
    params[key] === undefined ? `{${key}}` : String(params[key]),
  );
}

/**
 * 界面文案翻译：中文原文作为 key（与 Mac 的 keyed Localizable.strings 不同，
 * 但迁移成本最低且便于逐页核对）。中文模式直接返回原文；英文模式查字典。
 *
 * 在模板里直接调用 `t("...")` 会读取 `effectiveLanguage`，语言变化时 Vue 会
 * 自动重渲染。数组/选项等只计算一次的常量请改用 computed 或在模板里调用。
 */
export function t(
  source: string,
  params?: Record<string, string | number>,
): string {
  if (effectiveLanguage.value === "zh-Hans") {
    return interpolate(source, params);
  }
  const translated = en[source];
  if (translated === undefined) {
    if (!missing.has(source)) {
      missing.add(source);
      console.warn(`i18n missing translation: ${source}`);
    }
    return interpolate(source, params);
  }
  return interpolate(translated, params);
}

/** 供开发/测试检查：本次运行中实际缺失的英文 key。 */
export function missingTranslations(): string[] {
  return [...missing];
}

/** 侧栏导航文案；与页面内同名中文（如设备徽标「推荐」）使用不同命名空间。 */
export function tNav(source: string): string {
  if (effectiveLanguage.value === "zh-Hans") return source;
  return t(`nav.${source}`);
}

/** 页面标题按 Mac page.title 的语境取词（可与侧栏短标签不同）。 */
export function tPage(source: string): string {
  if (effectiveLanguage.value === "zh-Hans") return source;
  return t(`page.${source}`);
}

export { en as ENGLISH_TRANSLATIONS };

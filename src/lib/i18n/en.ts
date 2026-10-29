import common from "./en/common";
import nav from "./en/nav";
import page from "./en/page";
import settings from "./en/settings";
import buttons from "./en/buttons";
import macros from "./en/macros";
import connection from "./en/connection";
import onboarding from "./en/onboarding";
import permissions from "./en/permissions";
import reflections from "./en/reflections";
import lib from "./en/lib";
import membership from "./en/membership";
import commonPhrases from "./en/common-phrases";
import buttonProfiles from "./en/button-profiles";

/**
 * 英文文案总表。每个页面/模块一个 fragment，避免多人/多 agent 同时改同一文件。
 * key 必须是代码里 `t("...")` 的**完整中文原文**（含占位符）。
 */
export const en: Record<string, string> = {
  ...common,
  ...nav,
  ...page,
  ...settings,
  ...buttons,
  ...macros,
  ...connection,
  ...onboarding,
  ...permissions,
  ...reflections,
  ...lib,
  ...membership,
  ...commonPhrases,
  ...buttonProfiles,
};

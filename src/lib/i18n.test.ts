// @vitest-environment node

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { ENGLISH_TRANSLATIONS, t, tNav } from "./i18n";
import { effectiveLanguage } from "./language-state";
import common from "./i18n/en/common";
import nav from "./i18n/en/nav";
import settings from "./i18n/en/settings";
import buttons from "./i18n/en/buttons";
import macros from "./i18n/en/macros";
import connection from "./i18n/en/connection";
import onboarding from "./i18n/en/onboarding";
import permissions from "./i18n/en/permissions";
import lib from "./i18n/en/lib";

const sourceRoot = join(process.cwd(), "src");
const han = /\p{Script=Han}/u;
const callPattern = /\bt\(\s*(["'])((?:\\.|(?!\1)[\s\S])*?)\1\s*(?:,|\))/g;
const navCallPattern = /\btNav\(\s*(["'])((?:\\.|(?!\1)[\s\S])*?)\1\s*\)/g;

function sourceFiles(directory: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      if (entry === "i18n") continue;
      files.push(...sourceFiles(path));
      continue;
    }
    if (
      !entry.endsWith(".test.ts") &&
      !entry.endsWith(".d.ts") &&
      (entry.endsWith(".ts") || entry.endsWith(".vue"))
    ) {
      files.push(path);
    }
  }
  return files;
}

function literalKeys(): Map<string, string> {
  const keys = new Map<string, string>();
  for (const file of sourceFiles(sourceRoot)) {
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(callPattern)) {
      const key = match[2];
      if (han.test(key) && !keys.has(key)) {
        keys.set(key, relative(sourceRoot, file));
      }
    }
  }
  return keys;
}

function navKeys(): Map<string, string> {
  const keys = new Map<string, string>();
  for (const file of sourceFiles(sourceRoot)) {
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(navCallPattern)) {
      const key = `nav.${match[2]}`;
      if (!keys.has(key)) keys.set(key, relative(sourceRoot, file));
    }
  }
  return keys;
}

describe("全 UI i18n 完整性", () => {
  it("所有 t(\"中文原文\") 都有英文翻译", () => {
    const missing = [...literalKeys()]
      .filter(([key]) => ENGLISH_TRANSLATIONS[key] === undefined)
      .map(([key, file]) => `${file}: ${key}`);
    expect(missing).toEqual([]);
  });

  it("所有 tNav(\"导航项\") 都有 nav. 前缀译文", () => {
    const missing = [...navKeys()]
      .filter(([key]) => ENGLISH_TRANSLATIONS[key] === undefined)
      .map(([key, file]) => `${file}: ${key}`);
    expect(missing).toEqual([]);
  });

  it("英文模式返回英文，中文模式返回原文", () => {
    effectiveLanguage.value = "en";
    expect(t("应用语言")).toBe("App Language");
    expect(tNav("设置")).toBe("Settings");
    expect(tNav("推荐")).toBe("Recommendations");
    expect(t("推荐")).toBe("Recommended");
    effectiveLanguage.value = "zh-Hans";
    expect(t("应用语言")).toBe("应用语言");
    expect(tNav("设置")).toBe("设置");
  });

  it("各 fragment 不出现同 key 不同译文", () => {
    const fragments: Record<string, Record<string, string>> = {
      common,
      nav,
      settings,
      buttons,
      macros,
      connection,
      onboarding,
      permissions,
      lib,
    };
    const seen = new Map<string, { value: string; fragment: string }>();
    const conflicts: string[] = [];
    for (const [name, fragment] of Object.entries(fragments)) {
      for (const [key, value] of Object.entries(fragment)) {
        const previous = seen.get(key);
        if (previous && previous.value !== value) {
          conflicts.push(`${key}: ${previous.fragment}=${previous.value} / ${name}=${value}`);
        } else {
          seen.set(key, { value, fragment: name });
        }
      }
    }
    expect(conflicts).toEqual([]);
  });
});

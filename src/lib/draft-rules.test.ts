import { describe, expect, it } from "vitest";
import { applyDraftRules, draftRuleLimits, parseDraftRules, type DraftRule } from "./draft-rules";

const term = (trigger: string, replacement: string): DraftRule => ({ kind: "term", trigger, replacement });
const snippet = (trigger: string, replacement: string): DraftRule => ({ kind: "snippet", trigger, replacement });

describe("local draft rules", () => {
  it("matches the longest phrase, preserves Chinese prose, and inserts snippets literally once", () => {
    const inserted = "let 结果 = '$&';\r\n\t// 露米";
    expect(applyDraftRules("请使用鲁米助手，展开示例。鲁米", [
      term("鲁米", "露米"),
      term("鲁米助手", "露米助手"),
      term("露米", "Lumi"),
      snippet("展开示例", inserted),
    ])).toEqual({ status: "ok", text: `请使用露米助手，${inserted}。露米`, matches: 3 });
  });

  it("keeps case and identifier boundaries, including qualified names and paths", () => {
    expect(applyDraftRules("Lumi, lumi, Lumi. LumiApp Lumi$App $Lumi _Lumi 测Lumi Lumi测 config.Lumi ns::Lumi src/Lumi Lumi-cli 𐐀Lumi", [
      term("Lumi", "露米"),
    ])).toEqual({
      status: "ok",
      text: "露米, lumi, 露米. LumiApp Lumi$App $Lumi _Lumi 测Lumi Lumi测 config.Lumi ns::Lumi src/Lumi Lumi-cli 𐐀Lumi",
      matches: 2,
    });
  });

  it("preserves Markdown code, indentation, and mixed line endings", () => {
    const source = "鲁米 `鲁米` ``鲁米`鲁米``\r\n ```ts\r\n\t鲁米\r\n ```\n~~~\n鲁米\n~~~~\n  鲁米\n`鲁米\n鲁米";
    const expected = "露米 `鲁米` ``鲁米`鲁米``\r\n ```ts\r\n\t鲁米\r\n ```\n~~~\n鲁米\n~~~~\n  露米\n`鲁米\n鲁米";
    expect(applyDraftRules(source, [term("鲁米", "露米")])).toEqual({ status: "ok", text: expected, matches: 2 });
    expect(applyDraftRules("鲁米\n```\n鲁米", [term("鲁米", "露米")])).toEqual({
      status: "ok", text: "露米\n```\n鲁米", matches: 1,
    });
  });

  it("fails closed on invalid rules, including duplicate and multiline term entries", () => {
    expect(parseDraftRules(null)).toEqual([]);
    expect(parseDraftRules("[invalid")).toBeNull();
    expect(parseDraftRules(JSON.stringify([term(" trigger ", "replacement")]))).toEqual([term("trigger", "replacement")]);
    expect(applyDraftRules("鲁米", [term("鲁米", "露米"), snippet(" 鲁米 ", "duplicate")])).toEqual({ status: "invalid_rules" });
    expect(applyDraftRules("a", [term("a", "first\nsecond")])).toEqual({ status: "invalid_rules" });
    expect(applyDraftRules("a", [snippet("a", "\u0000")])).toEqual({ status: "invalid_rules" });
    expect(parseDraftRules(JSON.stringify(Array.from({ length: draftRuleLimits.count + 1 }, (_, i) => term(`${i}`, "x"))))).toBeNull();
  });

  it("rejects excessive drafts and rule expansion as a whole", () => {
    expect(applyDraftRules("鲁".repeat(draftRuleLimits.input + 1), [term("鲁", "露")])).toEqual({ status: "size_limit" });
    expect(applyDraftRules("鲁".repeat(200), [snippet("鲁", "露".repeat(draftRuleLimits.replacement))])).toEqual({ status: "size_limit" });
  });
});

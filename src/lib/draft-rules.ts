export type DraftRuleKind = "term" | "snippet";

export interface DraftRule {
  kind: DraftRuleKind;
  trigger: string;
  replacement: string;
}

export const draftRuleLimits = {
  count: 32,
  trigger: 96,
  replacement: 1024,
  input: 65_536,
  output: 131_072,
};

export const draftRulesStorageKey = "sayall-local-draft-rules-v1";

const inlineControl = /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u;
const snippetControl = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u;
const word = /[\p{L}\p{N}\p{M}\p{Pc}$\u200c\u200d]/u;
const han = /\p{Script=Han}/u;

export function normalizeDraftRules(value: unknown): DraftRule[] | null {
  if (!Array.isArray(value) || value.length > draftRuleLimits.count) return null;
  const rules: DraftRule[] = [];
  const triggers = new Set<string>();
  for (const candidate of value) {
    if (!candidate || typeof candidate !== "object") return null;
    const { kind, trigger, replacement } = candidate as Partial<DraftRule>;
    if (kind !== "term" && kind !== "snippet") return null;
    if (typeof trigger !== "string" || typeof replacement !== "string") return null;
    const phrase = trigger.trim();
    if (!phrase || trigger.length > draftRuleLimits.trigger || inlineControl.test(trigger)) return null;
    if (!replacement.trim() || replacement.length > draftRuleLimits.replacement) return null;
    if ((kind === "term" ? inlineControl : snippetControl).test(replacement)) return null;
    if (triggers.has(phrase)) return null;
    triggers.add(phrase);
    rules.push({ kind, trigger: phrase, replacement });
  }
  return rules;
}

export function parseDraftRules(raw: string | null): DraftRule[] | null {
  if (raw === null) return [];
  // JSON may encode any UTF-16 code unit as six characters, plus object keys.
  const maxJsonLength = draftRuleLimits.count * (6 * (draftRuleLimits.trigger + draftRuleLimits.replacement) + 64) + 2;
  if (raw.length > maxJsonLength) return null;
  try {
    return normalizeDraftRules(JSON.parse(raw));
  } catch {
    return null;
  }
}

function codePointAt(text: string, offset: number): string {
  const point = text.codePointAt(offset);
  return point === undefined ? "" : String.fromCodePoint(point);
}

function codePointBefore(text: string, offset: number): string {
  const tail = text.charCodeAt(offset - 1);
  const head = text.charCodeAt(offset - 2);
  const pair = tail >= 0xdc00 && tail <= 0xdfff && head >= 0xd800 && head <= 0xdbff;
  return codePointAt(text, offset - (pair ? 2 : 1));
}

function hasBoundary(left: string, right: string): boolean {
  // Running Chinese text has no whitespace between words. Other word characters
  // stay joined to protect identifiers, including mixed Chinese/Latin identifiers.
  return !word.test(left) || !word.test(right) || (han.test(left) && han.test(right));
}

function hasTriggerBoundaries(text: string, offset: number, trigger: string): boolean {
  const end = offset + trigger.length;
  const prefix = codePointBefore(text, offset);
  const suffix = codePointAt(text, end);
  const first = codePointAt(trigger, 0);
  const last = codePointBefore(trigger, trigger.length);
  if (!hasBoundary(prefix, first) || !hasBoundary(last, suffix)) return false;

  // Avoid replacing an individual part of bare identifiers such as config.Lumi,
  // ns::Lumi, src/Lumi, or Lumi-cli while retaining sentence punctuation.
  const prefixLength = offset >= 2 && text.startsWith("::", offset - 2) ? 2 : Number(/[.\-/\\]/u.test(prefix));
  const suffixLength = text.startsWith("::", end) ? 2 : Number(/[.\-/\\]/u.test(suffix));
  return !(prefixLength && word.test(first) && word.test(codePointBefore(text, offset - prefixLength))) &&
    !(suffixLength && word.test(last) && word.test(codePointAt(text, end + suffixLength)));
}

function* draftSections(text: string): Generator<{ text: string; protected: boolean }> {
  let fence = "";
  let offset = 0;
  let sectionStart = 0;
  for (const [line] of text.matchAll(/[^\r\n]+(?:\r\n|[\r\n])?|\r\n|[\r\n]/g)) {
    const marker = /^ {0,3}(`{3,}|~{3,})([^\r\n]*)/u.exec(line);
    if (marker && !fence && (marker[1][0] === "~" || !marker[2].includes("`"))) {
      yield { text: text.slice(sectionStart, offset), protected: false };
      sectionStart = offset;
      fence = marker[1];
    } else if (marker && fence && marker[1][0] === fence[0] && marker[1].length >= fence.length && !marker[2].trim()) {
      yield { text: text.slice(sectionStart, offset + line.length), protected: true };
      sectionStart = offset + line.length;
      fence = "";
    }
    offset += line.length;
  }
  yield { text: text.slice(sectionStart), protected: !!fence };
}

function* inlineSections(text: string): Generator<{ text: string; protected: boolean }> {
  const markers = [...text.matchAll(/`+/g)];
  let offset = 0;
  for (let index = 0; index < markers.length; index++) {
    const opener = markers[index];
    yield { text: text.slice(offset, opener.index), protected: false };
    do {
      index++;
    } while (index < markers.length && markers[index][0].length !== opener[0].length);
    const closer = markers[index];
    offset = closer ? closer.index + closer[0].length : text.length;
    yield { text: text.slice(opener.index, offset), protected: true };
  }
  yield { text: text.slice(offset), protected: false };
}

export type DraftRuleResult =
  | { status: "ok"; text: string; matches: number }
  | { status: "invalid_rules" | "size_limit" };

export function applyDraftRules(text: string, rules: readonly DraftRule[]): DraftRuleResult {
  if (text.length > draftRuleLimits.input) return { status: "size_limit" };
  const normalized = normalizeDraftRules(rules);
  if (!normalized) return { status: "invalid_rules" };
  if (normalized.length === 0) return { status: "ok", text, matches: 0 };
  const ordered = normalized.sort((a, b) => b.trigger.length - a.trigger.length);
  let output = "";
  let matches = 0;
  for (const block of draftSections(text)) {
    if (block.protected) {
      output += block.text;
    } else {
      for (const section of inlineSections(block.text)) {
        if (section.protected) {
          output += section.text;
          continue;
        }
        for (let offset = 0; offset < section.text.length;) {
          const first = codePointAt(section.text, offset);
          const rule = ordered.find(({ trigger }) =>
            section.text.startsWith(trigger, offset) && hasTriggerBoundaries(section.text, offset, trigger),
          );
          output += rule?.replacement ?? first;
          offset += rule?.trigger.length ?? first.length;
          if (rule && rule.trigger !== rule.replacement) matches++;
          if (output.length > draftRuleLimits.output) return { status: "size_limit" };
        }
      }
    }
    if (output.length > draftRuleLimits.output) return { status: "size_limit" };
  }
  return { status: "ok", text: output, matches };
}

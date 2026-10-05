import { describe, expect, it } from "vitest";
import fixture from "../../contracts/ipc/template-catalog-builtins.json";
import type { TemplateCatalogEntry } from "./bridge";
describe("fixed-key template IPC contract", () => {
  it("exposes one mapping model and provides fixed keys and explicit system task actions", () => {
    const entries = fixture as TemplateCatalogEntry[];
    expect(entries.map(t => t.id)).toEqual(["preset-agent", "preset-chat", "preset-browser"]);
    for (const entry of entries) {
      expect(entry.kind).toBe("direct"); expect(entry.builtIn).toBe(true);
      expect(entry).not.toHaveProperty("semanticTemplate");
      expect(entry.buttonMappings?.actions.left?.single).toEqual({type:"shortcut",chord:{keys:["left"]}});
      expect(entry.buttonMappings?.actions.home?.double).toEqual({type:"shortcut",chord:{keys:["control","home"]}});
      expect(entry.buttonMappings?.actions.tv?.single).toEqual({type:"shortcut",chord:{keys:["tab"]}});
      expect(entry.buttonMappings?.actions.tv?.double).toEqual({type:"disabled"});
      expect(entry.buttonMappings?.actions.tv?.long).toEqual({type:"task_switch",view:"desktops"});
      expect(entry.buttonMappings?.actions.ok?.long).toEqual({type:"disabled"});
    }
  });
});

import { readFileSync } from "node:fs";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { expect, it, vi } from "vitest";
import {
  getCaptureSupport,
  getCommonPhrasePanelSnapshot,
  getRc003BridgeSnapshot,
  getRc003TaskStatus,
  getRuntimeSnapshot,
  type ButtonEdge,
} from "../lib/bridge";
import ButtonsPage from "./ButtonsPage.vue";

it("routes the Chromecast center key through the Tauri JS event bridge to the diagram and row", async () => {
  const [preview, capture, commonPhrases, rc003Bridge, rc003Task] = await Promise.all([
    getRuntimeSnapshot(),
    getCaptureSupport(),
    getCommonPhrasePanelSnapshot(),
    getRc003BridgeSnapshot(),
    getRc003TaskStatus(),
  ]);
  const cast = {
    ...preview,
    platform: {
      ...preview.platform,
      connection: {
        ...preview.platform.connection,
        phase: "ready" as const,
        remoteModel: "chromecast" as const,
        remoteToken: "cast-fixture",
      },
      rawInput: {
        ...preview.platform.rawInput,
        phase: "ready" as const,
        matchedDeviceCount: 1,
        activeButtons: [],
      },
    },
  };
  const expectedEdges: ButtonEdge[] = [
    { button: "ok", isPressed: true },
    { button: "ok", isPressed: false },
  ];
  // Native QA can replay the edge array emitted by the Rust descriptor fixture.
  // Ordinary frontend runs use the same public wire contract without a Rust build.
  const fixture = process.env.SAYALL_CAST_E2E_EDGE_FIXTURE;
  const edges = fixture
    ? JSON.parse(readFileSync(fixture, "utf8")) as ButtonEdge[]
    : expectedEdges;
  expect(edges).toEqual(expectedEdges);

  let mounted = false;
  const readMapping = vi.fn();
  const stopCapture = vi.fn();
  const unexpectedCommands: string[] = [];
  mockIPC((command, args) => {
    switch (command) {
      case "get_button_mappings":
        readMapping(args);
        return { enabled: true, actions: {} };
      case "get_button_mapping_snapshot":
        return { ...preview.platform.buttonMapping, listenerActive: true };
      case "get_lock_mapping_selection":
        return true;
      case "list_preset_apps":
        return [];
      case "get_combined_actions":
        return { formatVersion: 1, definitions: [] };
      case "get_capture_support":
        return capture;
      case "get_rc003_bridge_snapshot":
        return rc003Bridge;
      case "get_rc003_task_status":
        return rc003Task;
      case "common_phrase_panel_snapshot":
        return commonPhrases;
      case "stop_shortcut_capture":
        stopCapture();
        return;
      case "report_frontend_event":
        if ((args as { report?: { event?: string } }).report?.event === "buttons_page_resource_setup") {
          mounted = true;
        }
        return;
      default:
        unexpectedCommands.push(command);
        throw new Error(`No native IPC in this renderer fixture: ${command}`);
    }
  }, { shouldMockEvents: true });

  const page = mount(ButtonsPage, { props: { runtime: cast } });
  try {
    await vi.waitFor(() => expect(mounted).toBe(true));
    expect(readMapping).toHaveBeenCalledWith({ remoteModel: "chromecast" });
    const center = () => page.get('.cast-button-lights [data-remote-button="ok"]');
    const up = () => page.get('.cast-button-lights [data-remote-button="up"]');
    const row = () => page.get('.mapping-card[data-remote-button="ok"]');
    expect(center().classes()).not.toContain("lit");
    await emit("button-edge", edges[0]);
    await flushPromises();
    expect(center().classes()).toContain("lit");
    expect(row().classes()).toContain("active");
    expect(up().classes()).not.toContain("lit");

    await emit("button-edge", edges[1]);
    await vi.waitFor(() => {
      expect(center().classes()).not.toContain("lit");
      expect(row().classes()).not.toContain("active");
    });
  } finally {
    page.unmount();
    await flushPromises();
    clearMocks();
  }
  expect(stopCapture).toHaveBeenCalledOnce();
  expect(unexpectedCommands).toEqual([]);
});

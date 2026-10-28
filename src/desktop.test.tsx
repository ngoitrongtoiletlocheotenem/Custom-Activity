import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import type { ActivityState } from "./types/application";
const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: mocks.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (name: string, callback: (e: { payload: unknown }) => void) => {
      mocks.listeners.set(name, callback);
      return () => mocks.listeners.delete(name);
    },
  ),
}));
import App from "./App";
import { defaultSettings, emptyActivity } from "./api";
const app = {
  id: "editor",
  name: "Example Editor",
  executableName: "editor.exe",
  executablePath: "C:\\Editor\\editor.exe",
  processIds: [4],
  running: true,
};
let state: ActivityState;
beforeEach(() => {
  mocks.listeners.clear();
  state = {
    ...emptyActivity,
    selectedApplication: app,
    selectedApplicationRunning: true,
    description: "Saved description",
  };
  mocks.invoke.mockImplementation(
    async (command: string, args?: Record<string, unknown>) => {
      if (command === "get_activity_state") return state;
      if (command === "get_settings")
        return { ...defaultSettings, onboardingComplete: true };
      if (command === "list_running_applications") return [app];
      if (command === "set_presence") {
        state = { ...state, description: String(args?.description) };
        return state;
      }
      if (command === "set_sharing_enabled") {
        state = { ...state, enabled: Boolean(args?.enabled) };
        return state;
      }
      throw new Error(`Unexpected command: ${command}`);
    },
  );
});
it("keeps drafts local, preserves them across backend events, and applies before enabling", async () => {
  const user = userEvent.setup();
  render(<App />);
  const input = await screen.findByRole("textbox", { name: /Description/ });
  await waitFor(() => expect(input).toHaveValue("Saved description"));
  await user.clear(input);
  await user.type(input, "A private draft");
  expect(
    mocks.invoke.mock.calls.filter((c) => c[0] === "set_presence"),
  ).toHaveLength(0);
  act(() =>
    mocks.listeners.get("presence-updated")?.({
      payload: {
        ...state,
        discordState: { status: "waiting", message: "Waiting" },
      },
    }),
  );
  expect(input).toHaveValue("A private draft");
  await user.click(
    screen.getByRole("switch", { name: "Share my current activity" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("switch", { name: "Share my current activity" }),
    ).toBeChecked(),
  );
  const mutations = mocks.invoke.mock.calls.filter((c) =>
    ["set_presence", "set_sharing_enabled"].includes(c[0]),
  );
  expect(mutations).toEqual([
    ["set_presence", { appId: "editor", description: "A private draft" }],
    ["set_sharing_enabled", { enabled: true }],
  ]);
  await user.click(
    screen.getByRole("switch", { name: "Share my current activity" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("switch", { name: "Share my current activity" }),
    ).not.toBeChecked(),
  );
  expect(mocks.invoke).toHaveBeenLastCalledWith("set_sharing_enabled", {
    enabled: false,
  });
});
it("Ctrl+Enter publishes once while ordinary typing never sends presence", async () => {
  const user = userEvent.setup();
  render(<App />);
  const input = await screen.findByRole("textbox", { name: /Description/ });
  await waitFor(() => expect(input).toHaveValue("Saved description"));
  await user.clear(input);
  await user.type(input, "New activity");
  await user.keyboard("{Control>}{Enter}{/Control}");
  await waitFor(() =>
    expect(mocks.invoke).toHaveBeenCalledWith("set_presence", {
      appId: "editor",
      description: "New activity",
    }),
  );
  expect(
    mocks.invoke.mock.calls.filter((c) => c[0] === "set_presence"),
  ).toHaveLength(1);
});

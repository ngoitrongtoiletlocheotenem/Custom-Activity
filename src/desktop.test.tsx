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
  mocks.invoke.mockClear();
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
        state = {
          ...state,
          description: String(args?.description),
          applicationNameOverride: String(args?.applicationNameOverride ?? ""),
          details: String(args?.details ?? ""),
        };
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
    [
      "set_presence",
      {
        appId: "editor",
        description: "A private draft",
        applicationNameOverride: "",
        details: "",
      },
    ],
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
      applicationNameOverride: "",
      details: "",
    }),
  );
  expect(
    mocks.invoke.mock.calls.filter((c) => c[0] === "set_presence"),
  ).toHaveLength(1);
});
it("preserves custom field drafts through disconnect events and submits all fields together", async () => {
  state = {
    ...state,
    applicationNameOverride: "Saved name",
    details: "Saved details",
    startedAt: Math.floor(Date.now() / 1000) - 125,
    enabled: true,
    published: false,
    discordState: { status: "waiting", message: "Waiting for Discord" },
  };
  const user = userEvent.setup();
  render(<App />);
  const name = await screen.findByRole("textbox", {
    name: /Discord application name/,
  });
  await waitFor(() => expect(name).toHaveValue("Saved name"));
  const details = screen.getByRole("textbox", { name: /^Details/ });
  await user.clear(name);
  await user.type(name, "My custom app");
  await user.clear(details);
  await user.type(details, "Second line");
  act(() => {
    mocks.listeners.get("presence-updated")?.({
      payload: {
        ...state,
        applicationNameOverride: "Remote old name",
        details: "Remote old details",
      },
    });
    mocks.listeners.get("discord-state-changed")?.({
      payload: { status: "error", message: "Disconnected" },
    });
  });
  expect(name).toHaveValue("My custom app");
  expect(details).toHaveValue("Second line");
  expect(screen.getByLabelText("Elapsed activity time")).toHaveTextContent(
    /2:\d{2} elapsed/,
  );
  expect(
    mocks.invoke.mock.calls.filter((c) => c[0] === "set_presence"),
  ).toHaveLength(0);
  await user.keyboard("{Control>}{Enter}{/Control}");
  await waitFor(() =>
    expect(mocks.invoke).toHaveBeenCalledWith("set_presence", {
      appId: "editor",
      description: "Saved description",
      applicationNameOverride: "My custom app",
      details: "Second line",
    }),
  );
});
it("selection restores each saved custom field and a blank name follows the selected application", async () => {
  const user = userEvent.setup();
  render(<App />);
  const name = await screen.findByRole("textbox", {
    name: /Discord application name/,
  });
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: /Description/ })).toHaveValue(
      "Saved description",
    ),
  );
  await user.type(name, "Unsent name");
  const previous = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation(
    async (command: string, args?: Record<string, unknown>) => {
      if (command === "get_foreground_application")
        return { ...app, id: "second", name: "Second Editor" };
      if (command === "select_application")
        return {
          ...state,
          selectedApplication: { ...app, id: "second", name: "Second Editor" },
          description: "Restored title",
          applicationNameOverride: "",
          details: "Restored details",
          startedAt: null,
        };
      return previous(command, args);
    },
  );
  await user.click(screen.getByRole("button", { name: "Use active window" }));
  await waitFor(() => expect(name).toHaveValue(""));
  expect(screen.getByRole("textbox", { name: /Description/ })).toHaveValue(
    "Restored title",
  );
  expect(screen.getByRole("textbox", { name: /^Details/ })).toHaveValue(
    "Restored details",
  );
  expect(
    screen.getByRole("textbox", { name: /Discord application name/ }),
  ).toHaveAttribute("placeholder", "Second Editor");
  await user.type(name, "   ");
  expect(screen.getAllByText("Second Editor", { exact: true })).toHaveLength(2);
  await user.clear(name);
  await user.type(name, "  Custom Name  ");
  expect(screen.getByText("Custom Name", { exact: true })).toBeInTheDocument();
  expect(
    screen.queryByLabelText("Elapsed activity time"),
  ).not.toBeInTheDocument();
});

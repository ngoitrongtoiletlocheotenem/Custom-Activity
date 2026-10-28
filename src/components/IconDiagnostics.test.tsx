import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
const diagnostics = vi.hoisted(() => vi.fn());
vi.mock("../api", () => ({
  desktop: true,
  api: { iconDiagnostics: diagnostics },
}));
import IconDiagnostics from "./IconDiagnostics";
beforeEach(() => { diagnostics.mockReset(); });
it("hides developer controls when the release command is unavailable", async () => {
  diagnostics.mockRejectedValue(new Error("unsupported"));
  const { container } = render(<IconDiagnostics />);
  await waitFor(() => expect(diagnostics).toHaveBeenCalledOnce());
  expect(container).toBeEmptyDOMElement();
});
it("separates local and resolved SDK icons and renders only diagnostic fields", async () => {
  diagnostics.mockResolvedValue({
    selectedApplication: { name: "Editor", executableName: "editor.exe" },
    localIcon: { available: true },
    resolvedIcon: { discordImage: "editor_asset", source: "mapped" },
    native: {
      assetsCreated: true,
      setAssetsCalled: true,
      largeImage: "editor_asset",
      largeText: "Editor",
      callbackReceived: true,
      callbackSuccessful: false,
      description: "PRIVATE_SENTINEL",
    },
  });
  render(<IconDiagnostics />);
  await userEvent.click(await screen.findByText("Developer icon diagnostics"));
  expect(screen.getByText("Local icon available")).toBeVisible();
  expect(screen.getByText("Resolved Discord image")).toBeVisible();
  expect(screen.getAllByText("editor_asset")).toHaveLength(2);
  expect(screen.queryByText("PRIVATE_SENTINEL")).not.toBeInTheDocument();
  await userEvent.click(
    screen.getByRole("button", { name: "Refresh diagnostics" }),
  );
  expect(diagnostics).toHaveBeenCalledTimes(2);
});

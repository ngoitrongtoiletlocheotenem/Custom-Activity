import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import App from "./App";
it("does not present fabricated processes or enable sharing in a browser", () => {
  render(<App />);
  expect(screen.getByText("Desktop app required")).toBeInTheDocument();
  expect(
    screen.getByRole("switch", { name: "Share my current activity" }),
  ).toBeDisabled();
  expect(screen.getByRole("button", { name: "Application" })).toBeDisabled();
  expect(screen.queryByText("Visual Studio Code")).not.toBeInTheDocument();
});
it("allows keyboard access to preferences while browser actions stay disabled", async () => {
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: "Open settings" }));
  expect(screen.getByText("Make yourself at home.")).toBeInTheDocument();
  expect(
    screen.getByRole("switch", { name: /Automatically reconnect/ }),
  ).toBeChecked();
  expect(screen.getByRole("switch", { name: /Auto-resume/ })).not.toBeChecked();
  expect(
    screen.getByRole("button", { name: "Save preferences" }),
  ).toBeDisabled();
});

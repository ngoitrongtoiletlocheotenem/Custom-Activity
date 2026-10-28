import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import AppPicker from "./AppPicker";

it("filters executable names and selects the remaining application by keyboard", async () => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  const select = vi.fn();
  const user = userEvent.setup();
  render(
    <AppPicker
      apps={[
        {
          id: "a",
          name: "Editor",
          executableName: "code.exe",
          executablePath: "a",
          processIds: [1],
          running: true,
        },
        {
          id: "b",
          name: "Browser",
          executableName: "chrome.exe",
          executablePath: "b",
          processIds: [2],
          running: true,
        },
      ]}
      busy={false}
      refresh={vi.fn()}
      select={select}
      close={vi.fn()}
    />,
  );
  const search = screen.getByRole("textbox", { name: "Search applications" });
  await user.click(search);
  await user.type(search, "code.exe");
  expect(
    screen.queryByRole("button", { name: /Browser/ }),
  ).not.toBeInTheDocument();
  await user.tab(); // Refresh
  await user.tab(); // Matching app
  expect(screen.getByRole("button", { name: /Editor/ })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(select).toHaveBeenCalledWith(expect.objectContaining({ id: "a" }));
});

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
const configure = vi.hoisted(() => vi.fn());
vi.mock("../api", () => ({ api: { configureIconUpload: configure } }));
import IconHosting from "./IconHosting";
const empty = {
  enabled: false,
  consentGranted: false,
  endpoint: "",
  publicBase: "",
  credentialStored: false,
};
beforeEach(() => {
  configure.mockReset();
});
it("requires explicit consent and clears the password after narrow native configuration", async () => {
  const user = userEvent.setup();
  const onConfigured = vi.fn();
  configure.mockResolvedValue({
    iconUpload: {
      enabled: true,
      consentGranted: true,
      endpoint: "https://icons.example.com/upload",
      publicBase: "https://images.example.com/",
      credentialStored: true,
    },
  });
  render(
    <IconHosting
      settings={empty}
      onConfigured={onConfigured}
      disabled={false}
    />,
  );
  await user.click(screen.getByText("Icon hosting · Cloudflare R2"));
  expect(
    screen.getByRole("switch", { name: /Use my icon host/ }),
  ).not.toBeChecked();
  await user.click(screen.getByRole("switch", { name: /Use my icon host/ }));
  await user.type(
    screen.getByLabelText("Worker upload endpoint"),
    "https://icons.example.com/upload",
  );
  await user.type(
    screen.getByLabelText("Public image base URL"),
    "https://images.example.com/",
  );
  await user.type(screen.getByLabelText("Upload token"), "test-only-secret-at-least-32-characters");
  expect(
    screen.getByRole("button", { name: "Save icon hosting" }),
  ).toBeDisabled();
  await user.click(
    screen.getByRole("checkbox", { name: /Upload selected application/ }),
  );
  await user.click(screen.getByRole("button", { name: "Save icon hosting" }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith({
      enabled: true,
      consent: true,
      endpoint: "https://icons.example.com/upload",
      publicBase: "https://images.example.com/",
      token: "test-only-secret-at-least-32-characters",
    }),
  );
  expect(screen.getByLabelText("Upload token")).toHaveValue("");
  expect(screen.queryByText("test-only-secret-at-least-32-characters")).not.toBeInTheDocument();
  expect(onConfigured).toHaveBeenCalledOnce();
});
it("retains a saved credential without echoing it but requires a new token for a changed endpoint", async () => {
  const user = userEvent.setup();
  const settings = {
    ...empty,
    enabled: true,
    consentGranted: true,
    endpoint: "https://old.example.com/upload",
    publicBase: "https://images.example.com/",
    credentialStored: true,
  };
  configure.mockResolvedValue({ iconUpload: settings });
  render(
    <IconHosting settings={settings} onConfigured={vi.fn()} disabled={false} />,
  );
  await user.click(screen.getByText("Icon hosting · Cloudflare R2"));
  expect(screen.getByLabelText("Upload token")).toHaveValue("");
  await user.click(screen.getByRole("button", { name: "Save icon hosting" }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      expect.objectContaining({ token: null }),
    ),
  );
  await user.clear(screen.getByLabelText("Worker upload endpoint"));
  await user.type(
    screen.getByLabelText("Worker upload endpoint"),
    "https://new.example.com/upload",
  );
  expect(
    screen.getByRole("button", { name: "Save icon hosting" }),
  ).toBeDisabled();
});
it("clears the token and hides raw backend errors after failed configuration", async () => {
  const user = userEvent.setup();
  configure.mockRejectedValue(new Error("sensitive-backend-content"));
  render(
    <IconHosting settings={empty} onConfigured={vi.fn()} disabled={false} />,
  );
  await user.click(screen.getByText("Icon hosting · Cloudflare R2"));
  await user.type(screen.getByLabelText("Upload token"), "test-only-secret-at-least-32-characters");
  await user.click(screen.getByRole("button", { name: "Save icon hosting" }));
  await screen.findByText(/Could not save icon hosting/);
  expect(screen.getByLabelText("Upload token")).toHaveValue("");
  expect(
    screen.queryByText("sensitive-backend-content"),
  ).not.toBeInTheDocument();
});


import { describe, it, expect } from "vitest";
import { applicationNameError, descriptionError } from "./api";
describe("Discord description validation", () => {
  it("allows a one-character custom application name but enforces UTF-8 bytes and controls", () => {
    expect(applicationNameError("")).toBeNull();
    expect(applicationNameError("a")).toBeNull();
    expect(applicationNameError("😀")).toBeNull();
    expect(applicationNameError("😀".repeat(33))).toContain("128");
    expect(applicationNameError("a\nb")).toContain("control");
  });
  it("rejects a single Unicode scalar and control characters", () => {
    expect(descriptionError("😀")).toContain("2 characters");
    expect(descriptionError("hello\nworld")).toContain("control");
  });
  it("allows an omitted details field", () =>
    expect(descriptionError("")).toBeNull());
  it("rejects a one character Discord field", () =>
    expect(descriptionError("a")).toContain("2 characters"));
  it("measures UTF-8 bytes rather than JavaScript code units", () => {
    expect(descriptionError("😀".repeat(32))).toBeNull();
    expect(descriptionError("😀".repeat(33))).toContain("128");
  });
  it("allows the exact ASCII boundary", () => {
    expect(descriptionError("a".repeat(128))).toBeNull();
    expect(descriptionError("a".repeat(129))).toContain("128");
  });
});

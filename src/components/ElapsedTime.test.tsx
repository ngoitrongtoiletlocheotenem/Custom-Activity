import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ElapsedTime from "./ElapsedTime";
afterEach(() => {
  vi.useRealTimers();
});
it("advances from the backend timestamp across rerenders and removes a reset timer", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-01-01T00:02:05Z"));
  const start = Date.parse("2026-01-01T00:00:00Z") / 1000;
  const { rerender } = render(<ElapsedTime startedAt={start} />);
  expect(screen.getByLabelText("Elapsed activity time")).toHaveTextContent(
    "2:05 elapsed",
  );
  act(() => {
    vi.advanceTimersByTime(5000);
  });
  rerender(<ElapsedTime startedAt={start} />);
  expect(screen.getByLabelText("Elapsed activity time")).toHaveTextContent(
    "2:10 elapsed",
  );
  rerender(<ElapsedTime startedAt={null} />);
  expect(
    screen.queryByLabelText("Elapsed activity time"),
  ).not.toBeInTheDocument();
});

import { useEffect, useState } from "react";

export default function ElapsedTime({
  startedAt,
}: {
  startedAt?: number | null;
}) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (startedAt == null) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [startedAt]);
  if (startedAt == null || !Number.isFinite(startedAt)) return null;
  const seconds = Math.max(0, Math.floor(now / 1000 - startedAt));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds / 60) % 60;
  const text =
    hours > 0
      ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`
      : `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
  return (
    <span className="preview-source" aria-label="Elapsed activity time">
      {text} elapsed
    </span>
  );
}

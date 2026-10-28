import { useEffect, useRef, useState } from "react";
import { Search, X, RefreshCw } from "lucide-react";
import type { DetectedApplication } from "../types/application";
import AppIcon from "./AppIcon";
export default function AppPicker({
  apps,
  busy,
  refresh,
  select,
  close,
}: {
  apps: DetectedApplication[];
  busy: boolean;
  refresh: () => void;
  select: (app: DetectedApplication) => void;
  close: () => void;
}) {
  const [query, setQuery] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
    const el = dialog.current;
    return () => el?.close();
  }, []);
  const filtered = apps.filter((a) =>
    `${a.name} ${a.executableName}`.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <dialog
      ref={dialog}
      onCancel={close}
      className="picker"
      aria-labelledby="picker-title"
    >
      <div className="dialog-header">
        <h2 id="picker-title">Choose an application</h2>
        <button
          className="icon-button"
          aria-label="Close application picker"
          onClick={close}
        >
          <X size={18} />
        </button>
      </div>
      <p className="muted">
        Only applications with visible windows appear here.
      </p>
      <div className="search">
        <Search size={17} />
        <input
          autoFocus
          aria-label="Search applications"
          placeholder="Search applications…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button
          className="icon-button"
          aria-label="Refresh applications"
          disabled={busy}
          onClick={refresh}
        >
          <RefreshCw size={16} className={busy ? "spin" : ""} />
        </button>
      </div>
      <div className="app-list">
        {filtered.map((app) => (
          <button
            className="app-option"
            key={app.id}
            onClick={() => select(app)}
          >
            <AppIcon app={app} />
            <span>
              <strong>{app.name}</strong>
              <small>{app.executableName}</small>
            </span>
          </button>
        ))}
        {filtered.length === 0 && (
          <p className="empty">
            {busy
              ? "Finding open applications…"
              : query
                ? "No matching applications."
                : "No applications found. Open a program and refresh."}
          </p>
        )}
      </div>
      <div className="dialog-footer">
        Window titles and contents are never read.
      </div>
    </dialog>
  );
}

import { useCallback, useEffect, useState } from "react";
import { api, desktop } from "../api";
import type { IconDiagnostics as Diagnostics } from "../types/application";

export default function IconDiagnostics() {
  const [data, setData] = useState<Diagnostics | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (!desktop) return;
    let alive = true;
    api
      .iconDiagnostics()
      .then((value) => {
        if (alive) setData(value);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  const refresh = useCallback(async () => {
    setBusy(true);
    setFailed(false);
    try {
      setData(await api.iconDiagnostics());
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }, []);
  if (!data) return null; // The debug-only command is unavailable in release builds.
  const display = (value: unknown) =>
    value === true
      ? "Yes"
      : value === false
        ? "No"
        : typeof value === "string" && value
          ? value
          : "Not available";
  const rows: [string, unknown][] = [
    ["Application", data.selectedApplication?.name],
    ["Executable", data.selectedApplication?.executableName],
    ["Local icon available", data.localIcon?.available],
    ["Local icon cache", data.localIcon?.cachePath],
    ["Discord icon source", data.resolvedIcon?.source],
    ["Resolved Discord image", data.resolvedIcon?.discordImage],
    ["SDK assets created", data.native?.assetsCreated],
    ["SDK SetAssets called", data.native?.setAssetsCalled],
    ["SDK large image", data.native?.largeImage],
    ["SDK large image text", data.native?.largeText],
    ["SDK callback received", data.native?.callbackReceived],
    ["SDK callback successful", data.native?.callbackSuccessful],
  ];
  return (
    <details
      style={{ marginTop: 24, borderTop: "1px solid #37313f", paddingTop: 16 }}
    >
      <summary style={{ cursor: "pointer", fontSize: 12, color: "#b7a2f7" }}>
        Developer icon diagnostics
      </summary>
      <p className="hint">
        The executable icon is local. Discord uses the resolved image below; a
        successful SDK callback does not verify image rendering.
      </p>
      <dl>
        {rows.map(([label, value]) => (
          <div
            key={label}
            style={{
              display: "grid",
              gridTemplateColumns: "45% 1fr",
              gap: 12,
              fontSize: 10,
              lineHeight: 1.6,
              padding: "5px 0",
            }}
          >
            <dt className="muted" style={{ fontSize: 10 }}>
              {label}
            </dt>
            <dd style={{ margin: 0, overflowWrap: "anywhere" }}>
              {display(value)}
            </dd>
          </div>
        ))}
      </dl>
      <button
        className="text-button"
        disabled={busy}
        onClick={() => void refresh()}
      >
        {busy ? "Refreshing…" : "Refresh diagnostics"}
      </button>
      {failed && (
        <p className="hint" role="status">
          Diagnostics could not be refreshed.
        </p>
      )}
    </details>
  );
}

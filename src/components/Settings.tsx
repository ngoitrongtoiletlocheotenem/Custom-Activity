import { useState } from "react";
import { ArrowLeft, Check } from "lucide-react";
import type { Settings as SettingsType } from "../types/application";
import IconDiagnostics from "./IconDiagnostics";
import IconHosting from "./IconHosting";
export default function Settings({
  settings,
  save,
  back,
  busy,
  onIconUploadConfigured,
}: {
  settings: SettingsType;
  save: (settings: SettingsType) => Promise<boolean>;
  back: () => void;
  busy: boolean;
  onIconUploadConfigured: (settings: SettingsType) => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [saved, setSaved] = useState(false);
  const rows: [keyof SettingsType, string, string][] = [
    [
      "startWithWindows",
      "Start with Windows",
      "Open CurrentActivity when you sign in.",
    ],
    [
      "startMinimized",
      "Start minimized",
      "Keep the main window in the system tray.",
    ],
    [
      "automaticallyReconnect",
      "Automatically reconnect",
      "Restore the connection when Discord is available.",
    ],
    [
      "autoResume",
      "Auto-resume selected application",
      "Share again when the same application restarts.",
    ],
    [
      "rememberDescriptions",
      "Remember descriptions",
      "Save your descriptions per application, on this device.",
    ],
  ];
  return (
    <section className="settings">
      <button className="text-button" onClick={back}>
        <ArrowLeft size={15} /> Back to activity
      </button>
      <h2>Make yourself at home.</h2>
      <p className="muted">Your preferences stay on this device.</p>
      <div className="setting-list">
        {rows.map(([key, label, hint]) => (
          <label className="setting-row" key={key}>
            <span>
              <strong>{label}</strong>
              <small>{hint}</small>
            </span>
            <input
              type="checkbox"
              role="switch"
              checked={Boolean(draft[key])}
              onChange={(e) => {
                setSaved(false);
                setDraft({ ...draft, [key]: e.target.checked });
              }}
            />
          </label>
        ))}
      </div>
      <label className="field-label" htmlFor="client-id">
        Discord application ID
      </label>
      <input
        id="client-id"
        className="text-input"
        inputMode="numeric"
        value={draft.discordClientId}
        placeholder="Use the build-configured application ID"
        onChange={(e) => {
          setSaved(false);
          setDraft({ ...draft, discordClientId: e.target.value });
        }}
      />
      <p className="hint">
        Optional override for your Discord Developer Application. This ID is
        public; no token is needed.
      </p>
      <button
        className="primary"
        disabled={
          busy ||
          Boolean(
            draft.discordClientId && !/^\d{17,20}$/.test(draft.discordClientId),
          )
        }
        onClick={async () => {
          setSaved(await save(draft));
        }}
      >
        {saved ? (
          <>
            <Check size={16} /> Saved
          </>
        ) : (
          "Save preferences"
        )}
      </button>
      <IconDiagnostics />
      <IconHosting
        settings={draft.iconUpload}
        disabled={busy}
        onConfigured={(current) => {
          setDraft((previous) => ({
            ...previous,
            iconUpload: current.iconUpload,
          }));
          onIconUploadConfigured(current);
        }}
      />
    </section>
  );
}

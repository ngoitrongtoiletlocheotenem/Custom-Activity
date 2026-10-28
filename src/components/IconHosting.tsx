import { useState } from "react";
import { api } from "../api";
import type { IconUploadSettings, Settings } from "../types/application";
const empty: IconUploadSettings = {
  enabled: false,
  consentGranted: false,
  endpoint: "",
  publicBase: "",
  credentialStored: false,
};
function https(value: string) {
  try {
    const url = new URL(value);
    return (
      url.protocol === "https:" && !url.username && !url.password && !url.hash
    );
  } catch {
    return false;
  }
}
export default function IconHosting({
  settings,
  onConfigured,
  disabled,
}: {
  settings: IconUploadSettings | undefined;
  onConfigured: (settings: Settings) => void;
  disabled: boolean;
}) {
  const current = settings ?? empty;
  const [enabled, setEnabled] = useState(current.enabled),
    [consent, setConsent] = useState(current.consentGranted),
    [endpoint, setEndpoint] = useState(current.endpoint),
    [publicBase, setPublicBase] = useState(current.publicBase),
    [token, setToken] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const endpointChanged = endpoint.trim().replace(/\/+$/, "") !== current.endpoint.replace(/\/+$/, "");
  const needsToken = endpointChanged || !current.credentialStored;
  const valid =
    !enabled ||
    (consent &&
      https(endpoint.trim()) &&
      https(publicBase.trim()) &&
      (!needsToken || token.length >= 32) &&
      (token.length === 0 || (token.length >= 32 && token.length <= 1024)));
  return (
    <details
      style={{ marginTop: 24, borderTop: "1px solid #37313f", paddingTop: 16 }}
    >
      <summary style={{ cursor: "pointer", fontSize: 12, color: "#b7a2f7" }}>
        Icon hosting · Cloudflare R2
      </summary>
      <p className="hint">
        Optional. Upload selected executable icons to your own configured host
        so Discord can display them. Uploaded PNGs are public and remain on your
        host until you remove them.
      </p>
      <p className="hint">An R2 public URL serves images but cannot accept uploads. Configure an authenticated Worker upload endpoint and a token of at least 32 characters before enabling.</p>
      <label className="setting-row">
        <span>
          <strong>Use my icon host</strong>
          <small>
            Off by default. No icons upload until you consent and save.
          </small>
        </span>
        <input
          type="checkbox"
          role="switch"
          checked={enabled}
          disabled={disabled || busy}
          onChange={(e) => {
            setEnabled(e.target.checked);
            setMessage("");
          }}
        />
      </label>
      <label
        className="field-label"
        htmlFor="icon-upload-endpoint"
        style={{ marginTop: 16 }}
      >
        Worker upload endpoint
      </label>
      <input
        id="icon-upload-endpoint"
        className="text-input"
        type="url"
        value={endpoint}
        placeholder="https://your-worker.example.workers.dev"
        disabled={disabled || busy}
        onChange={(e) => {
          setEndpoint(e.target.value);
          setMessage("");
        }}
      />
      <p className="hint">Use the deployed Worker’s base URL, without /v1/icons.</p>
      <label
        className="field-label"
        htmlFor="icon-public-base"
        style={{ marginTop: 16 }}
      >
        Public image base URL
      </label>
      <input
        id="icon-public-base"
        className="text-input"
        type="url"
        value={publicBase}
        placeholder="https://images.example.com/"
        disabled={disabled || busy}
        onChange={(e) => {
          setPublicBase(e.target.value);
          setMessage("");
        }}
      />
      <label
        className="field-label"
        htmlFor="icon-upload-token"
        style={{ marginTop: 16 }}
      >
        Upload token
      </label>
      <input
        id="icon-upload-token"
        className="text-input"
        type="password"
        maxLength={1024}
        autoComplete="new-password"
        spellCheck={false}
        value={token}
        placeholder={
          current.credentialStored && !endpointChanged
            ? "Saved securely; leave blank to keep"
            : "Enter your Worker upload token"
        }
        disabled={disabled || busy}
        onChange={(e) => {
          setToken(e.target.value);
          setMessage("");
        }}
      />
      <p className="hint">
        Stored with Windows DPAPI on this device. Changing the endpoint requires
        a new token. Tokens are never included in normal preferences.
      </p>
      <label
        style={{
          display: "flex",
          gap: 9,
          fontSize: 11,
          lineHeight: 1.7,
          margin: "16px 0",
        }}
      >
        <input
          type="checkbox"
          checked={consent}
          disabled={disabled || busy}
          onChange={(e) => {
            setConsent(e.target.checked);
            setMessage("");
          }}
        />
        <span>
          Upload selected application’s extracted icon to my configured host.
        </span>
      </label>
      {enabled && !valid && (
        <p className="hint">
          Enter HTTPS URLs and a token, and give explicit upload consent before
          enabling.
        </p>
      )}
      <button
        className="primary"
        style={{ marginTop: 8 }}
        disabled={disabled || busy || !valid}
        onClick={async () => {
          setBusy(true);
          setMessage("");
          try {
            const result = await api.configureIconUpload({
              enabled,
              consent: enabled && consent,
              endpoint: endpoint.trim(),
              publicBase: publicBase.trim(),
              token: token || null,
            });
            onConfigured(result);
            setEnabled(result.iconUpload.enabled);
            setConsent(result.iconUpload.consentGranted);
            setEndpoint(result.iconUpload.endpoint);
            setPublicBase(result.iconUpload.publicBase);
            setMessage("Icon hosting preferences saved.");
          } catch {
            setMessage(
              "Could not save icon hosting. Check the HTTPS endpoints, consent and token.",
            );
          } finally {
            setToken("");
            setBusy(false);
          }
        }}
      >
        {busy ? "Saving…" : "Save icon hosting"}
      </button>
      {message && (
        <p className="hint" role="status">
          {message}
        </p>
      )}
    </details>
  );
}

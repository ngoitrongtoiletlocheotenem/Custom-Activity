import { useCallback, useEffect, useRef, useState } from "react";
import {
  Activity,
  ArrowUpRight,
  ChevronDown,
  Focus,
  Info,
  Radio,
  RefreshCw,
  Settings as SettingsIcon,
  ShieldCheck,
  X,
} from "lucide-react";
import {
  api,
  defaultSettings,
  descriptionError,
  desktop,
  emptyActivity,
} from "./api";
import type {
  ActivityState,
  DetectedApplication,
  Settings as SettingsType,
} from "./types/application";
import AppPicker from "./components/AppPicker";
import AppIcon from "./components/AppIcon";
import Settings from "./components/Settings";
import Onboarding from "./components/Onboarding";

export default function App() {
  const [state, setState] = useState(emptyActivity),
    [settings, setSettings] = useState(defaultSettings),
    [apps, setApps] = useState<DetectedApplication[]>([]),
    [draft, setDraft] = useState(""),
    [picker, setPicker] = useState(false),
    [page, setPage] = useState<"activity" | "settings">("activity"),
    [busy, setBusy] = useState(false),
    [scanning, setScanning] = useState(false),
    [error, setError] = useState(""),
    [loaded, setLoaded] = useState(!desktop);
  const dirty = useRef(false);
  const selected = state.selectedApplication;
  const apply = useCallback((next: ActivityState, reset = false) => {
    setState(next);
    if (reset || !dirty.current) {
      setDraft(next.description);
      dirty.current = false;
    }
  }, []);
  const run = useCallback(async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await action();
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setBusy(false);
    }
  }, []);
  const refresh = useCallback(async () => {
    if (!desktop) return;
    setScanning(true);
    try {
      setApps(await api.applications());
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }, []);
  useEffect(() => {
    if (!desktop) return;
    let alive = true;
    const unlisten: (() => void)[] = [];
    Promise.all([api.activity(), api.settings()])
      .then(([a, s]) => {
        if (alive) {
          apply(a, true);
          setSettings(s);
          setLoaded(true);
        }
      })
      .catch((e) => {
        if (alive) {
          setError(String(e));
          setLoaded(true);
        }
      });
    const subscriptions = [
      api.on<ActivityState>("presence-updated", (e) => {
        if (alive) apply(e.payload);
      }),
      api.on<ActivityState>("application-stopped", (e) => {
        if (alive) apply(e.payload);
      }),
      api.on<ActivityState["discordState"]>("discord-state-changed", (e) => {
        if (alive) setState((s) => ({ ...s, discordState: e.payload }));
      }),
      api.on<SettingsType>("settings-changed", (e) => {
        if (alive) setSettings(e.payload);
      }),
    ];
    subscriptions.forEach((p) =>
      p
        .then((off) => {
          if (alive) unlisten.push(off);
          else off();
        })
        .catch((e) => {
          if (alive) setError(String(e));
        }),
    );
    void refresh();
    return () => {
      alive = false;
      unlisten.forEach((off) => off());
    };
  }, [apply, refresh]);
  const validation = descriptionError(draft);
  const update = useCallback(() => {
    if (!selected || validation || busy || !desktop) return;
    void run(async () => {
      apply(await api.publish(selected.id, draft), true);
    });
  }, [selected, validation, busy, draft, run, apply]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "Enter" && page === "activity" && !picker) {
        e.preventDefault();
        update();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [update, page, picker]);
  async function select(app: DetectedApplication) {
    setPicker(false);
    await run(async () => {
      apply(await api.select(app.id), true);
    });
  }
  const connectionLabels = {
    disabled: "Activity disabled",
    waiting: "Waiting for Discord",
    connected: "Connected to Discord",
    error: "Discord connection error",
  };
  const canPublish =
    desktop &&
    !!selected &&
    state.selectedApplicationRunning &&
    !validation &&
    !busy;
  return (
    <div className="shell">
      <header className="topbar">
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setPage("activity");
          }}
        >
          <span className="brand-icon">
            <Activity size={20} />
          </span>
          CurrentActivity<span className="version">BETA</span>
        </a>
        <button
          className={"icon-button " + (page === "settings" ? "active" : "")}
          title="Preferences"
          aria-label="Open settings"
          onClick={() => setPage(page === "settings" ? "activity" : "settings")}
        >
          <SettingsIcon size={19} />
        </button>
      </header>
      <main>
        {!desktop && (
          <div className="notice">
            <Info size={17} />
            <span>
              <strong>Desktop app required</strong> This browser preview cannot
              detect programs or connect to Discord. Run the Windows app to get
              started.
            </span>
          </div>
        )}
        {state.discordState.status === "error" && (
          <div className="error" role="alert">
            <Info size={17} />
            <span>{state.discordState.message}</span>
          </div>
        )}
        {error && (
          <div className="error" role="alert">
            <span>{error}</span>
            <button
              className="icon-button"
              aria-label="Dismiss error"
              onClick={() => setError("")}
            >
              <X size={16} />
            </button>
          </div>
        )}
        {page === "settings" ? (
          <Settings
            key={String(settings.onboardingComplete)}
            settings={settings}
            onIconUploadConfigured={setSettings}
            busy={busy || !desktop}
            back={() => setPage("activity")}
            save={async (next) =>
              await run(async () => setSettings(await api.saveSettings(next)))
            }
          />
        ) : (
          <>
            <div className="intro">
              <span className="eyebrow">
                A LITTLE CONTEXT. ENTIRELY YOUR CHOICE.
              </span>
              <h1>
                Let them know
                <br />
                what you’re up to<span>.</span>
              </h1>
              <p>One application. Your own words. Shared on Discord.</p>
            </div>
            <section className="sharing-card">
              <div className="sharing-copy">
                <span className="sharing-symbol">
                  <Radio size={20} />
                </span>
                <div>
                  <h2>Share my current activity</h2>
                  <p>
                    {state.published
                      ? "Your activity is live on Discord."
                      : state.enabled
                        ? "Ready to share when Discord connects."
                        : "You decide when you’re visible."}
                  </p>
                </div>
              </div>
              <input
                aria-label="Share my current activity"
                type="checkbox"
                role="switch"
                checked={state.enabled}
                disabled={busy || !desktop || (!state.enabled && !canPublish)}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  void run(async () => {
                    if (enabled && selected) {
                      apply(await api.publish(selected.id, draft), true);
                    }
                    apply(await api.enable(enabled));
                  });
                }}
              />
            </section>
            <section className="editor" aria-label="Activity details">
              <div className="label-row">
                <label className="field-label" id="application-label">
                  Application
                </label>
                <button
                  className="text-button"
                  disabled={!desktop || busy}
                  onClick={() =>
                    void run(async () => {
                      const app = await api.foreground();
                      if (app) apply(await api.select(app.id), true);
                      else
                        setError(
                          "No eligible foreground application found. Switch to a program, then try again.",
                        );
                    })
                  }
                >
                  <Focus size={14} />
                  Use active window
                </button>
              </div>
              <button
                className="application-select"
                aria-labelledby="application-label"
                aria-haspopup="dialog"
                disabled={!desktop || busy}
                onClick={() => {
                  setPicker(true);
                  void refresh();
                }}
              >
                <AppIcon key={selected?.id} app={selected} />
                <span>
                  <strong>{selected?.name || "Choose an application"}</strong>
                  <small>
                    {selected
                      ? selected.executableName
                      : "Select from your open programs"}
                  </small>
                </span>
                <ChevronDown size={18} />
              </button>
              <div className="application-meta">
                {selected ? (
                  <span
                    className={
                      state.selectedApplicationRunning ? "running" : "stopped"
                    }
                  >
                    <i />
                    {state.selectedApplicationRunning
                      ? "Running"
                      : "Selected application is no longer running."}
                  </span>
                ) : (
                  <span>Select a program to start sharing.</span>
                )}
                <button
                  className="icon-button"
                  aria-label="Refresh running applications"
                  title="Refresh running applications"
                  disabled={!desktop || scanning}
                  onClick={() => void refresh()}
                >
                  <RefreshCw size={13} className={scanning ? "spin" : ""} />
                </button>
              </div>
              <label
                className="field-label description-label"
                htmlFor="description"
              >
                Description <span>Optional</span>
              </label>
              <textarea
                id="description"
                rows={2}
                placeholder="What are you working on?"
                value={draft}
                aria-invalid={!!validation}
                aria-describedby="description-hint"
                disabled={!desktop || !selected}
                onChange={(e) => {
                  dirty.current = true;
                  setDraft(e.target.value);
                }}
              />
              <div className="input-meta">
                <span
                  id="description-hint"
                  className={validation ? "invalid" : ""}
                >
                  {validation || "Only the words you type will be shared."}
                </span>
                <span>{new TextEncoder().encode(draft).length}/128</span>
              </div>
            </section>
            <section className="preview-section">
              <div className="label-row">
                <span className="field-label">Discord preview</span>
                <span className="preview-badge">PREVIEW</span>
              </div>
              <div className="discord-preview">
                <span className="generic-icon">
                  <AppIcon key={selected?.id} app={selected} />
                </span>
                <div>
                  <span className="playing">PLAYING A GAME</span>
                  <strong>
                    {selected?.name || "Your selected application"}
                  </strong>
                  <p>{draft || "Your description will appear here"}</p>
                  <span className="preview-source">Local icon preview</span>
                </div>
              </div>
              <p className="hint preview-note">
                Actual appearance may vary in Discord. {settings.iconUpload?.enabled
                  ? "Your selected application's icon is uploaded to your configured host."
                  : "Discord uses a configured public image or asset. Executable icons stay local while uploads are off."}
              </p>
            </section>
            {state.resumeRequired && (
              <div className="notice">
                <Info size={16} />
                <span>
                  The application stopped. Click Update Presence or toggle
                  sharing to resume when it is running.
                </span>
              </div>
            )}
            <div className="actions">
              <span
                role="status"
                aria-live="polite"
                className={"connection " + state.discordState.status}
                title={state.discordState.message}
              >
                <i />
                {connectionLabels[state.discordState.status]}
              </span>
              <button
                className="primary"
                disabled={!canPublish}
                onClick={update}
              >
                {busy ? (
                  "Updating…"
                ) : (
                  <>
                    <ArrowUpRight size={17} />
                    Update Presence
                  </>
                )}
                <kbd>Ctrl ↵</kbd>
              </button>
            </div>
            <p className="applied-note">
              {state.enabled
                ? state.published
                  ? "Applied activity is live. Editing stays local until you update."
                  : "Updates are saved while waiting for a Discord connection."
                : "Sharing is off. Saving your activity does not enable sharing."}
            </p>
          </>
        )}
      </main>
      <footer>
        <span>
          <ShieldCheck size={14} />
          Private by design
        </span>
        <span>No telemetry. No tracking.</span>
      </footer>
      {picker && (
        <AppPicker
          apps={apps}
          busy={scanning}
          refresh={() => void refresh()}
          select={(app) => void select(app)}
          close={() => setPicker(false)}
        />
      )}
      {desktop && loaded && !settings.onboardingComplete && (
        <Onboarding
          busy={busy}
          complete={() =>
            void run(async () => setSettings(await api.onboard()))
          }
        />
      )}
    </div>
  );
}

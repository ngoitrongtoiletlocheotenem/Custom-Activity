import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ActivityState,
  DetectedApplication,
  Settings,
  IconDiagnostics,
  ConfigureIconUpload,
} from "./types/application";
export const desktop = isTauri();
export const defaultSettings: Settings = {
  iconUpload: {
    enabled: false,
    consentGranted: false,
    endpoint: "",
    publicBase: "",
    credentialStored: false,
  },
  startWithWindows: false,
  startMinimized: false,
  automaticallyReconnect: true,
  autoResume: false,
  rememberDescriptions: true,
  onboardingComplete: false,
  discordClientId: "",
};
export const emptyActivity: ActivityState = {
  enabled: false,
  selectedApplication: null,
  description: "",
  discordState: { status: "disabled", message: "Activity disabled" },
  selectedApplicationRunning: false,
  resumeRequired: false,
  published: false,
};
export const api = {
  configureIconUpload: (configuration: ConfigureIconUpload) =>
    invoke<Settings>("configure_icon_upload", { ...configuration }),
  iconDiagnostics: () => invoke<IconDiagnostics>("get_icon_diagnostics"),
  applications: () =>
    invoke<DetectedApplication[]>("list_running_applications"),
  foreground: () =>
    invoke<DetectedApplication | null>("get_foreground_application"),
  activity: () => invoke<ActivityState>("get_activity_state"),
  settings: () => invoke<Settings>("get_settings"),
  select: (appId: string) =>
    invoke<ActivityState>("select_application", { appId }),
  publish: (appId: string, description: string) =>
    invoke<ActivityState>("set_presence", { appId, description }),
  enable: (enabled: boolean) =>
    invoke<ActivityState>("set_sharing_enabled", { enabled }),
  clear: () => invoke<ActivityState>("clear_presence"),
  saveSettings: (settings: Settings) =>
    invoke<Settings>("update_settings", { settings }),
  onboard: () => invoke<Settings>("complete_onboarding"),
  on: listen,
};
export function descriptionError(value: string): string | null {
  const bytes = new TextEncoder().encode(value).length;
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(value))
    return "Use plain text without control characters or line breaks.";
  return bytes > 128
    ? "Use 128 UTF-8 bytes or fewer."
    : Array.from(value).length === 1
      ? "Use at least 2 characters, or leave this empty."
      : null;
}

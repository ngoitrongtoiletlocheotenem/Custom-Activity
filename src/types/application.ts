export interface DetectedApplication {
  id: string;
  name: string;
  executableName: string;
  executablePath: string;
  processIds: number[];
  iconDataUrl?: string | null;
  running: boolean;
}
export interface ConnectionState {
  status: "disabled" | "waiting" | "connected" | "error";
  message: string;
}
export interface ActivityState {
  applicationNameOverride?: string;
  details?: string;
  startedAt?: number | null;
  enabled: boolean;
  selectedApplication: DetectedApplication | null;
  description: string;
  discordState: ConnectionState;
  selectedApplicationRunning: boolean;
  resumeRequired: boolean;
  published: boolean;
}
export interface Settings {
  iconUpload: IconUploadSettings;
  startWithWindows: boolean;
  startMinimized: boolean;
  automaticallyReconnect: boolean;
  autoResume: boolean;
  rememberDescriptions: boolean;
  onboardingComplete: boolean;
  discordClientId: string;
}
export interface IconUploadSettings {
  enabled: boolean;
  consentGranted: boolean;
  endpoint: string;
  publicBase: string;
  credentialStored: boolean;
}
export interface ConfigureIconUpload {
  enabled: boolean;
  consent: boolean;
  endpoint: string;
  publicBase: string;
  token: string | null;
}
export interface IconDiagnostics {
  selectedApplication: { name: string; executableName: string } | null;
  localIcon: { available: boolean; cachePath?: string | null };
  resolvedIcon: { discordImage?: string | null; source: string } | null;
  native: {
    assetsCreated?: boolean;
    setAssetsCalled?: boolean;
    largeImage?: string | null;
    largeText?: string | null;
    callbackReceived?: boolean;
    callbackSuccessful?: boolean;
    [key: string]: unknown;
  } | null;
}

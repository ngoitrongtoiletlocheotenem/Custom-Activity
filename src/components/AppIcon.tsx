import { AppWindow } from "lucide-react";
import { useState } from "react";
import type { DetectedApplication } from "../types/application";
export default function AppIcon({ app }: { app?: DetectedApplication | null }) {
  const [failed, setFailed] = useState(false);
  return (
    <span className="app-icon">
      {app?.iconDataUrl && !failed ? (
        <img src={app.iconDataUrl} alt="" onError={() => setFailed(true)} />
      ) : (
        <AppWindow size={25} />
      )}
    </span>
  );
}

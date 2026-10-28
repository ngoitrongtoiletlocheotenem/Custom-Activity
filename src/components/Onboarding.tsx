import { useEffect, useRef } from "react";
import { ArrowUpRight, Check, ShieldCheck, Sparkles } from "lucide-react";

export default function Onboarding({
  busy,
  complete,
}: {
  busy: boolean;
  complete: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
    const current = dialog.current;
    return () => current?.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="onboarding"
      aria-labelledby="welcome-title"
      onCancel={(e) => e.preventDefault()}
    >
      <span className="welcome-icon">
        <Sparkles size={27} />
      </span>
      <span className="eyebrow">WELCOME TO CURRENTACTIVITY</span>
      <h2 id="welcome-title">
        A presence.
        <br />
        With your permission.
      </h2>
      <p>Manually share one running application with your Discord friends.</p>
      <ul>
        <li>
          <Check size={16} />
          Share the application name you select and description you submit.
        </li>
        <li>
          <Check size={16} />
          Uploading an application icon requires separate permission in Settings.
        </li>
        <li>
          <ShieldCheck size={16} />
          Window titles, filenames, browser tabs and folders stay private.
        </li>
      </ul>
      <button autoFocus className="primary" disabled={busy} onClick={complete}>
        Continue <ArrowUpRight size={17} />
      </button>
      <small>Sharing starts off. You’re always in control.</small>
    </dialog>
  );
}

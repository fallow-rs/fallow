import { useEffect, useRef, useState } from "react";
import { DrawableImage } from "./DrawableImage";
import { theme } from "../theme";

/** Minimal slice of the Electron <webview> element we drive imperatively. */
type WebviewEl = HTMLElement & {
  src: string;
  loadURL: (url: string) => Promise<void>;
  capturePage: () => Promise<{ toDataURL: () => string }>;
};

/**
 * Live, interactive embed of the app-under-review (Electron <webview>). The
 * picker runs inside it (dev) and posts to the bridge; "Annotate view" captures
 * the CURRENT interacted state for drawing (Tier-2 live annotation).
 */
export const LiveApp = () => {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const webviewRef = useRef<WebviewEl | null>(null);
  const [url, setUrl] = useState("http://localhost:5273");
  const [shot, setShot] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const wv = document.createElement("webview") as WebviewEl;
    wv.src = url;
    Object.assign(wv.style, { width: "100%", height: "100%", border: "none" });
    host.appendChild(wv);
    webviewRef.current = wv;
    return () => {
      wv.remove();
      webviewRef.current = null;
    };
    // Create the webview once; subsequent navigation uses loadURL via "Go".
  }, []);

  const go = (): void => {
    const wv = webviewRef.current;
    if (wv) void wv.loadURL(url).catch(() => undefined);
  };

  const annotate = async (): Promise<void> => {
    const wv = webviewRef.current;
    if (!wv) return;
    setStatus("capturing live view…");
    try {
      const img = await wv.capturePage();
      setShot(img.toDataURL());
      setStatus(null);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div
      style={{ display: "grid", gridTemplateRows: "auto 1fr", height: "100%", color: theme.text }}
    >
      <div
        style={{ display: "flex", gap: 6, padding: 8, borderBottom: `1px solid ${theme.border}` }}
      >
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          style={{ flex: 1, fontSize: 12 }}
        />
        <button onClick={go} style={{ fontSize: 12 }}>
          Go
        </button>
        <button onClick={() => void annotate()} style={{ fontSize: 12 }}>
          Annotate view
        </button>
      </div>
      {status && <p style={{ fontSize: 11, color: theme.muted, margin: 4 }}>{status}</p>}
      {shot ? (
        <DrawableImage dataUrl={shot} target={url} onDone={() => setShot(null)} />
      ) : (
        <div ref={hostRef} style={{ height: "100%" }} />
      )}
    </div>
  );
};

import { useEffect, useRef, useState } from "react";
import { DrawableImage } from "./DrawableImage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/** Minimal slice of the Electron <webview> element we drive imperatively. */
type WebviewEl = HTMLElement & {
  src: string;
  loadURL: (url: string) => Promise<void>;
  capturePage: () => Promise<{ toDataURL: () => string }>;
};

/**
 * Live, interactive embed of the app-under-review (Electron <webview>). The
 * picker runs inside it (dev) and posts to the bridge; "annotate view" captures
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
    wv.style.width = "100%";
    wv.style.height = "100%";
    wv.style.border = "none";
    host.append(wv);
    webviewRef.current = wv;
    return () => {
      wv.remove();
      webviewRef.current = null;
    };
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
    <div className="grid h-full grid-rows-[auto_1fr] text-foreground">
      <div className="flex gap-1.5 border-b border-border p-2">
        <Input value={url} onChange={(e) => setUrl(e.target.value)} className="h-8 text-xs" />
        <Button size="sm" variant="secondary" className="lowercase" onClick={go}>
          go
        </Button>
        <Button size="sm" className="lowercase" onClick={() => void annotate()}>
          annotate view
        </Button>
      </div>
      {status && <p className="m-1 text-[11px] text-muted-foreground">{status}</p>}
      {shot ? (
        <DrawableImage dataUrl={shot} target={url} onDone={() => setShot(null)} />
      ) : (
        <div ref={hostRef} className="h-full" />
      )}
    </div>
  );
};

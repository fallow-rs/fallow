import { useEffect, useRef, useState } from "react";
import { Camera, Loader2, RefreshCw, Unplug } from "lucide-react";
import { DrawableImage } from "./DrawableImage";
import { errorMessage } from "../lib/errors";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/** Minimal slice of the Electron <webview> element we drive imperatively. */
type WebviewEl = HTMLElement & {
  src: string;
  loadURL: (url: string) => Promise<void>;
  capturePage: () => Promise<{ toDataURL: () => string }>;
};

type Conn = "loading" | "ready" | "failed";

/**
 * Live, interactive embed of the app-under-review (Electron <webview>). The
 * picker runs inside it (dev) and posts to the bridge; "annotate view" captures
 * the CURRENT interacted state for drawing (Tier-2 live annotation). A
 * connection state machine keeps the surface dark and explains an unreachable
 * dev server instead of leaking a white webview void.
 */
export const LiveApp = () => {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const webviewRef = useRef<WebviewEl | null>(null);
  const failedRef = useRef(false);
  const [url, setUrl] = useState("http://localhost:5273");
  const [shot, setShot] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [conn, setConn] = useState<Conn>("loading");

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const wv = document.createElement("webview") as WebviewEl;
    wv.src = url;
    wv.style.width = "100%";
    wv.style.height = "100%";
    wv.style.border = "none";
    const onStart = (): void => {
      failedRef.current = false;
      setConn("loading");
    };
    const onFinish = (): void => {
      if (!failedRef.current) setConn("ready");
    };
    const onFail = (e: Event): void => {
      const ev = e as unknown as { errorCode?: number; isMainFrame?: boolean };
      if (ev.isMainFrame === false) return;
      if (ev.errorCode === -3) return; // user-aborted navigation
      failedRef.current = true;
      setConn("failed");
    };
    wv.addEventListener("did-start-loading", onStart);
    wv.addEventListener("did-finish-load", onFinish);
    wv.addEventListener("did-fail-load", onFail);
    host.append(wv);
    webviewRef.current = wv;
    return () => {
      wv.removeEventListener("did-start-loading", onStart);
      wv.removeEventListener("did-finish-load", onFinish);
      wv.removeEventListener("did-fail-load", onFail);
      wv.remove();
      webviewRef.current = null;
    };
  }, []);

  const go = (): void => {
    const wv = webviewRef.current;
    if (!wv) return;
    failedRef.current = false;
    setConn("loading");
    void wv.loadURL(url).catch(() => setConn("failed"));
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
      setStatus(errorMessage(e));
    }
  };

  return (
    <div className="grid h-full grid-rows-[auto_1fr] text-foreground">
      <div className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border px-2">
        <Input
          value={url}
          data-testid="live-url"
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") go();
          }}
          className="h-7 font-mono text-xs"
        />
        <Button
          size="sm"
          variant="secondary"
          data-testid="live-go"
          className="h-7 lowercase"
          onClick={go}
        >
          <RefreshCw className="size-3.5" />
          go
        </Button>
        <Button size="sm" className="h-7 lowercase" onClick={() => void annotate()}>
          <Camera className="size-3.5" />
          annotate
        </Button>
      </div>
      {shot ? (
        <DrawableImage dataUrl={shot} target={url} onDone={() => setShot(null)} />
      ) : (
        <div className="relative h-full bg-background">
          <div ref={hostRef} className="h-full" />
          {conn !== "ready" && (
            <div
              data-testid="live-overlay"
              data-conn={conn}
              className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-background px-6 text-center"
            >
              {conn === "loading" ? (
                <>
                  <Loader2 className="size-5 animate-spin text-muted-foreground" />
                  <p className="text-xs text-muted-foreground">
                    connecting to <span className="font-mono text-foreground">{url}</span>
                  </p>
                </>
              ) : (
                <>
                  <div className="flex size-11 items-center justify-center rounded-full border border-border bg-muted/30">
                    <Unplug className="size-5 text-muted-foreground" />
                  </div>
                  <div className="space-y-1">
                    <p className="text-sm font-medium text-foreground">can't reach the app</p>
                    <p className="text-xs text-muted-foreground">
                      nothing responded at <span className="font-mono">{url}</span>
                    </p>
                    <p className="text-[11px] text-muted-foreground">
                      start your dev server, then retry
                    </p>
                  </div>
                  <Button size="sm" variant="secondary" className="h-7 lowercase" onClick={go}>
                    <RefreshCw className="size-3.5" />
                    retry
                  </Button>
                </>
              )}
            </div>
          )}
          {status && (
            <p className="absolute inset-x-0 bottom-0 m-2 rounded-md bg-muted/60 px-2 py-1 text-center text-[11px] text-muted-foreground backdrop-blur">
              {status}
            </p>
          )}
        </div>
      )}
    </div>
  );
};

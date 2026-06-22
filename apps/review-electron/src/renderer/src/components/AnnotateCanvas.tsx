import { useEffect, useState } from "react";
import { Camera, ImageOff, Loader2, RefreshCw } from "lucide-react";
import { DrawableImage } from "./DrawableImage";
import { errorMessage } from "../lib/errors";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type Phase = "idle" | "capturing" | "error";

/** Screenshot a URL (fresh load) and annotate it. */
export const AnnotateCanvas = () => {
  const [url, setUrl] = useState("http://localhost:5273");
  const [img, setImg] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void window.fallow.getConfig().then((cfg) => setUrl(cfg.defaultUrl));
  }, []);

  const capture = async (): Promise<void> => {
    setPhase("capturing");
    setError(null);
    try {
      const shot = await window.fallow.capture(url);
      setImg(shot.dataUrl);
      setPhase("idle");
    } catch (e) {
      setError(errorMessage(e));
      setPhase("error");
    }
  };

  return (
    <div className="grid h-full grid-rows-[auto_1fr] overflow-hidden text-foreground">
      <div className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border px-2">
        <Input
          value={url}
          data-testid="shot-url"
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void capture();
          }}
          className="h-7 font-mono text-xs"
        />
        <Button
          size="sm"
          data-testid="shot-capture"
          className="h-7 lowercase"
          onClick={() => void capture()}
        >
          <Camera className="size-3.5" />
          screenshot
        </Button>
      </div>
      {img ? (
        <div className="overflow-auto p-3">
          <DrawableImage dataUrl={img} target={url} onDone={() => setImg(null)} />
        </div>
      ) : (
        <div
          data-testid="shot-overlay"
          data-phase={phase}
          className="flex flex-col items-center justify-center gap-3 bg-background px-6 text-center"
        >
          {phase === "capturing" ? (
            <>
              <Loader2 className="size-5 animate-spin text-muted-foreground" />
              <p className="text-xs text-muted-foreground">
                capturing <span className="font-mono text-foreground">{url}</span>
              </p>
            </>
          ) : phase === "error" ? (
            <>
              <div className="flex size-11 items-center justify-center rounded-full border border-border bg-muted/30">
                <ImageOff className="size-5 text-muted-foreground" />
              </div>
              <div className="space-y-1">
                <p className="text-sm font-medium text-foreground">couldn't capture</p>
                <p className="max-w-xs break-words text-xs text-muted-foreground">{error}</p>
                <p className="text-[11px] text-muted-foreground">
                  check the url is reachable, then retry
                </p>
              </div>
              <Button
                size="sm"
                variant="secondary"
                className="h-7 lowercase"
                onClick={() => void capture()}
              >
                <RefreshCw className="size-3.5" />
                retry
              </Button>
            </>
          ) : (
            <>
              <div className="flex size-11 items-center justify-center rounded-full border border-border bg-muted/30">
                <Camera className="size-5 text-muted-foreground" />
              </div>
              <div className="space-y-1">
                <p className="text-sm font-medium text-foreground">annotate a screenshot</p>
                <p className="max-w-xs text-xs text-muted-foreground">
                  capture a fresh load of the url, draw on it, and send the annotation to the agent
                </p>
              </div>
              <Button size="sm" className="h-7 lowercase" onClick={() => void capture()}>
                <Camera className="size-3.5" />
                screenshot this url
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
};

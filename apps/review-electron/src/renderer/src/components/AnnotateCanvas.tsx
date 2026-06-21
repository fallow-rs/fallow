import { useEffect, useState } from "react";
import { Camera } from "lucide-react";
import { DrawableImage } from "./DrawableImage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

/** Screenshot a URL (fresh load) and annotate it. */
export const AnnotateCanvas = () => {
  const [url, setUrl] = useState("http://localhost:5273");
  const [img, setImg] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    void window.fallow.getConfig().then((cfg) => setUrl(cfg.defaultUrl));
  }, []);

  const capture = async (): Promise<void> => {
    setStatus("capturing…");
    try {
      const shot = await window.fallow.capture(url);
      setImg(shot.dataUrl);
      setStatus(null);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="flex h-full w-full flex-col overflow-auto text-foreground">
      <div className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border px-2">
        <Input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          className="h-7 font-mono text-xs"
        />
        <Button size="sm" className="h-7 lowercase" onClick={() => void capture()}>
          <Camera className="size-3.5" />
          screenshot
        </Button>
      </div>
      <div className="p-3">
        {status && <p className="text-[11px] text-muted-foreground">{status}</p>}
        {!img && !status && (
          <p className="text-[11px] text-muted-foreground">
            screenshot a url to draw on it and send the annotation to the agent.
          </p>
        )}
        {img && <DrawableImage dataUrl={img} target={url} onDone={() => setImg(null)} />}
      </div>
    </div>
  );
};

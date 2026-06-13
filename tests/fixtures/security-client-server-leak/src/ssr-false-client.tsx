"use client";
import dynamic from "next/dynamic";

// `server-mod` imports node:fs (server-only) but is reached ONLY through a
// next/dynamic ssr:false dynamic import, the sanctioned client-only escape
// hatch. fallow's arrow-wrapped dynamic-import detection resolves
// next/dynamic(() => import('./server-mod')) to a STATIC graph edge, so the edge
// IS in the cone. The ssr:false exclusion works by SKIPPING that edge via the
// captured ssr:false import span, so no server-only finding fires.
const ServerMod = dynamic(() => import("./server-mod"), { ssr: false });

export function DynamicView() {
  return ServerMod;
}

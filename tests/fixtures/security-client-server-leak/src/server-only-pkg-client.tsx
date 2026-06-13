"use client";
import { loadServerData } from "./server-only-pkg-mod";
export function ServerDataView() {
  return loadServerData();
}

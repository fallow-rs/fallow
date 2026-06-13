"use client";
import { readSession } from "./headers-util";
export function SessionView() {
  return readSession();
}

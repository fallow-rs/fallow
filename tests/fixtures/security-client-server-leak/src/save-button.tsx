"use client";
import { save } from "./actions/save";

// Issue #2074 repro: a client component calling a Server Action. The bundler
// replaces the import with an action reference, so no server code enters the
// client bundle and no finding may be reported.
export function SaveButton() {
  return <button onClick={() => save({})}>Save</button>;
}

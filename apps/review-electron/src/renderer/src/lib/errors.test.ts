import { describe, it, expect } from "vitest";
import { errorMessage } from "./errors";

describe("errorMessage", () => {
  it("strips the Electron IPC wrapper and Error prefix", () => {
    const e = new Error(
      `Error invoking remote method 'review:get': Error: Couldn't find the "fallow" binary. Set FALLOW_BIN or add fallow to your PATH.`,
    );
    expect(errorMessage(e)).toBe(
      `Couldn't find the "fallow" binary. Set FALLOW_BIN or add fallow to your PATH.`,
    );
  });

  it("leaves an already-clean message untouched", () => {
    expect(errorMessage(new Error("Couldn't reach http://localhost:5273."))).toBe(
      "Couldn't reach http://localhost:5273.",
    );
  });

  it("handles non-Error throwables and empties", () => {
    expect(errorMessage("boom")).toBe("boom");
    expect(errorMessage(new Error("Error: "))).toBe("Something went wrong.");
  });
});

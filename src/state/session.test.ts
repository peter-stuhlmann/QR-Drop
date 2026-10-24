import { describe, expect, it } from "vitest";
import { isSettled, toneOf } from "./session";

describe("phase mapping", () => {
  it("colours end states and keeps the accent while working", () => {
    expect(toneOf("done")).toBe("success");
    expect(toneOf("received")).toBe("success");
    expect(toneOf("expired")).toBe("warning");
    expect(toneOf("error")).toBe("danger");
    for (const phase of ["preparing", "ready", "connected", "transferring", "uploading"] as const) {
      expect(toneOf(phase)).toBe("accent");
    }
  });

  it("closes instead of cancelling once nothing can be lost", () => {
    for (const phase of ["done", "received", "expired", "error"] as const) {
      expect(isSettled(phase)).toBe(true);
    }
    for (const phase of ["preparing", "ready", "connected", "transferring", "uploading"] as const) {
      expect(isSettled(phase)).toBe(false);
    }
  });
});

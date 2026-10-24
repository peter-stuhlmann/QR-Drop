import { describe, expect, it } from "vitest";
import type { ShareInfo } from "../lib/tauri";
import {
  etaSeconds,
  initialState,
  progressFraction,
  shareReducer,
  type SharePhase,
} from "./shareMachine";

const info = (patch: Partial<ShareInfo> = {}): ShareInfo => ({
  id: "abc",
  status: "ready",
  error: null,
  name: "foto.jpg",
  size: 4_200_000,
  kind: "image",
  isZip: false,
  fileCount: 1,
  content: "file",
  secret: false,
  charCount: 0,
  preview: null,
  url: "http://192.168.0.2:50000/s/x",
  qrSvg: "<svg/>",
  expiresInMs: 600_000,
  ttlMs: 600_000,
  networks: [],
  selectedIp: "192.168.0.2",
  ...patch,
});

const run = (...actions: Parameters<typeof shareReducer>[1][]): SharePhase =>
  actions.reduce(shareReducer, initialState);

describe("shareReducer", () => {
  it("follows the happy path", () => {
    let s = run({ type: "info", info: info({ status: "preparing" }) });
    expect(s.phase).toBe("preparing");
    s = shareReducer(s, { type: "info", info: info() });
    expect(s.phase).toBe("ready");
    s = shareReducer(s, { type: "connected" });
    expect(s).toEqual({ phase: "connected", interrupted: false });
    s = shareReducer(s, { type: "progress", sent: 10, total: 100, bytesPerSec: 5 });
    expect(s.phase).toBe("transferring");
    s = shareReducer(s, { type: "done" });
    expect(s.phase).toBe("done");
  });

  it("keeps done and cancelled final", () => {
    const done = run({ type: "info", info: info() }, { type: "done" });
    expect(shareReducer(done, { type: "expired" }).phase).toBe("done");
    expect(shareReducer(done, { type: "progress", sent: 1, total: 2, bytesPerSec: 1 }).phase).toBe(
      "done",
    );
    const cancelled = shareReducer(done, { type: "cancel" });
    expect(cancelled.phase).toBe("cancelled");
    expect(shareReducer(cancelled, { type: "info", info: info() }).phase).toBe("cancelled");
  });

  it("returns to connected after an interrupted download", () => {
    const s = run(
      { type: "info", info: info() },
      { type: "connected" },
      { type: "progress", sent: 10, total: 100, bytesPerSec: 5 },
      { type: "interrupted" },
    );
    expect(s).toEqual({ phase: "connected", interrupted: true });
  });

  it("does not reset a running transfer when the QR is refreshed", () => {
    const s = run(
      { type: "info", info: info() },
      { type: "progress", sent: 10, total: 100, bytesPerSec: 5 },
      { type: "info", info: info() },
    );
    expect(s.phase).toBe("transferring");
  });

  it("lets a running transfer finish past expiry", () => {
    const s = run(
      { type: "info", info: info() },
      { type: "progress", sent: 10, total: 100, bytesPerSec: 5 },
      { type: "expired" },
    );
    expect(s.phase).toBe("transferring");
    expect(run({ type: "info", info: info() }, { type: "expired" }).phase).toBe("expired");
  });

  it("maps backend errors", () => {
    expect(run({ type: "info", info: info({ status: "error", error: "no_network" }) })).toEqual({
      phase: "error",
      reason: "no_network",
    });
    expect(run({ type: "failed", reason: "not_found" })).toEqual({
      phase: "error",
      reason: "not_found",
    });
    // Retry after the network came back.
    const s = shareReducer(run({ type: "failed", reason: "no_network" }), {
      type: "info",
      info: info(),
    });
    expect(s.phase).toBe("ready");
  });

  it("computes progress and eta", () => {
    const s: SharePhase = { phase: "transferring", sent: 25, total: 100, bytesPerSec: 15 };
    expect(progressFraction(s)).toBe(0.25);
    expect(etaSeconds(s)).toBe(5);
    expect(progressFraction({ phase: "ready" })).toBeNull();
    expect(etaSeconds({ ...s, bytesPerSec: 0 })).toBeNull();
  });
});

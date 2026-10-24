import { describe, expect, it } from "vitest";
import type { ShareInfo } from "../lib/tauri";
import {
  initialReceiveState,
  receiveReducer,
  receivedCounts,
  uploadEta,
  uploadFraction,
  type ReceiveAction,
  type ReceiveState,
} from "./receiveMachine";

const info = (patch: Partial<ShareInfo> = {}): ShareInfo => ({
  id: "abc",
  status: "ready",
  error: null,
  name: "Downloads",
  size: 0,
  kind: "receive",
  isZip: false,
  fileCount: 0,
  content: "receive",
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

const upload = (index: number, count: number, sent: number, total: number): ReceiveAction => ({
  type: "upload",
  name: `IMG_${index}.jpg`,
  index,
  count,
  sent,
  total,
  bytesPerSec: 10_000_000,
});

const file = (key: string): ReceiveAction => ({
  type: "received",
  item: { key, kind: "image", name: `${key}.jpg`, size: 1000 },
});

const run = (...actions: ReceiveAction[]): ReceiveState =>
  actions.reduce(receiveReducer, initialReceiveState);

describe("receiveReducer", () => {
  it("follows a batch of two files", () => {
    let s = run({ type: "info", info: info() }, { type: "connected" });
    expect(s.view.phase).toBe("connected");
    s = receiveReducer(s, upload(1, 2, 500, 2000));
    expect(uploadFraction(s.view)).toBe(0.25);
    // First file done, second follows: keep showing the progress.
    s = receiveReducer(s, file("a"));
    expect(s.view.phase).toBe("uploading");
    s = receiveReducer(s, upload(2, 2, 2000, 2000));
    s = receiveReducer(s, file("b"));
    expect(s.view).toEqual({ phase: "received", interrupted: false });
    expect(s.items.map((i) => i.key)).toEqual(["a", "b"]);
    // A second batch later on.
    s = receiveReducer(s, upload(1, 1, 10, 100));
    expect(s.view.phase).toBe("uploading");
    expect(s.items).toHaveLength(2);
  });

  it("ignores duplicate events", () => {
    const s = run({ type: "info", info: info() }, file("a"), file("a"));
    expect(s.items).toHaveLength(1);
  });

  it("handles interruptions with and without earlier files", () => {
    let s = run({ type: "info", info: info() }, upload(1, 1, 5, 100), { type: "interrupted" });
    expect(s.view).toEqual({ phase: "connected", interrupted: true });
    s = run(
      { type: "info", info: info() },
      upload(1, 2, 50, 100),
      file("a"),
      upload(2, 2, 60, 100),
      { type: "interrupted" },
    );
    expect(s.view).toEqual({ phase: "received", interrupted: true });
  });

  it("keeps received files when the QR code is refreshed", () => {
    const s = run({ type: "info", info: info() }, file("a"), { type: "info", info: info() });
    expect(s.view.phase).toBe("received");
    expect(s.items).toHaveLength(1);
    // After "Neuen Code erstellen" the QR code is shown again.
    const renewed = run(
      { type: "info", info: info() },
      file("a"),
      { type: "expired" },
      {
        type: "info",
        info: info(),
      },
    );
    expect(renewed.view.phase).toBe("ready");
    expect(renewed.items).toHaveLength(1);
  });

  it("expires only when idle", () => {
    expect(
      run({ type: "info", info: info() }, upload(1, 1, 1, 10), { type: "expired" }).view.phase,
    ).toBe("uploading");
    const s = run({ type: "info", info: info() }, file("a"), { type: "expired" });
    expect(s.view.phase).toBe("expired");
    expect(s.items).toHaveLength(1);
  });

  it("stays cancelled", () => {
    const s = run({ type: "cancel" }, { type: "connected" }, upload(1, 1, 1, 1));
    expect(s.view.phase).toBe("cancelled");
  });

  it("computes eta and counts", () => {
    const s = run({ type: "info", info: info() }, upload(1, 1, 0, 50_000_000));
    expect(uploadEta(s.view)).toBe(5);
    expect(
      receivedCounts([
        { key: "1", kind: "image", name: "a", size: 1 },
        { key: "2", kind: "text", name: "b", size: 1 },
        { key: "3", kind: "secret", name: "", size: 1 },
      ]),
    ).toEqual({ files: 1, texts: 2 });
  });
});

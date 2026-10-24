import { describe, expect, it } from "vitest";
import { describeError } from "./errors";

describe("describeError", () => {
  it("offers retry only where a second attempt can succeed", () => {
    expect(describeError("no_network")).toEqual({ key: "no_network", retryable: true });
    expect(describeError("generic").retryable).toBe(true);
    expect(describeError("unknown_share")).toEqual({ key: "generic", retryable: false });
    expect(describeError("qr").retryable).toBe(false);
    expect(describeError("not_found").retryable).toBe(false);
    expect(describeError("clipboard_empty").retryable).toBe(false);
  });

  it("treats unknown codes as generic, even prototype names", () => {
    expect(describeError("something_new")).toEqual(describeError("generic"));
    expect(describeError("toString")).toEqual(describeError("generic"));
  });
});

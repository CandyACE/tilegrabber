import { describe, expect, it, vi } from "vitest";
import { gcj02PixelDelta } from "./gcj02";

describe("GCJ02 瓦片像素偏移", () => {
  it("瓦片尺寸翻倍时像素偏移量同步翻倍", () => {
    const delta256 = gcj02PixelDelta(10, 857, 418, 256);
    const delta512 = gcj02PixelDelta(10, 857, 418, 512);

    expect(Math.abs(delta512.dx - delta256.dx * 2)).toBeLessThanOrEqual(1);
    expect(Math.abs(delta512.dy - delta256.dy * 2)).toBeLessThanOrEqual(1);
  });

  it("无效瓦片尺寸回退到 256 并记录日志", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const fallback = gcj02PixelDelta(10, 857, 418, 0);
    const expected = gcj02PixelDelta(10, 857, 418, 256);

    expect(fallback).toEqual(expected);
    expect(warn).toHaveBeenCalledWith(
      "[gcj02] 收到无效瓦片尺寸，已回退到 256",
      { tileSize: 0 },
    );
    warn.mockRestore();
  });
});

import { describe, expect, it } from "vitest";
import { parseTileCoordsFromUrl, replaceTileXYInUrl } from "./tile-url";

describe("瓦片 URL 坐标工具", () => {
  it("解析 Google 旧式无问号参数地址", () => {
    const url =
      "https://mt0.google.com/vt/lyrs=s&scale=2&x=843&y=421&z=10&s=Galile&gl=cn";

    expect(parseTileCoordsFromUrl(url)).toEqual({ z: 10, x: 843, y: 421 });
  });

  it("替换 Google 旧式地址的邻接瓦片坐标", () => {
    const url =
      "https://mt0.google.com/vt/lyrs=s&scale=2&x=843&y=421&z=10&s=Galile&gl=cn";

    expect(replaceTileXYInUrl(url, 843, 421, 10, 844, 420)).toBe(
      "https://mt0.google.com/vt/lyrs=s&scale=2&x=844&y=420&z=10&s=Galile&gl=cn",
    );
  });
});

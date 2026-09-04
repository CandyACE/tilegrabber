/**
 * 从瓦片 URL 中解析 XYZ 坐标。
 *
 * 同时支持标准查询参数、Google 旧式 `/vt/lyrs=s&x=...` 参数和路径式
 * `/{z}/{x}/{y}` 地址。
 */
export function parseTileCoordsFromUrl(
  url: string,
): { z: number; x: number; y: number } | null {
  try {
    const parsed = new URL(
      url.startsWith("http")
        ? url
        : "https://" + url.replace(/^[^/]*\/\//, ""),
    );
    const zValue = parsed.searchParams.get("z");
    const xValue = parsed.searchParams.get("x");
    const yValue = parsed.searchParams.get("y");
    if (zValue && xValue && yValue) {
      const z = Number.parseInt(zValue, 10);
      const x = Number.parseInt(xValue, 10);
      const y = Number.parseInt(yValue, 10);
      if (![z, x, y].some(Number.isNaN)) return { z, x, y };
    }
  } catch {
    // 非标准协议会继续尝试旧式参数和路径格式。
  }

  const readLegacyParam = (name: string): number | null => {
    const match = url.match(new RegExp(`(?:[?&/])${name}=(-?\\d+)`, "i"));
    if (!match) return null;
    const value = Number.parseInt(match[1], 10);
    return Number.isNaN(value) ? null : value;
  };
  const legacyZ = readLegacyParam("z");
  const legacyX = readLegacyParam("x");
  const legacyY = readLegacyParam("y");
  if (legacyZ !== null && legacyX !== null && legacyY !== null) {
    return { z: legacyZ, x: legacyX, y: legacyY };
  }

  const pathMatch = url.match(
    /\/(\d+)\/(\d+)\/(\d+)(?:\.\w+)?(?:[?#]|$)/,
  );
  if (pathMatch) {
    return {
      z: Number(pathMatch[1]),
      x: Number(pathMatch[2]),
      y: Number(pathMatch[3]),
    };
  }
  return null;
}

/**
 * 替换瓦片 URL 中的 X/Y 坐标，缩放级别保持不变。
 */
export function replaceTileXYInUrl(
  url: string,
  origX: number,
  origY: number,
  origZ: number,
  newX: number,
  newY: number,
): string {
  try {
    const schemeMatch = url.match(/^([a-zA-Z][a-zA-Z0-9+\-.]*:\/\/)/);
    const scheme = schemeMatch?.[1] ?? "";
    const parsed = new URL(
      "https://" + url.slice(scheme.length).replace(/^\/\//, ""),
    );
    if (parsed.searchParams.has("x") && parsed.searchParams.has("y")) {
      parsed.searchParams.set("x", String(newX));
      parsed.searchParams.set("y", String(newY));
      return scheme + parsed.toString().slice("https://".length);
    }
  } catch {
    // 非标准协议继续尝试旧式参数和路径格式。
  }

  const legacyXPattern = /([?&/]x=)-?\d+/i;
  const legacyYPattern = /([?&/]y=)-?\d+/i;
  if (legacyXPattern.test(url) && legacyYPattern.test(url)) {
    return url
      .replace(legacyXPattern, `$1${newX}`)
      .replace(legacyYPattern, `$1${newY}`);
  }

  const withNewX = url.replace(
    new RegExp(`(/${origZ}/)${origX}(/${origY}(?:\\.|[?#]|$))`),
    `$1${newX}$2`,
  );
  return withNewX.replace(
    new RegExp(`(/${origZ}/${newX}/)${origY}(?=\\.|[?#]|$)`),
    `$1${newY}`,
  );
}

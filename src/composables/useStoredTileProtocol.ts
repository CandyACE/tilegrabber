/**
 * useStoredTileProtocol
 *
 * 注册 `tilegrab-stored://` MapLibre 自定义协议，用于从本地 SQLite 存储读取已下载的瓦片。
 * 协议格式：tilegrab-stored://taskId/z/x/y
 *
 * 模块级单例注册，可从多个组件安全调用，只注册一次。
 */

import maplibregl from "maplibre-gl";
import { invoke } from "@tauri-apps/api/core";

export const STORED_TILE_PROTO = "tilegrab-stored";

let _registered = false;

/** 确保 tilegrab-stored:// 协议已在 MapLibre 中注册（全局仅注册一次）。 */
export function ensureStoredTileProtocol() {
  if (_registered) return;
  _registered = true;

  maplibregl.addProtocol(STORED_TILE_PROTO, async (params) => {
    // URL 格式：tilegrab-stored://taskId/z/x/y
    const raw = params.url.replace(`${STORED_TILE_PROTO}://`, "");
    const parts = raw.split("/");
    if (parts.length < 4) throw new Error("bad tilegrab-stored url: " + params.url);

    const taskId = parts[0];
    const z = parseInt(parts[1], 10);
    const x = parseInt(parts[2], 10);
    const y = parseInt(parts[3], 10);

    try {
      const bytes = await invoke<number[]>("get_stored_tile", { taskId, z, x, y });
      return { data: new Uint8Array(bytes).buffer };
    } catch {
      // 瓦片不存在时抛出，MapLibre 将显示空白格
      throw new Error("tile not found");
    }
  });
}

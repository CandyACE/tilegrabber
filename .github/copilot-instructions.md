# Copilot 指引 — TileGrabber (御图)

跨平台桌面应用（Tauri 2 + Vue 3），用于批量下载、导出、本地发布地图瓦片。

---

## 命令

```bash
# 仅启动前端开发服务器（端口 4000）
npm run dev

# 完整开发模式（Tauri + 前端热更新）
npm run tauri:dev

# 仅构建前端
npm run build

# 完整生产构建（Rust + 前端）
npm run tauri:build
```

当前无测试套件。Rust 编译错误可通过 `npm run tauri:dev` 或在 `src-tauri/` 目录内运行 `cargo check` 发现。

---

## 架构

### 双进程模型

应用由两个进程组成，通过 Tauri IPC（`invoke` / `emit`）通信：

- **前端** (`src/`)：Vue 3 + TypeScript SPA，两个 HTML 入口：
  - `index.html` → 主窗口，入口组件 `App.vue`
  - `float.html` → 悬浮进度条窗口，入口组件 `FloatApp.vue`（独立窗口）

- **后端** (`src-tauri/src/`)：Rust。所有 IPC 处理器在 `lib.rs::run()` 的 `invoke_handler![]` 中注册。

### 后端模块一览

| 模块 | 职责 |
|---|---|
| `types.rs` | 核心共享类型：`TileSource`、`Bounds`、`CrsType`、`CoordType`、`SourceKind` |
| `tile_math.rs` | WebMercator ↔ WGS84 ↔ 瓦片坐标互转；范围内瓦片枚举 |
| `gcj02.rs` | GCJ02（"火星坐标"）↔ WGS84 偏移纠正 |
| `parser/` | LRC、LRA、WMTS、TMS URL、ovmap、区域文件、网页抓取解析器 |
| `download/` | 异步多任务下载引擎（`engine.rs`），worker 并发、限速节流、反封禁规则 |
| `export/` | MBTiles、GeoTIFF（分条带流式）、目录树、瓦片裁剪（`tile_clip.rs`） |
| `storage/app_db.rs` | 主 SQLite 数据库（`app.db`）：任务、日志、设置 — 作为 Tauri 托管状态共享 |
| `storage/tile_store.rs` | 每任务独立 SQLite 文件（`{task_id}.tiles`）：瓦片二进制数据 + 下载状态 |
| `server/` | axum HTTP 服务器，对外暴露 TMS / WMTS / WMS / ArcGIS 接口 |
| `commands/` | 薄封装层，Tauri 命令调用各模块功能 |

### 数据流

```
TileSource（已解析）→ Task（app.db）→ DownloadEngine → TileStore（{task_id}.tiles）
                                                              ↓
                                         export_mbtiles / export_geotiff / export_directory
                                         start_tile_server（axum TMS/WMTS/WMS）
```

### 前端结构

- `src/types/tile-source.ts` — TypeScript 镜像，与 `src-tauri/src/types.rs` 手动保持同步
- `src/composables/` — 通过模块级 `ref` 共享跨组件状态（如 `useWizardState`、`useTaskDetail`）
- `src/components/map/` — MapLibre GL 地图图层与叠加层
- `src/components/sidebar/` — 任务列表、任务详情、下载配置、导出、发布面板
- 路径别名 `~` 和 `@` 均解析为 `src/`

---

## 重要约定

### Tauri 托管状态

共享后端状态在 `lib.rs` 中通过 `app.manage(...)` 注册，在命令函数中用 `State<T>` 接收：

- `AppDb` — 主数据库（Arc<Mutex<Connection>>）
- `DownloadEngine` — 多任务下载管理器
- `TileServerState` — axum 瓦片服务器
- `StatsMap` — 各任务请求计数
- `ExportState` / `CancelMap` — 活跃导出任务与取消令牌
- `NetworkPausedSet` — 网络监控自动暂停的任务集合

### 新增 Tauri 命令

1. 在对应的 `commands/` 文件中实现函数
2. 在 `lib.rs` 的 `invoke_handler![]` 宏中注册
3. 在 Vue 前端添加对应的 `invoke(...)` 调用

### TypeScript ↔ Rust 类型同步

`src/types/tile-source.ts` 镜像 `src-tauri/src/types.rs`。修改 Rust 结构体时，需同步更新 TS 文件。Rust 使用 `#[serde(rename_all = "camelCase")]` 修饰 `Task`，枚举（如 `CrsType`）使用 `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`。

### 存储

- `app.db`：任务、设置（键值对）、下载日志。全局唯一实例，`Arc<Mutex<Connection>>`。
- `{task_id}.tiles`：每任务瓦片 blob 存储，WAL 模式，`PRAGMA synchronous=OFF`，`mmap_size=268435456`。
- 设置通过 `get_setting(key)` / `set_setting(key, value)` 读写，值均为字符串。

### 下载引擎

- 每个任务分配一个 `watch::channel`，发送 `CtrlSignal`（Run / Pause / Cancel）。
- 进度以 Tauri 事件 `tilegrab-progress`（payload: `ProgressPayload`）推送到前端。
- 网络监控每 15 秒探测一次；连续 2 次失败 → 宣告离线，自动暂停正在下载的任务；网络恢复后自动续传。
- 重启后，遗留 `downloading` 状态的任务重置为 `paused`；若 `app.auto_resume_on_startup` 为 true，5 秒后自动续传。

### GCJ02 坐标纠偏

`coord_type: GCJ02` 的数据源（高德、腾讯地图等）需要拼图后处理：解码瓦片图像后，逐像素应用 GCJ02→WGS84 逆偏移。逻辑在 `download/engine.rs::post_gcj02_composite`，偏移数学在 `gcj02.rs`。

### GeoTIFF 导出

- 分条带写入（每次一行瓦片，高度 256 px），峰值内存与图像高度无关。
- 支持 WebMercator（EPSG:3857）和 WGS84（EPSG:4326）源坐标系，写入正确的 EPSG tag。
- `ModelPixelScale` 的 Y 值始终为**负数**（北上方向约定）。
- 使用 vendored `tiff` crate（`src-tauri/vendor/tiff/`），在 `Cargo.toml` 中通过 `[patch.crates-io]` 覆盖。

### 瓦片发布服务

- 作为 axum 服务器在 Tauri 进程内以随机端口运行。
- 暴露端点：`/tiles/{task_id}/{z}/{x}/{y}`（TMS）、`/wmts`（GetCapabilities + GetTile）、`/wms`、`/arcgis/...`
- 所有瓦片的 MIME 类型取自任务配置的单一 `metadata.format` 字段，不做逐瓦片格式探测。

### 国际化（i18n）

- 两套语言包：`zh-CN`（默认）和 `en`，位于 `src/locales/`。
- 根据 `navigator.language` 自动检测，回退至 `zh-CN`。
- 组件内使用 `const { t } = useI18n()`；新增 key 时须同时更新两个语言文件。

### 发布流程

- 推送 `v*` tag 或手动触发 `workflow_dispatch` 启动 CI。
- 并行构建 Windows（NSIS）、macOS arm64（DMG）、Linux（AppImage + deb）。
- CI 构建时自动将 tag 版本号同步写入 `Cargo.toml`。
- 构建完成后生成 `latest.json` 并上传到 Release，供应用内更新检查使用。

---

## 解析器（parser/ 模块）

所有解析器的最终产物均为 `TileSource` 结构体。

| 解析器 | 文件 | 格式说明 |
|---|---|---|
| LRC | `parser/lrc.rs` | GB18030 编码的 XML，`<DataDefine>` 根节点；URL 用 `%d` 占位符，顺序由 `<UrlParamOrder>` 指定 |
| LRA | `parser/lra.rs` | gzip 压缩的 LRC（`.lra` 扩展名），解压后同 LRC |
| WMTS | `parser/wmts.rs` | 解析 GetCapabilities XML；自动选择第一个合法 TileMatrixSet |
| TMS | `commands/source.rs::parse_tms_url` | 直接解析 `{z}/{x}/{y}` 或 `{x}/{y}/{z}` 格式 URL 模板 |
| Area file | `parser/area_file.rs` | 文本文件，描述下载区域范围（经纬度）和层级 |
| ovmap | `parser/ovmap.rs` | OviO Map 二进制格式 |
| Web capture | `parser/web_capture.rs` | 嗅探网页瓦片请求后归一化为 TileSource |

**URL 归一化**：`TileSource::normalize_url()` 统一把 LRC 的 `%d/%d/%d` 和 `{$serverpart}` 转换为标准的 `{z}/{x}/{y}` + `{s}` 占位符（在 `types.rs` 中定义）。

---

## 前端组件模式

### MapLibre 图层集成

地图图层均为**纯非可视组件**（不渲染 DOM），通过 props 接收 `map: MaplibreMap | null` 和 `taskId`：

```vue
<!-- 在父组件中向图层传递地图实例 -->
<LocalTaskTileLayer :map="mapRef" :task-id="selectedTaskId" />
```

组件内统一模式：
- 监听 `taskId` 变化 → 添加/移除 MapLibre source 和 layer
- 用 `generation` 计数器防止异步回调在组件卸载后继续操作已销毁的地图
- 图层 ID 和 source ID 用组件内常量声明，方便 `getLayer` / `removeLayer` 判断

### 自定义协议：`tilegrab-stored://`

本地已下载瓦片通过自定义 MapLibre 协议展示，避免绕道 Tauri HTTP 服务器：

```
tilegrab-stored://{taskId}/{z}/{x}/{y}
→ invoke("get_stored_tile", { taskId, z, x, y })
→ 返回 Uint8Array
```

注册逻辑在 `composables/useStoredTileProtocol.ts`，模块级单例，多个组件调用 `ensureStoredTileProtocol()` 只注册一次。

### Composable 模式

跨组件共享状态用**模块级 `ref`**（非 `provide/inject`）：

```ts
// 模块顶部声明单例 ref
const selectedTaskId = ref<string | null>(null)

// 导出函数操作状态
export function useTaskDetail() {
  return { selectedTaskId, openTask, closeTask }
}
```

所有 composable 均在 `src/composables/` 目录，目前有：`useWizardState`、`useTaskDetail`、`useStoredTileProtocol`、`useExportJobs`、`useToast`、`useUpdateState`。

---

## 导出格式细节

### MBTiles

- 表结构：`metadata`（name/value）+ `tiles`（zoom_level, tile_column, tile_row, tile_data BLOB）
- **Y 轴翻转**：MBTiles 规范要求 TMS 坐标系，`tile_row = 2^zoom - 1 - y`（存储时翻转）
- 元数据必填字段：`name`、`description`、`version`、`bounds`（west,south,east,north）、`center`、`minzoom`、`maxzoom`、`format`、`type`
- 写入时：`PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF` 批量插入，完成后恢复

### GeoTIFF / BigTIFF

- 分条带流式写入，每次处理一行瓦片（256 px），峰值内存 = `宽度 × 256 × 4 字节`
- 支持输出坐标系重投影（`target_epsg` 参数）：
  - `4326` / `4490` 等 → Geographic（经纬度像素网格）
  - `3857` → WebMercator（米坐标）
  - `32601–32760` → UTM 分带（真正的像素重投影）
- `ModelPixelScale` Y 值始终为负数（北上方向约定）
- 压缩选项：`"none"` / `"lzw"` / `"deflate"`
- 使用 vendored `tiff` crate（`src-tauri/vendor/tiff/`），以支持 BigTIFF（> 4 GB）

### 目录树

- 按 `{z}/{x}/{y}.{format}` 结构输出到文件夹
- 边界瓦片可按多边形裁剪（`tile_clip.rs` 扫描线抗锯齿算法）

### 瓦片裁剪（tile_clip.rs）

边界相交的瓦片不整体排除，而是逐像素裁剪：
- 范围外像素 alpha 清零（就地修改 raw buffer）
- 批量清零用 `slice::fill(0)` 触发 SIMD/memset 向量化
- 边界抗锯齿：按子像素覆盖比例设置半透明
- 裁剪后重新编码为 PNG（`zlib Fast` 级别，速度约 2-4×）

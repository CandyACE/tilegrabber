# 更新日志

所有版本的变更记录按时间倒序排列，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)。

---

## [v0.5.0] - 待发布

### 安全

- **更新通道端到端签名**：移除自实现 updater，改用官方 `tauri-plugin-updater` + minisign 签名。CI 用仓库 Secret 注入私钥签所有 updater 产物，客户端用编译期内置公钥校验，篡改 latest.json 或中间人替换 exe 会被直接拒绝
- **WMTS / WMS 服务端 XML 转义**：所有用户提供的任务名 / format 字段在 GetCapabilities XML 中转义，杜绝 XML 注入；输出格式与 ResourceURL.format / WMTS Format / ArcGIS tileInfo.format 跟随任务实际格式，避免格式不一致导致的客户端解析异常
- **隐藏未上线的远程协作功能**：避免误用，相关入口已从 UI 与服务端路由暂时下架

### 新增

- **H 套件（H1 + H2 + H3）— 矢量瓦片基础支持**：下载链接含 `pbf` / `mvt` / `vector` 关键字时自动识别为矢量格式 (format=pbf)
  - **下载流水线**：矢量任务跳过 GCJ02 像素纠偏与 tile_clip 像素裁剪（这些后处理仅对栅格图像有意义）；若同时是 GCJ02 坐标会写入警告日志
  - **预览**：`LocalTaskTileLayer` 自动按 task.format 切换 raster / vector 数据源；矢量分支会对首块瓦片做「样式内省」，按 source-layer 与几何类型（点/线/面）自动生成灰白骨架样式（fill + line + circle）
  - **MBTiles 导出**（H3）：矢量任务导出会按 MBTiles 规范自动 gzip 包装 tile_data、metadata.type 设为 overlay、扫描首批瓦片提取真实 layer 名后写入 `vector_layers` JSON；像素裁剪、PNG/JPEG 重编码、GeoTIFF 选项在矢量任务下自动隐藏并给出说明
  - **依赖**：新增 `@mapbox/vector-tile` + `pbf` 两个前端依赖用于 MVT 内省；后端新增轻量 `mvt_scan` 模块（纯 Rust，无 protobuf crate 依赖）
  - **暂未支持**（H4-H5 后续）：PMTiles 矢量导出、GeoJSON 导出
- **G 套件 — 存储占用面板**：设置页顶部新增可折叠的「存储占用」卡片，展示磁盘使用率（本应用 vs 其他占用 双层进度条）、各任务 .tiles 文件大小（按大小排序，默认显示前 5 个）；自动扫描默认目录的孤儿 .tiles 文件（已删任务残留），支持一键清理
  - **后端**：新增 `storage_stats` 命令汇总每任务存储 + 容量 + 孤儿；新增 `cleanup_orphan_tiles` 命令带白名单保护（仅删默认目录内未被引用的 .tiles，含 WAL/SHM 兄弟文件）
  - **前端**：新增 `StorageStatsCard.vue` 组件，集成于 SettingsPanel 顶部
- **E 套件 — 失败瓦片可视化**：任务详情新增「失败瓦片分布」卡片，按 zoom 层级显示失败瓦片数量徽章；点击层级在地图上以红色透明矩形高亮该层级的所有失败瓦片位置；可单独「重试本层」或沿用旧的「重试全部失败瓦片」
  - **后端**：新增 `failed_tiles_summary` / `list_failed_tiles` / `retry_failed_tiles` 三个命令；`reset_failed_at_zoom` 支持按层级重置失败状态后续传
  - **前端**：新增 `FailedTilesLayer` 地图覆盖层 + `useFailedTilesView` 共享状态；切换任务自动清理可视化
  - **UX 优化**：点击层级自动 fitBounds 缩放到该层级失败瓦片包围盒；支持多选层级 + 批量重试（全选时一次性重置全部失败）
- **D 套件 — 预设源市场**：在「新建任务」向导的「预置底图」分类下追加多家常用底图，一键载入即可下载
  - **腾讯地图**：路网图、地形图（GCJ02，TMS Y 翻转）
  - **天地图**：矢量底图 / 矢量注记 / 影像底图 / 影像注记 / 地形晕渲 / 地形注记（WGS84，需用户填 `tk`）
  - **Bing Maps**：Aerial / Road / Hybrid（WGS84，使用 quadkey）
  - **OSM 子图层**：Humanitarian、CyclOSM、OpenTopoMap
  - **高德新增**：卫星注记叠加图层（半透明标签层）
- **后端**：`build_tile_url` 新增 `{q}` 占位符，自动按 (z,x,y) 计算 Bing 风格 quadkey
- **Token 弹层**：选中需要凭证的预设（如天地图）时自动弹出输入框，填入后保存到设置（`preset.tianditu_token`），下次默认带入；URL 模板用 `{{tk}}` 引用、通过 `extra_params` 注入
- **F 套件 — 增量更新（仅限已完成的任务）**：在任务详情底部新增「增量更新」卡片，含 2 个入口：
  - **扩展任务（F2）**：点击后进入地图绘制模式，顶部横幅显示当前任务名，原始区域以红色轮廓同步显示。用户在地图上框选要追加的区域，确认时后端取「原 bbox ∪ 新框选」并通过 `update_task_geometry` 写库 + 续传新增瓦片
  - **借数据（F4）**：候选列表严格筛选「同源（URL/格式/CRS/坐标类型一致）且已完整下载」的任务，列表右侧实时显示所选任务的缩略图预览（`MiniMapPreview` + `tilegrab-stored://` MapLibre 协议直读本地瓦片），确认后从源 .tiles 复制可复用瓦片到当前任务，跳过网络下载
- **后端命令**：`update_task_geometry`、`count_reusable_tiles`、`import_tiles_from_source`、`get_stored_tile`
- **Windows 静默更新**：使用 NSIS passive 模式，更新过程仅显示进度条无需点击，安装完成后自动重启
- **macOS / Linux 在应用内自动替换**：plugin-updater 解压 `.app.tar.gz` / `.AppImage.tar.gz` 直接覆盖原可执行文件
- **更新进度事件**：UI 显示下载已传输字节 / 总大小，回滚到内置百分比模型

### 修复

- **托盘构建失败导致启动崩溃**：缺图标时改为日志告警并跳过托盘创建，不阻断主窗口
- **下载信号量错误处理**：`.unwrap()` → `.expect()` 并加注释；mutex 中毒时优雅恢复（`PoisonError::into_inner`），避免链式 panic 让导出任务半成品状态卡死

### 破坏性变更

- **`commands/updater.rs` 整个移除**：旧的 `check_for_update` / `download_and_install_update` / `open_release_url` Tauri 命令不再存在
- **`latest.json` schema 双写**：本版本同时输出旧 schema（`assets.{windows,macos,linux}`）和新 schema（`version` / `notes` / `pub_date` / `platforms.{target}.{signature,url}`）。等所有用户从 v0.4.8 升上来后，下一个 minor 版本可清理旧 `assets` 字段
- **必须配置签名密钥**：fork 本仓库后，必须按 README → 「Fork 后自建发布通道」生成自己的 minisign 密钥对，否则 CI 不会产出可用的更新

---

## [v0.4.8] - 2026-04-29

### 新增

- **GCJ02 地图预览实时纠偏**：地图预览中对高德 / 腾讯等 GCJ02 坐标系图层启用前端像素级纠偏——新增 `tilegrab-gcj02://` 自定义 MapLibre 协议，在 OffscreenCanvas 上动态抓取 2×2 邻块瓦片并按亚像素偏移量拼合裁剪，消除 GCJ02 与 WGS84 的视觉偏差（约 100–700 m），无需服务端参与
- **图层列表 GCJ02 标识**：图层侧栏对 GCJ02 坐标系来源显示「GCJ02纠偏」琥珀色角标，一目了然
- **GeoTIFF 压缩导出**：导出 GeoTIFF 时可在导出面板选择压缩方式（无压缩 / LZW 无损 / DEFLATE 无损），默认无压缩；通过 vendor 本地修补 `tiff` 0.9.1 库，修复 `write_strip` 不激活压缩器的底层缺陷，确保 LZW / DEFLATE 条带均可被 GDAL / QGIS 正确读取
- **下载前预估大小**：新建任务时实时显示预计瓦片数量和数据量（自动防抖，拖动缩放级别时即时更新）
- **失败瓦片角标**：任务卡片在任务完成或暂停后，若存在失败瓦片将显示 ⚠ N 失败 角标，方便追踪问题
- **WebP 瓦片支持**：下载引擎可正确处理并存储 WebP 格式的瓦片响应
- **下载并发数设置**：设置面板支持配置同时下载的并发连接数上限
- **全部暂停 / 全部恢复**：任务列表一键操作所有任务的暂停与恢复
- **网络离线自动暂停**：检测到网络中断时自动暂停所有进行中任务，网络恢复后提示用户继续；使用 `www.baidu.com` 作为探测目标，连续 2 次失败才判断为离线，避免误判
- **崩溃自动恢复**：重启后自动恢复上次意外中断的下载任务（5 秒延迟，可在设置中关闭）
- **下载时磁盘监控**：每 50 个批次检查一次可用磁盘空间，剩余不足 200 MB 时暂停任务并在顶部显示警告横幅

### 修复

- **GCJ02 任务进度卡在 99%**：GCJ02 下载完成后清理边缘补白瓦片时，`tasks.total_tiles` 未随之更新，导致进度永远无法到达 100%；现在在补白清理后同步更新数据库中的总瓦片数
- **GCJ02 多边形任务边缘缺瓦片**：GCJ02 + 多边形任务改为下载完整扩展范围（含 0.01° 补白），多边形遮罩移至后处理阶段应用，避免边缘合成时因来源瓦片缺失导致的黑边
- **重试抖动（Jitter）**：重试退避时间引入 0–500 ms 随机抖动，减少大量并发重试时的集中冲击

### 优化

- **最大重试次数生效**：设置面板中的「最大重试次数」现已正确连接至下载后端（此前为硬编码值 5，现在使用设置中的值，默认 3）
- **已下载瓦片预览协议解耦**：`tilegrab-stored://` 协议提取为独立 composable（`useStoredTileProtocol.ts`），可跨组件复用，避免重复注册

---

## [v0.4.6] - 2026-04-23

### 修复

- **GeoTIFF 坐标参考异常**：修复导出时错误的 GeoTIFF 像素尺度标签号，并按源瓦片 CRS 正确写入 EPSG:4326 / EPSG:3857 与范围信息，避免外部 GIS 将坐标范围识别错误
- **GeoTIFF 在 QGIS 中读取失败**：GeoTIFF 导出统一改为 BigTIFF 无压缩写出，绕开压缩 strip 兼容性问题，避免 `TIFFReadEncodedStrip()` / `IReadBlock failed` 等加载错误
- **GeoTIFF 严格裁剪黑底**：为 RGBA GeoTIFF 显式写入 Alpha 通道 `ExtraSamples` 标签，修复 QGIS 中裁剪透明区显示为黑块的问题
- **导出默认裁剪行为**：导出面板中的「严格裁剪至任务选框范围」现在默认开启

### 优化

- 移除 GeoTIFF 压缩选项，统一收敛为更稳定的 BigTIFF 导出行为
- 修正过期的瓦片数学测试断言，并修复目录导出模块注释块导致的 doctest 误判

---

## [v0.4.5] - 2026-04-10

### 新增

- **MBTiles 导入**：支持将本地 `.mbtiles` 文件导入为下载任务，导入完成后可直接导出为其他格式
- **GeoTIFF 压缩选项**：导出 GeoTIFF 时可选择压缩方式（LZW 无损 / Deflate 无损 / 不压缩），默认 LZW；旧版本默认为不压缩
- **瓦片裁剪抗锯齿**：精准裁剪模式下，矩形边界与多边形边界的裁剪边缘改为子像素覆盖率 alpha，消除锯齿感

### 修复

- **切换 Tab 时地图预览未清除**：从地图 Tab 切走时，自动清除已选图层的瓦片预览叠加层
- **任务详情不自动关闭**：从任务 Tab 切走再回来时，自动返回任务列表（不再停留在任务详情页）
- **任务筛选显示原始 key**：修复「全部」筛选按钮显示 `tasks.filterAll` 原始文本的问题
- **CI 安装包名称**：将 `productName` 改为 `TileGrabber`，避免中文在 CI 环境（US locale）被截断导致安装包文件名缺少前缀
- **latest.json 下载地址 404**：修复 CI 工作流中 `latest.json` 生成时序问题——现在先发布 Release，再重新获取资产 URL（真实 tag slug），再生成并上传 `latest.json`，避免草稿状态的 `untagged-*` URL 被写入

### 优化

- 下载范围配置页面调整布局：精准裁剪开关移至缩放级别选择之前

---

## [v0.4.3] - 2026-04-09

### 新增

- **发布页面 — 多协议支持**：内置 HTTP 服务器现同时提供五种标准服务协议
  - **WMS 1.1.1**（OGC Web Map Service）：支持 `GetCapabilities` 和 `GetMap`，兼容 EPSG:4326 / EPSG:3857，实时合成瓦片图像
  - **OGC API Tiles**：新一代 OGC REST API，兼容 MapLibre GL JS 等现代客户端
  - **ArcGIS REST API**（MapServer 兼容）：可直接挂载至 ArcGIS Online、Esri Leaflet 等 Esri 生态
- **局域网地址切换**：服务启动后自动检测所有本机 IP，「访问地址」下拉选择后，所有协议 URL 及内置代码示例同步更新
- **内置代码示例**：每个发布任务内嵌示例代码，支持 Cesium.js / Leaflet.js / MapLibre GL JS 三种框架 × 五种协议（TMS / WMTS / WMS / OGC / ArcGIS）共 15 种组合，一键复制
- **请求统计**：实时显示各任务的累计请求数（含 XYZ / WMTS / WMS / OGC / ArcGIS 五种协议之和）及最后请求时间

### 优化

- 发布页面协议切换按钮从 2 个（TMS / WMTS）扩展为 5 个（TMS / WMTS / WMS / OGC / ESRI）
- 「示例代码」区域不再受服务运行状态影响，始终可展开查看
- 内置帮助文档「本地发布服务」章节扩充至 11 条，覆盖所有新增协议及功能

---

## [v0.4.2] - 2026-04-08

### 优化

- 任务列表与图层列表切换时增加 `TransitionGroup` 过渡动画（淡入 / 滑出 / FLIP 位移），交互更流畅
- 撤销面板懒加载改动，改回直接 `import`，移除已不再使用的 `PanelLoading` 组件

### CI

- Node.js 版本固定改为跟随 LTS（`lts/*`），避免因大版本锁定导致的构建警告
- 修复 CI 脚本中版本同步打印语句的编码问题

---

## [v0.4.1] - 2026-04-07

### 优化

- 懒加载面板（设置、帮助、关于、发布、新建任务向导）加载时显示居中转圈动画，提升交互流畅感

---

## [v0.4.0] - 2026-04-07

### 新增

- 界面完整国际化（简体中文 / English），自动检测系统语言，可在设置中手动切换
- 启动时后台静默检查更新（延迟 12 秒），有新版本时标题栏设置按钮显示红点提示
- 更新内容（Release Notes）现在完整显示，不再截断

### 修复

- 修复通过 GitHub Actions 构建后自动更新无法检测到最新版本的问题（`build.rs` 中空值 `cargo:rustc-env` 覆盖了 CI 注入的更新地址）
- 修复更新日志正文被强制截断至 500 字符的问题

### 优化

- 非首屏面板（设置、帮助、关于、发布、新建任务向导）改为按需懒加载，启动时减少约 190 kB JS 解析量
- 应用启动时将多次 IPC 调用合并为单次 `get_all_settings`，并行注册事件监听器，降低启动延迟
- CI 构建优化：自动同步 Cargo.toml 版本号、轮询等待替代固定 `sleep`、多平台产物就绪校验

---

## [v0.3.0] - 2026-04-04

### 新增

- 支持多边形（任意形状）区域绘制，精确框选下载范围
- GeoTIFF 导出：自动按多边形边界进行像素级裁剪，导出结果更精确
- GeoTIFF 支持 BigTIFF 格式（64-bit 偏移量），支持超过 4 GB 的超大图像导出
- 支持 `.tgr` 任务包文件导入与导出（零拷贝 SQLite v2 格式），大幅提升迁移速度
- 任务边界在地图上叠加多边形轮廓（橙色）预览

### 修复

- 修复导出 GeoTIFF 时多边形任务边缘未按多边形形状裁剪的问题
- 修复下载遇到 HTTP 502 / 503 / 504 响应未自动重试的问题，现改为最多重试 5 次并逐步加大等待间隔
- 修复部分数据源（代理协议）预览瓦片加载空白的问题
- 修复 ZoomPicker 拖拽时出现闪烁/高度跳动的问题
- 外部 `.tgr` 任务卡片显示存储标识；删除时可选择保留或一并删除外部文件

### 优化

- 后处理精确裁剪性能大幅提升：多边形裁剪改用扫描线算法（O(H×V) 替代原来的 O(H×W×V)）、PNG 编码切换为快速压缩模式，裁剪速度显著提升
- 下载引擎 SQLite I/O 优化：`cache_size` 增至 200 MB，`mmap_size` 增至 512 MB，批量处理量提升至 1000 条/批

---

<!-- 在此处添加新版本，保持倒序排列，格式示例：

## [vX.Y.Z] - YYYY-MM-DD

### 新增
- ...

### 修复
- ...

### 优化
- ...

-->

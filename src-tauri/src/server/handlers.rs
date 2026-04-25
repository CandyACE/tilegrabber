//! axum 请求处理器
//!
//! 包含：
//! - `tms_tile`        — TMS /tiles/{task}/{z}/{x}/{y}
//! - `wmts_dispatch`   — WMTS 单入口（GetCapabilities / GetTile）
//! - `wms_dispatch`    — WMS 1.1.1 单入口（GetCapabilities / GetMap）
//! - `api_tasks`       — GET /api/tasks  (JSON 列表)
//! - `api_stats`       — GET /api/stats  (JSON 请求统计)

use std::f64::consts::PI;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use rusqlite::{params, Connection};

use super::{ServerAppState, StatsMap};

// ─── 统计辅助 ─────────────────────────────────────────────────────────────────

fn update_stats(stats: &StatsMap, task_id: &str, service: &str) {
    if let Ok(mut map) = stats.lock() {
        let entry = map.entry(task_id.to_string()).or_default();
        match service {
            "tms" => entry.tms_requests += 1,
            "wmts" => entry.wmts_requests += 1,
            "wms" => entry.wms_requests += 1,
            "ogc" => entry.ogc_requests += 1,
            "arcgis" => entry.arcgis_requests += 1,
            _ => {}
        }
        entry.last_request_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
    }
}

// ─── TMS 单瓦片 ─────────────────────────────────────────────────────────────

pub async fn tms_tile(
    State(state): State<ServerAppState>,
    Path((task_id, z, x, y)): Path<(String, i64, i64, i64)>,
) -> Response {
    update_stats(&state.stats, &task_id, "tms");
    serve_tile(&state, task_id, z, x, y).await
}

/// 共享的瓦片读取逻辑，不计入统计（由调用方决定统计类型）
async fn serve_tile(state: &ServerAppState, task_id: String, z: i64, x: i64, y: i64) -> Response {
    // 从 app_db 查询 tile_store_path
    let tile_store_path = match state.app_db.get_task(&task_id) {
        Ok(t) => match t.tile_store_path {
            Some(p) => p,
            None => return (StatusCode::NOT_FOUND, "task tile_store_path is null").into_response(),
        },
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    // 读取瓦片数据（阻塞 IO 放到 spawn_blocking）
    let result =
        tokio::task::spawn_blocking(move || read_tile_from_store(&tile_store_path, z, x, y)).await;

    match result {
        Ok(Ok(Some((data, format)))) => {
            let mime = format_to_mime(&format);
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("image/png")),
            );
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=86400"),
            );
            (headers, data).into_response()
        }
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "tile not found").into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 从 .tiles SQLite 文件读取瓦片原始字节
fn read_tile_from_store(
    path: &str,
    z: i64,
    x: i64,
    y: i64,
) -> anyhow::Result<Option<(Bytes, String)>> {
    let conn = Connection::open(path)?;
    // 查 format
    let format: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE name='format'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "png".to_string());

    // 查瓦片
    let mut stmt = conn.prepare(
        "SELECT tile_data FROM tiles WHERE tile_column=?1 AND tile_row=?2 AND zoom_level=?3",
    )?;
    let mut rows = stmt.query(params![x, y, z])?;
    if let Some(row) = rows.next()? {
        let data: Vec<u8> = row.get(0)?;
        Ok(Some((Bytes::from(data), format)))
    } else {
        Ok(None)
    }
}

fn format_to_mime(fmt: &str) -> &'static str {
    match fmt.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "pbf" | "mvt" => "application/x-protobuf",
        _ => "application/octet-stream",
    }
}

// ─── WMTS 单入口 ─────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Default)]
pub struct WmtsParams {
    #[serde(rename = "SERVICE")]
    pub service: Option<String>,
    #[serde(rename = "REQUEST")]
    pub request: Option<String>,
    #[serde(rename = "VERSION")]
    pub version: Option<String>,
    #[serde(rename = "LAYER")]
    pub layer: Option<String>,
    #[serde(rename = "TILEMATRIXSET")]
    pub tile_matrix_set: Option<String>,
    #[serde(rename = "TILEMATRIX")]
    pub tile_matrix: Option<String>,
    #[serde(rename = "TILEROW")]
    pub tile_row: Option<String>,
    #[serde(rename = "TILECOL")]
    pub tile_col: Option<String>,
    #[serde(rename = "FORMAT")]
    pub format: Option<String>,
}

pub async fn wmts_dispatch(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
    Query(params): Query<WmtsParams>,
) -> Response {
    update_stats(&state.stats, &task_id, "wmts");
    let request = params
        .request
        .as_deref()
        .unwrap_or("GetCapabilities")
        .to_ascii_uppercase();

    match request.as_str() {
        "GETCAPABILITIES" | "GETMAP" => wmts_get_capabilities(&state, &task_id).await,
        "GETTILE" => wmts_get_tile(State(state), Path(task_id), Query(params)).await,
        _ => (StatusCode::BAD_REQUEST, "unsupported WMTS REQUEST").into_response(),
    }
}

async fn wmts_get_capabilities(state: &ServerAppState, task_id: &str) -> Response {
    // 获取任务信息用于构建能力文档
    let task = match state.app_db.get_task(task_id) {
        Ok(t) => t,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    let base = &state.base_url;
    let min_z = task.min_zoom;
    let max_z = task.max_zoom;

    // 生成 TileMatrix 条目
    let mut tile_matrices = String::new();
    for z in min_z..=max_z {
        let size = 1u64 << z; // 2^z
        let scale = 559_082_264.028_717_6 / (size as f64); // OGC 标准比例尺
        tile_matrices.push_str(&format!(
            r#"
            <TileMatrix>
              <ows:Identifier>{z}</ows:Identifier>
              <ScaleDenominator>{scale:.6}</ScaleDenominator>
              <TopLeftCorner>-20037508.3428 20037508.3428</TopLeftCorner>
              <TileWidth>256</TileWidth>
              <TileHeight>256</TileHeight>
              <MatrixWidth>{size}</MatrixWidth>
              <MatrixHeight>{size}</MatrixHeight>
            </TileMatrix>"#,
        ));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Capabilities xmlns="http://www.opengis.net/wmts/1.0"
              xmlns:ows="http://www.opengis.net/ows/1.1"
              xmlns:xlink="http://www.w3.org/1999/xlink"
              version="1.0.0">
  <ows:ServiceIdentification>
    <ows:Title>TileGrabber WMTS</ows:Title>
    <ows:ServiceType>OGC WMTS</ows:ServiceType>
    <ows:ServiceTypeVersion>1.0.0</ows:ServiceTypeVersion>
  </ows:ServiceIdentification>
  <Contents>
    <Layer>
      <ows:Identifier>{task_id}</ows:Identifier>
      <ows:Title>{name}</ows:Title>
      <ows:BoundingBox crs="urn:ogc:def:crs:OGC:1.3:CRS84">
        <ows:LowerCorner>{west} {south}</ows:LowerCorner>
        <ows:UpperCorner>{east} {north}</ows:UpperCorner>
      </ows:BoundingBox>
      <Style isDefault="true"><ows:Identifier>default</ows:Identifier></Style>
      <Format>image/png</Format>
      <TileMatrixSetLink>
        <TileMatrixSet>WebMercatorQuad</TileMatrixSet>
      </TileMatrixSetLink>
      <ResourceURL format="image/png" resourceType="tile"
        template="{base}/wmts/{task_id}?SERVICE=WMTS&amp;REQUEST=GetTile&amp;VERSION=1.0.0&amp;LAYER={task_id}&amp;TILEMATRIXSET=WebMercatorQuad&amp;TILEMATRIX={{TileMatrix}}&amp;TILEROW={{TileRow}}&amp;TILECOL={{TileCol}}"/>
    </Layer>
    <TileMatrixSet>
      <ows:Identifier>WebMercatorQuad</ows:Identifier>
      <ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>
      {tile_matrices}
    </TileMatrixSet>
  </Contents>
  <ServiceMetadataURL xlink:href="{base}/wmts/{task_id}?SERVICE=WMTS&amp;REQUEST=GetCapabilities"/>
</Capabilities>"#,
        task_id = task_id,
        name = task.name,
        west = task.bounds_west,
        south = task.bounds_south,
        east = task.bounds_east,
        north = task.bounds_north,
        base = base,
        tile_matrices = tile_matrices,
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    (headers, xml).into_response()
}

async fn wmts_get_tile(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
    Query(params): Query<WmtsParams>,
) -> Response {
    let z: i64 = params
        .tile_matrix
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let row: i64 = params
        .tile_row
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let col: i64 = params
        .tile_col
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // WMTS TileRow 从左上角计数（北向下），与内部存储一致，无需翻转
    // 直接读取瓦片，不重复计入 TMS 统计
    serve_tile(&state, task_id, z, col, row).await
}

// ─── REST /api/tasks ─────────────────────────────────────────────────────────

/// 列出全部任务  GET /api/tasks
pub async fn api_tasks(State(state): State<ServerAppState>) -> Response {
    match state.app_db.list_tasks() {
        Ok(tasks) => json_response(&tasks),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 获取单个任务  GET /api/tasks/{id}
pub async fn api_task_get(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
) -> Response {
    match state.app_db.get_task(&task_id) {
        Ok(t) => json_response(&t),
        Err(_) => (StatusCode::NOT_FOUND, "task not found").into_response(),
    }
}

/// 获取任务日志  GET /api/tasks/{id}/logs
pub async fn api_task_logs(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
) -> Response {
    match state.app_db.get_task_logs(&task_id, 500) {
        Ok(logs) => json_response(&logs),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 服务器自身信息  GET /api/info
pub async fn api_info(State(state): State<ServerAppState>) -> Response {
    let info = serde_json::json!({
        "name": "TileGrabber",
        "version": env!("CARGO_PKG_VERSION"),
        "base_url": state.base_url,
        "endpoints": {
            "tms":  format!("{}/tiles/{{task_id}}/{{z}}/{{x}}/{{y}}", state.base_url),
            "wmts": format!("{}/wmts/{{task_id}}?SERVICE=WMTS&REQUEST=GetCapabilities", state.base_url),
            "tasks": format!("{}/api/tasks", state.base_url),
            "info":  format!("{}/api/info", state.base_url),
        }
    });
    json_response(&info)
}

// ─── 辅助 ────────────────────────────────────────────────────────────────────

fn json_response<T: serde::Serialize>(data: &T) -> Response {
    match serde_json::to_string(data) {
        Ok(body) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            (headers, body).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ─── WMS 1.1.1 单入口 ────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Default)]
pub struct WmsParams {
    #[serde(rename = "SERVICE")]
    pub service: Option<String>,
    #[serde(rename = "REQUEST")]
    pub request: Option<String>,
    #[serde(rename = "VERSION")]
    pub version: Option<String>,
    #[serde(rename = "LAYERS")]
    pub layers: Option<String>,
    #[serde(rename = "STYLES")]
    pub styles: Option<String>,
    /// WMS 1.1.x 坐标系参数
    #[serde(rename = "SRS")]
    pub srs: Option<String>,
    /// WMS 1.3.0 坐标系参数（兼容）
    #[serde(rename = "CRS")]
    pub crs: Option<String>,
    #[serde(rename = "BBOX")]
    pub bbox: Option<String>,
    #[serde(rename = "WIDTH")]
    pub width: Option<String>,
    #[serde(rename = "HEIGHT")]
    pub height: Option<String>,
    #[serde(rename = "FORMAT")]
    pub format: Option<String>,
    #[serde(rename = "TRANSPARENT")]
    pub transparent: Option<String>,
}

pub async fn wms_dispatch(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
    Query(params): Query<WmsParams>,
) -> Response {
    update_stats(&state.stats, &task_id, "wms");
    let request = params
        .request
        .as_deref()
        .unwrap_or("GetCapabilities")
        .to_ascii_uppercase();

    match request.as_str() {
        "GETCAPABILITIES" => wms_get_capabilities(&state, &task_id).await,
        "GETMAP" => wms_get_map(&state, &task_id, &params).await,
        _ => (StatusCode::BAD_REQUEST, "unsupported WMS REQUEST").into_response(),
    }
}

async fn wms_get_capabilities(state: &ServerAppState, task_id: &str) -> Response {
    let task = match state.app_db.get_task(task_id) {
        Ok(t) => t,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    let base = &state.base_url;
    let online_resource = format!("{base}/wms/{task_id}?");

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE WMT_MS_Capabilities SYSTEM "http://schemas.opengis.net/wms/1.1.1/WMS_MS_Capabilities.dtd">
<WMT_MS_Capabilities version="1.1.1">
  <Service>
    <Name>WMS</Name>
    <Title>TileGrabber WMS</Title>
    <Abstract>TileGrabber local WMS service</Abstract>
    <OnlineResource xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="{online_resource}"/>
  </Service>
  <Capability>
    <Request>
      <GetCapabilities>
        <Format>application/vnd.ogc.wms_xml</Format>
        <DCPType><HTTP><Get><OnlineResource xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="{online_resource}"/></Get></HTTP></DCPType>
      </GetCapabilities>
      <GetMap>
        <Format>image/png</Format>
        <DCPType><HTTP><Get><OnlineResource xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="{online_resource}"/></Get></HTTP></DCPType>
      </GetMap>
    </Request>
    <Exception><Format>application/vnd.ogc.se_xml</Format></Exception>
    <Layer>
      <Title>TileGrabber WMS</Title>
      <SRS>EPSG:4326</SRS>
      <SRS>EPSG:3857</SRS>
      <Layer queryable="0" opaque="0" cascaded="0">
        <Name>{task_id}</Name>
        <Title>{name}</Title>
        <SRS>EPSG:4326</SRS>
        <SRS>EPSG:3857</SRS>
        <LatLonBoundingBox minx="{west}" miny="{south}" maxx="{east}" maxy="{north}"/>
        <BoundingBox SRS="EPSG:4326" minx="{west}" miny="{south}" maxx="{east}" maxy="{north}"/>
      </Layer>
    </Layer>
  </Capability>
</WMT_MS_Capabilities>"#,
        online_resource = online_resource,
        task_id = task_id,
        name = task.name,
        west = task.bounds_west,
        south = task.bounds_south,
        east = task.bounds_east,
        north = task.bounds_north,
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    (headers, xml).into_response()
}

async fn wms_get_map(state: &ServerAppState, task_id: &str, params: &WmsParams) -> Response {
    // ── 解析必填参数 ─────────────────────────────────────────────────────────
    let bbox_str = match &params.bbox {
        Some(b) => b.clone(),
        None => return (StatusCode::BAD_REQUEST, "missing BBOX parameter").into_response(),
    };
    let parts: Vec<f64> = bbox_str
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if parts.len() != 4 {
        return (
            StatusCode::BAD_REQUEST,
            "invalid BBOX, expected minx,miny,maxx,maxy",
        )
            .into_response();
    }

    let srs = params
        .srs
        .as_deref()
        .or(params.crs.as_deref())
        .unwrap_or("EPSG:4326");

    // 统一转换为 WGS84 (lon, lat)
    let (mut min_lon, mut min_lat, mut max_lon, mut max_lat) =
        if srs.to_uppercase().contains("3857") {
            let (lo0, la0) = merc_to_wgs84(parts[0], parts[1]);
            let (lo1, la1) = merc_to_wgs84(parts[2], parts[3]);
            (lo0, la0, lo1, la1)
        } else {
            (parts[0], parts[1], parts[2], parts[3])
        };

    // 保证方向正确
    if min_lon > max_lon {
        std::mem::swap(&mut min_lon, &mut max_lon);
    }
    if min_lat > max_lat {
        std::mem::swap(&mut min_lat, &mut max_lat);
    }

    // 裁剪到 Mercator 有效范围
    min_lat = min_lat.clamp(-85.051129, 85.051129);
    max_lat = max_lat.clamp(-85.051129, 85.051129);

    let width: u32 = params
        .width
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256)
        .min(4096);
    let height: u32 = params
        .height
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256)
        .min(4096);

    // ── 查询任务元数据 ────────────────────────────────────────────────────────
    let task = match state.app_db.get_task(task_id) {
        Ok(t) => t,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let tile_store_path = match task.tile_store_path {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "task has no tile store").into_response(),
    };

    // ── 选取合适的缩放级别 ───────────────────────────────────────────────────
    let lon_range = (max_lon - min_lon).max(1e-6);
    let z_f = (width as f64 / 256.0 * 360.0 / lon_range).log2();
    let z = (z_f.round() as i32)
        .max(task.min_zoom as i32)
        .min(task.max_zoom as i32)
        .max(0) as u32;
    let n = (1u64 << z) as f64;

    // ── 计算瓦片范围（XYZ 约定，Y=0 在北）────────────────────────────────────
    let merc_top = mercator_y(max_lat);
    let merc_bottom = mercator_y(min_lat);
    let merc_range = (merc_top - merc_bottom).max(1e-10);

    let tx_min = ((min_lon + 180.0) / 360.0 * n).floor() as i64;
    let tx_max = ((max_lon + 180.0) / 360.0 * n).ceil() as i64;
    let ty_min = ((1.0 - merc_top / PI) / 2.0 * n).floor() as i64;
    let ty_max = ((1.0 - merc_bottom / PI) / 2.0 * n).ceil() as i64;

    let tx_min = tx_min.max(0);
    let tx_max = tx_max.min(n as i64);
    let ty_min = ty_min.max(0);
    let ty_max = ty_max.min(n as i64);

    let tiles: Vec<(i64, i64)> = (ty_min..ty_max)
        .flat_map(|ty| (tx_min..tx_max).map(move |tx| (tx, ty)))
        .collect();

    if tiles.len() > 512 {
        return (
            StatusCode::BAD_REQUEST,
            "BBOX covers too many tiles; reduce area or increase zoom",
        )
            .into_response();
    }

    // ── 在线程池中执行同步 IO 与图像合成 ─────────────────────────────────────
    let result = tokio::task::spawn_blocking(move || {
        use image::{ImageEncoder, RgbaImage};

        let conn = Connection::open(&tile_store_path)?;
        let mut out = RgbaImage::new(width, height);

        for (tx, ty) in tiles {
            let mut stmt = conn.prepare(
                "SELECT tile_data FROM tiles WHERE tile_column=?1 AND tile_row=?2 AND zoom_level=?3",
            )?;
            let mut rows = stmt.query(params![tx, ty, z as i64])?;

            if let Some(row) = rows.next()? {
                let tile_bytes: Vec<u8> = row.get(0)?;

                if let Ok(tile_img) = image::load_from_memory(&tile_bytes) {
                    let tile_rgba = tile_img.to_rgba8();

                    // 瓦片边界（Mercator Y）
                    let tile_merc_top    = (1.0 - 2.0 * ty       as f64 / n) * PI;
                    let tile_merc_bottom = (1.0 - 2.0 * (ty + 1) as f64 / n) * PI;

                    // 瓦片边界（经度）
                    let tile_lon_left  = tx       as f64 / n * 360.0 - 180.0;
                    let tile_lon_right = (tx + 1) as f64 / n * 360.0 - 180.0;

                    // 映射到输出图像像素坐标
                    let ox_left   = ((tile_lon_left  - min_lon) / lon_range * width  as f64).round() as i64;
                    let ox_right  = ((tile_lon_right - min_lon) / lon_range * width  as f64).round() as i64;
                    let oy_top    = ((merc_top - tile_merc_top)    / merc_range * height as f64).round() as i64;
                    let oy_bottom = ((merc_top - tile_merc_bottom) / merc_range * height as f64).round() as i64;

                    let tw = (ox_right  - ox_left).max(1) as u32;
                    let th = (oy_bottom - oy_top ).max(1) as u32;

                    // 缩放瓦片到目标尺寸
                    let resized = image::imageops::resize(
                        &tile_rgba,
                        tw, th,
                        image::imageops::FilterType::Triangle,
                    );

                    // 逐像素写入输出图像（超出边界的像素直接丢弃）
                    for (rx, ry, pixel) in resized.enumerate_pixels() {
                        let ox = ox_left + rx as i64;
                        let oy = oy_top  + ry as i64;
                        if ox >= 0 && oy >= 0 && ox < width as i64 && oy < height as i64 {
                            out.put_pixel(ox as u32, oy as u32, *pixel);
                        }
                    }
                }
            }
        }

        // 编码为 PNG
        let mut buf: Vec<u8> = Vec::new();
        image::codecs::png::PngEncoder::new(&mut buf).write_image(
            out.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )?;

        Ok::<Vec<u8>, anyhow::Error>(buf)
    })
    .await;

    match result {
        Ok(Ok(png)) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=3600"),
            );
            (headers, Bytes::from(png)).into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 经纬度 → Mercator Y（弧度单位，与 Web Mercator 瓦片计算一致）
fn mercator_y(lat_deg: f64) -> f64 {
    let lat_rad = lat_deg.to_radians();
    (lat_rad.tan() + 1.0 / lat_rad.cos()).ln()
}

/// EPSG:3857 (meters) → WGS84 (lon, lat degrees)
fn merc_to_wgs84(x: f64, y: f64) -> (f64, f64) {
    let lon = x / 20_037_508.342_789_244 * 180.0;
    let lat = (PI * y / 20_037_508.342_789_244).sinh().atan().to_degrees();
    (lon, lat)
}

// ─── REST /api/stats ──────────────────────────────────────────────────────────

/// 获取各任务的服务请求统计  GET /api/stats
pub async fn api_stats(State(state): State<ServerAppState>) -> Response {
    match state.stats.lock() {
        Ok(map) => {
            let data: std::collections::HashMap<_, _> = map.iter().collect();
            json_response(&data)
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "stats mutex poisoned").into_response(),
    }
}

// ─── OGC API Tiles ────────────────────────────────────────────────────────────

/// 集合信息  GET /ogc-tiles/{task_id}
pub async fn ogc_tiles_collection(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
) -> Response {
    let task = match state.app_db.get_task(&task_id) {
        Ok(t) => t,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let base = &state.base_url;
    let info = serde_json::json!({
        "id": task_id,
        "title": task.name,
        "extent": {
            "spatial": {
                "bbox": [[task.bounds_west, task.bounds_south, task.bounds_east, task.bounds_north]],
                "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
            }
        },
        "links": [
            { "rel": "self", "type": "application/json", "href": format!("{base}/ogc-tiles/{task_id}") },
            { "rel": "tiles", "type": "application/json", "href": format!("{base}/ogc-tiles/{task_id}/tiles/WebMercatorQuad") }
        ]
    });
    json_response(&info)
}

/// Tileset 元数据  GET /ogc-tiles/{task_id}/tiles/WebMercatorQuad
pub async fn ogc_tiles_tileset(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
) -> Response {
    let task = match state.app_db.get_task(&task_id) {
        Ok(t) => t,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let base = &state.base_url;
    let tile_url = format!(
        "{base}/ogc-tiles/{task_id}/tiles/WebMercatorQuad/{{tileMatrix}}/{{tileRow}}/{{tileCol}}"
    );
    let info = serde_json::json!({
        "tileMatrixSetURI": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
        "dataType": "map",
        "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
        "tileMatrixSetLimits": (task.min_zoom..=task.max_zoom).map(|z| {
            let n = 1i64 << z;
            serde_json::json!({
                "tileMatrix": z.to_string(),
                "minTileRow": 0, "maxTileRow": n - 1,
                "minTileCol": 0, "maxTileCol": n - 1
            })
        }).collect::<Vec<_>>(),
        "links": [
            { "rel": "self", "type": "application/json", "href": format!("{base}/ogc-tiles/{task_id}/tiles/WebMercatorQuad") },
            { "rel": "item", "type": "image/png", "href": tile_url }
        ]
    });
    json_response(&info)
}

/// 单瓦片  GET /ogc-tiles/{task_id}/tiles/WebMercatorQuad/{z}/{row}/{col}
/// OGC 约定：路径为 {tileMatrix}/{tileRow}/{tileCol}，即 z/y/x
pub async fn ogc_tiles_tile(
    State(state): State<ServerAppState>,
    Path((task_id, z, row, col)): Path<(String, i64, i64, i64)>,
) -> Response {
    // OGC tileRow = Y（从北向南），tileCol = X；与内部存储一致
    update_stats(&state.stats, &task_id, "ogc");
    serve_tile(&state, task_id, z, col, row).await
}

// ─── ArcGIS REST API 兼容 ─────────────────────────────────────────────────────

#[derive(serde::Deserialize, Default)]
pub struct ArcGisParams {
    #[serde(rename = "f")]
    pub format: Option<String>,
}

/// MapServer 服务信息  GET /arcgis/rest/services/{task_id}/MapServer
pub async fn arcgis_mapserver(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
    Query(_params): Query<ArcGisParams>,
) -> Response {
    let task = match state.app_db.get_task(&task_id) {
        Ok(t) => t,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let base = &state.base_url;

    // 生成 LOD 列表（ArcGIS 标准 Web Mercator 比例尺）
    let scale_base: f64 = 591_657_527.591_555;
    let lods: Vec<serde_json::Value> = (task.min_zoom..=task.max_zoom)
        .map(|z| {
            serde_json::json!({
                "level": z,
                "resolution": 156_543.033_928 / (1 << z) as f64,
                "scale": scale_base / (1 << z) as f64
            })
        })
        .collect();

    let info = serde_json::json!({
        "currentVersion": 10.81,
        "serviceDescription": task.name,
        "mapName": task.name,
        "description": "",
        "copyrightText": "",
        "supportsDynamicLayers": false,
        "layers": [],
        "tables": [],
        "spatialReference": { "wkid": 3857, "latestWkid": 3857 },
        "singleFusedMapCache": true,
        "tileInfo": {
            "rows": 256, "cols": 256, "dpi": 96,
            "format": "PNG",
            "compressionQuality": 75,
            "origin": { "x": -20037508.342787, "y": 20037508.342787 },
            "spatialReference": { "wkid": 3857, "latestWkid": 3857 },
            "lods": lods
        },
        "initialExtent": {
            "xmin": task.bounds_west, "ymin": task.bounds_south,
            "xmax": task.bounds_east, "ymax": task.bounds_north,
            "spatialReference": { "wkid": 4326, "latestWkid": 4326 }
        },
        "fullExtent": {
            "xmin": task.bounds_west, "ymin": task.bounds_south,
            "xmax": task.bounds_east, "ymax": task.bounds_north,
            "spatialReference": { "wkid": 4326, "latestWkid": 4326 }
        },
        "minScale": scale_base / (1 << task.min_zoom) as f64,
        "maxScale": scale_base / (1 << task.max_zoom) as f64,
        "units": "esriMeters",
        "supportedImageFormatTypes": "PNG,PNG32,PNG24,PNG8",
        "documentInfo": { "Title": task.name },
        "capabilities": "Map,TilesOnly",
        "tileServers": [],
        "exportTilesAllowed": false,
        "maxExportTilesCount": 0,
        "resampling": false,
        "url": format!("{base}/arcgis/rest/services/{task_id}/MapServer")
    });
    json_response(&info)
}

/// 瓦片端点  GET /arcgis/rest/services/{task_id}/MapServer/tile/{z}/{row}/{col}
/// ArcGIS 约定：路径为 tile/{level}/{row}/{col}，即 z/y/x
pub async fn arcgis_tile(
    State(state): State<ServerAppState>,
    Path((task_id, z, row, col)): Path<(String, i64, i64, i64)>,
) -> Response {
    // ArcGIS row = Y（北向下），col = X；与内部 XYZ 存储一致
    update_stats(&state.stats, &task_id, "arcgis");
    serve_tile(&state, task_id, z, col, row).await
}

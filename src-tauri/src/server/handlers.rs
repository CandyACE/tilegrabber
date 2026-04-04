//! axum 请求处理器
//!
//! 包含：
//! - `tms_tile`        — TMS /tiles/{task}/{z}/{x}/{y}
//! - `wmts_dispatch`   — WMTS 单入口（GetCapabilities / GetTile）
//! - `api_tasks`       — GET /api/tasks  (JSON 列表)

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, params};

use super::ServerAppState;

// ─── TMS 单瓦片 ─────────────────────────────────────────────────────────────

pub async fn tms_tile(
    State(state): State<ServerAppState>,
    Path((task_id, z, x, y)): Path<(String, i64, i64, i64)>,
) -> Response {
    // 从 app_db 查询 tile_store_path
    let tile_store_path = match state.app_db.get_task(&task_id) {
        Ok(t) => match t.tile_store_path {
            Some(p) => p,
            None => return (StatusCode::NOT_FOUND, "task tile_store_path is null").into_response(),
        },
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    // 读取瓦片数据（阻塞 IO 放到 spawn_blocking）
    let result = tokio::task::spawn_blocking(move || {
        read_tile_from_store(&tile_store_path, z, x, y)
    })
    .await;

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
    let request = params
        .request
        .as_deref()
        .unwrap_or("GetCapabilities")
        .to_ascii_uppercase();

    match request.as_str() {
        "GETCAPABILITIES" | "GETMAP" => {
            wmts_get_capabilities(&state, &task_id).await
        }
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
    tms_tile(State(state), Path((task_id, z, col, row))).await
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


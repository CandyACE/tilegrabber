import type { TileSource } from '~/types/tile-source'

export type BasemapCategoryId = 'amap' | 'tencent' | 'google' | 'bing' | 'arcgis' | 'osm' | 'tianditu'

export interface BasemapPreset {
  id: string
  category: BasemapCategoryId
  name: string
  /** 选中此预设时需要的用户凭证（例如天地图 tk）。前端会弹层让用户填入，并存到 `extra_params[tokenKey]`。 */
  requiresToken?: {
    /** extra_params 的键名（URL 模板里写 `{{key}}`） */
    tokenKey: string
    /** 应用设置里持久化保存的 key */
    settingKey: string
    /** 提示文案（i18n key 优先，否则原文） */
    label: string
    /** 申请链接 */
    helpUrl?: string
  }
  source: Omit<TileSource, 'name'>
}

export interface BasemapCategory {
  id: BasemapCategoryId
  label: string
}

const WORLD_BOUNDS = { west: -180, east: 180, south: -85.051129, north: 85.051129 }

function tms(partial: Partial<Omit<TileSource, 'name'>>): Omit<TileSource, 'name'> {
  return {
    kind: 'tms',
    url_template: '',
    url_param_order: [],
    subdomains: [],
    crs: 'WEB_MERCATOR',
    coord_type: 'WGS84',
    tile_size: 256,
    north_to_south: true,
    bounds: WORLD_BOUNDS,
    min_zoom: 1,
    max_zoom: 18,
    headers: {},
    extra_params: {},
    param_scripts: {},
    attribution: null,
    format: 'png',
    ...partial,
  }
}

export const BASEMAP_CATEGORIES: BasemapCategory[] = [
  { id: 'amap', label: '高德地图' },
  { id: 'tencent', label: '腾讯地图' },
  { id: 'tianditu', label: '天地图' },
  { id: 'google', label: 'Google Maps' },
  { id: 'bing', label: 'Bing Maps' },
  { id: 'arcgis', label: 'ArcGIS' },
  { id: 'osm', label: 'OpenStreetMap' },
]

export const BASEMAP_PRESETS: BasemapPreset[] = [
  // ── 高德地图 ──────────────────────────────────────────────────────────────────
  {
    id: 'amap-road',
    category: 'amap',
    name: '路网图',
    source: tms({
      url_template:
        'https://webrd0{s}.is.autonavi.com/appmaptile?lang=zh_cn&size=1&scale=1&style=8&x={x}&y={y}&z={z}',
      subdomains: ['1', '2', '3', '4'],
      coord_type: 'GCJ02',
      max_zoom: 18,
      format: 'png',
      attribution: '© 高德地图',
    }),
  },
  {
    id: 'amap-satellite',
    category: 'amap',
    name: '卫星图',
    source: tms({
      url_template: 'https://webst0{s}.is.autonavi.com/appmaptile?style=6&x={x}&y={y}&z={z}',
      subdomains: ['1', '2', '3', '4'],
      coord_type: 'GCJ02',
      max_zoom: 18,
      format: 'jpg',
      attribution: '© 高德地图',
    }),
  },
  {
    id: 'amap-satellite-label',
    category: 'amap',
    name: '卫星注记叠加',
    source: tms({
      url_template: 'https://webst0{s}.is.autonavi.com/appmaptile?style=8&x={x}&y={y}&z={z}',
      subdomains: ['1', '2', '3', '4'],
      coord_type: 'GCJ02',
      max_zoom: 18,
      format: 'png',
      attribution: '© 高德地图',
    }),
  },
  // ── 腾讯地图 ──────────────────────────────────────────────────────────────────
  {
    id: 'tencent-road',
    category: 'tencent',
    name: '路网图',
    source: tms({
      url_template: 'https://rt{s}.map.gtimg.com/tile?z={z}&x={x}&y={y}&styleid=1&version=297',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'GCJ02',
      north_to_south: false,
      max_zoom: 18,
      format: 'png',
      attribution: '© 腾讯地图',
    }),
  },
  {
    id: 'tencent-terrain',
    category: 'tencent',
    name: '地形图',
    source: tms({
      url_template: 'https://rt{s}.map.gtimg.com/tile?z={z}&x={x}&y={y}&styleid=4&version=297',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'GCJ02',
      north_to_south: false,
      max_zoom: 18,
      format: 'png',
      attribution: '© 腾讯地图',
    }),
  },
  // ── 天地图（需 token）─────────────────────────────────────────────────────────
  // URL 末尾的 {{tk}} 在选中预设时由用户填入的 token 替换
  ...((): BasemapPreset[] => {
    const TIANDITU_SUBDOMAINS = ['0', '1', '2', '3', '4', '5', '6', '7']
    const TIANDITU_HOST = 'https://t{s}.tianditu.gov.cn'
    const tdt = (layer: string, fmt: 'png' | 'jpg' = 'png'): Omit<TileSource, 'name'> =>
      tms({
        url_template: `${TIANDITU_HOST}/${layer}_w/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=${layer}&STYLE=default&TILEMATRIXSET=w&FORMAT=tiles&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}&tk={{tk}}`,
        subdomains: TIANDITU_SUBDOMAINS,
        coord_type: 'WGS84',
        max_zoom: 18,
        format: fmt,
        attribution: '© 国家天地图',
      })
    const token = {
      tokenKey: 'tk',
      settingKey: 'preset.tianditu_token',
      label: '天地图 tk',
      helpUrl: 'https://console.tianditu.gov.cn/api/key',
    } as const
    return [
      { id: 'tianditu-vec',  category: 'tianditu', name: '矢量底图',   requiresToken: token, source: tdt('vec') },
      { id: 'tianditu-cva',  category: 'tianditu', name: '矢量注记',   requiresToken: token, source: tdt('cva') },
      { id: 'tianditu-img',  category: 'tianditu', name: '影像底图',   requiresToken: token, source: tdt('img', 'jpg') },
      { id: 'tianditu-cia',  category: 'tianditu', name: '影像注记',   requiresToken: token, source: tdt('cia') },
      { id: 'tianditu-ter',  category: 'tianditu', name: '地形晕渲',   requiresToken: token, source: tdt('ter', 'jpg') },
      { id: 'tianditu-cta',  category: 'tianditu', name: '地形注记',   requiresToken: token, source: tdt('cta') },
    ]
  })(),
  // ── Google Maps ───────────────────────────────────────────────────────────────
  {
    id: 'google-road',
    category: 'google',
    name: '路网图',
    source: tms({
      url_template: 'https://mt{s}.google.com/vt/lyrs=m&hl=zh-CN&x={x}&y={y}&z={z}',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 20,
      format: 'png',
      attribution: '© Google',
    }),
  },
  {
    id: 'google-satellite',
    category: 'google',
    name: '卫星图',
    source: tms({
      url_template: 'https://mt{s}.google.com/vt/lyrs=s&hl=zh-CN&x={x}&y={y}&z={z}',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 20,
      format: 'jpg',
      attribution: '© Google',
    }),
  },
  {
    id: 'google-hybrid',
    category: 'google',
    name: '混合图',
    source: tms({
      url_template: 'https://mt{s}.google.com/vt/lyrs=y&hl=zh-CN&x={x}&y={y}&z={z}',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 20,
      format: 'jpg',
      attribution: '© Google',
    }),
  },
  {
    id: 'google-terrain',
    category: 'google',
    name: '地形图',
    source: tms({
      url_template: 'https://mt{s}.google.com/vt/lyrs=p&hl=zh-CN&x={x}&y={y}&z={z}',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 15,
      format: 'png',
      attribution: '© Google',
    }),
  },
  // ── Bing Maps ─────────────────────────────────────────────────────────────────
  // 使用 {q} quadkey 占位符（worker 内置）
  {
    id: 'bing-aerial',
    category: 'bing',
    name: 'Aerial',
    source: tms({
      url_template: 'https://ecn.t{s}.tiles.virtualearth.net/tiles/a{q}.jpeg?g=1',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'jpg',
      attribution: '© Microsoft, DigitalGlobe',
    }),
  },
  {
    id: 'bing-road',
    category: 'bing',
    name: 'Road',
    source: tms({
      url_template: 'https://ecn.t{s}.tiles.virtualearth.net/tiles/r{q}.png?g=1',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'png',
      attribution: '© Microsoft, HERE',
    }),
  },
  {
    id: 'bing-hybrid',
    category: 'bing',
    name: 'Hybrid',
    source: tms({
      url_template: 'https://ecn.t{s}.tiles.virtualearth.net/tiles/h{q}.jpeg?g=1',
      subdomains: ['0', '1', '2', '3'],
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'jpg',
      attribution: '© Microsoft, DigitalGlobe',
    }),
  },
  // ── ArcGIS ────────────────────────────────────────────────────────────────────
  {
    id: 'arcgis-imagery',
    category: 'arcgis',
    name: 'World Imagery',
    source: tms({
      url_template:
        'https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}',
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'jpg',
      attribution: '© Esri, Maxar, Earthstar Geographics',
    }),
  },
  {
    id: 'arcgis-street',
    category: 'arcgis',
    name: 'World Street Map',
    source: tms({
      url_template:
        'https://server.arcgisonline.com/ArcGIS/rest/services/World_Street_Map/MapServer/tile/{z}/{y}/{x}',
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'png',
      attribution: '© Esri, HERE, Garmin',
    }),
  },
  {
    id: 'arcgis-topo',
    category: 'arcgis',
    name: 'World Topo Map',
    source: tms({
      url_template:
        'https://server.arcgisonline.com/ArcGIS/rest/services/World_Topo_Map/MapServer/tile/{z}/{y}/{x}',
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'png',
      attribution: '© Esri, HERE, Garmin',
    }),
  },
  {
    id: 'arcgis-dark',
    category: 'arcgis',
    name: 'World Dark Gray',
    source: tms({
      url_template:
        'https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/{z}/{y}/{x}',
      coord_type: 'WGS84',
      max_zoom: 16,
      format: 'png',
      attribution: '© Esri',
    }),
  },
  // ── OpenStreetMap ─────────────────────────────────────────────────────────────
  {
    id: 'osm-standard',
    category: 'osm',
    name: 'Standard',
    source: tms({
      url_template: 'https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png',
      subdomains: ['a', 'b', 'c'],
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'png',
      attribution: '© OpenStreetMap contributors',
    }),
  },
  {
    id: 'osm-humanitarian',
    category: 'osm',
    name: 'Humanitarian',
    source: tms({
      url_template: 'https://{s}.tile.openstreetmap.fr/hot/{z}/{x}/{y}.png',
      subdomains: ['a', 'b'],
      coord_type: 'WGS84',
      max_zoom: 19,
      format: 'png',
      attribution: '© HOT, © OpenStreetMap contributors',
    }),
  },
  {
    id: 'osm-cyclosm',
    category: 'osm',
    name: 'CyclOSM',
    source: tms({
      url_template: 'https://{s}.tile-cyclosm.openstreetmap.fr/cyclosm/{z}/{x}/{y}.png',
      subdomains: ['a', 'b', 'c'],
      coord_type: 'WGS84',
      max_zoom: 20,
      format: 'png',
      attribution: '© CyclOSM, © OpenStreetMap contributors',
    }),
  },
  {
    id: 'osm-opentopomap',
    category: 'osm',
    name: 'OpenTopoMap',
    source: tms({
      url_template: 'https://{s}.tile.opentopomap.org/{z}/{x}/{y}.png',
      subdomains: ['a', 'b', 'c'],
      coord_type: 'WGS84',
      max_zoom: 17,
      format: 'png',
      attribution: '© OpenTopoMap, © OpenStreetMap contributors',
    }),
  },
]

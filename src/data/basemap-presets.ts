import type { TileSource } from '~/types/tile-source'

export type BasemapCategoryId = 'amap' | 'google' | 'arcgis' | 'osm'

export interface BasemapPreset {
  id: string
  category: BasemapCategoryId
  name: string
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
  { id: 'google', label: 'Google Maps' },
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
]

import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import DownloadSetupPanel from "./DownloadSetupPanel.vue";
import { i18n } from "~/i18n";
import type { TileSource } from "~/types/tile-source";
import type { DownloadArea } from "~/lib/download-area";

// 桌面文件选择和解析由边界桩替代，验证配置面板到下载提交的关键链路。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const source: TileSource = {
  kind: "tms", name: "测试图源", url_template: "https://example.com/{z}/{x}/{y}.png",
  url_param_order: ["z", "x", "y"], subdomains: [], crs: "WEB_MERCATOR", coord_type: "WGS84",
  tile_size: 256, north_to_south: true, bounds: { west: -180, east: 180, south: -85, north: 85 }, min_zoom: 4, max_zoom: 6,
  headers: {}, extra_params: {}, param_scripts: {}, attribution: null, format: "png",
};
const west: DownloadArea = { west: 100, east: 101, south: 30, north: 31, polygons: null };
const east: DownloadArea = { west: 110, east: 111, south: 30, north: 31, polygons: null };
let wrapper: VueWrapper;
function setup(bounds: DownloadArea | null = west) {
  wrapper = mount(DownloadSetupPanel, {
    props: { source, bounds, drawActive: false },
    global: { plugins: [i18n], stubs: { ZoomPicker: true } },
  });
  return wrapper;
}
function button(text: string) {
  const result = wrapper.findAll("button").find(b => b.text() === text);
  if (!result) throw new Error(`未找到按钮：${text}`);
  return result;
}
function checkbox(label: string) {
  const result = wrapper.findAll("label").find(l => l.text() === label);
  if (!result) throw new Error(`未找到复选项：${label}`);
  return result.get("input");
}

describe("区域与层级组合下载", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    i18n.global.locale.value = "zh-CN";
    vi.mocked(invoke).mockResolvedValue({ tile_count: 10, estimated_bytes: 1000 });
  });
  afterEach(() => { wrapper?.unmount(); vi.useRealTimers(); });

  it("一次选择多个文件并追加已有矩形，全部成功后只更新一次范围", async () => {
    vi.mocked(open).mockResolvedValue(["west.json", "east.json"]);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "parse_area_file") return (args as { path: string }).path === "west.json" ? west : east;
      return null;
    });
    setup();
    await button("导入").trigger("click");
    await flushPromises();
    expect(open).toHaveBeenCalledWith(expect.objectContaining({ multiple: true }));
    const events = wrapper.emitted("import-bounds")!;
    expect(events).toHaveLength(1);
    expect(events[0][0]).toMatchObject({ west: 100, east: 111, south: 30, north: 31 });
    expect(events[0][1]).toHaveLength(3);
  });

  it("任一文件解析失败时保留原范围，允许重试", async () => {
    vi.mocked(open).mockResolvedValue(["west.json", "broken.json"]);
    vi.mocked(invoke).mockImplementation(async (_command, args) => {
      if ((args as { path: string }).path === "broken.json") throw new Error("区域文件损坏");
      return west;
    });
    setup();
    await button("导入").trigger("click");
    await flushPromises();
    expect(wrapper.emitted("import-bounds")).toBeUndefined();
    expect(wrapper.text()).toContain("区域文件损坏");
    expect(button("导入").attributes("disabled")).toBeUndefined();
  });

  it("离散层级跳过中间级别，全部取消时阻止提交", async () => {
    setup();
    await checkbox("勾选下载层级（可不连续）").setValue(true);
    await checkbox("Z5").setValue(false);
    await button("开始下载").trigger("click");
    expect(wrapper.emitted("start-download")?.[0]?.[0]).toMatchObject({ minZoom: 4, maxZoom: 6, zoomLevels: [4, 6] });
    await checkbox("Z4").setValue(false);
    await checkbox("Z6").setValue(false);
    expect(button("开始下载").attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("请至少勾选一个层级");
  });

  it("使用下拉框选择省份，并通过追加合并多个区域", async () => {
    setup(null);
    await vi.dynamicImportSettled();
    await flushPromises();
    expect(wrapper.findAll("select")).toHaveLength(3);
    expect(wrapper.text()).not.toContain("纠偏");
    const province = wrapper.get('select[aria-label="省份 / 直辖市"]');
    await province.setValue("北京市");
    await button("应用所选行政区").trigger("click");
    const first = wrapper.emitted("import-bounds")![0][0] as DownloadArea;
    // 模拟主窗口更新选区，再选择第二个省份，保留原来的多区域追加能力。
    await wrapper.setProps({ bounds: first, polygons: first.polygons });
    await province.setValue("上海市");
    await button("应用所选行政区").trigger("click");
    const merged = wrapper.emitted("import-bounds")![1][0] as DownloadArea;
    expect(merged.west).toBeLessThan(117);
    expect(merged.east).toBeGreaterThan(121);
    expect(merged.polygons!.length).toBeGreaterThan(first.polygons!.length);
  });

  it("省市联动仅应用所选城市，换省时清除旧城市", async () => {
    setup(null);
    await vi.dynamicImportSettled();
    await flushPromises();
    const province = wrapper.get('select[aria-label="省份 / 直辖市"]');
    const city = wrapper.get('select[aria-label="城市 / 自治州"]');
    expect(city.attributes("disabled")).toBeDefined();
    await province.setValue("江苏省");
    expect(city.text()).toContain("苏州市");
    expect(city.text()).toContain("南京市");
    await city.setValue("320500");
    await button("应用所选行政区").trigger("click");
    const suzhou = wrapper.emitted("import-bounds")![0][0] as DownloadArea;
    // 苏州边界包含市中心，且不应包含南京或江苏北部的范围。
    expect(suzhou.west).toBeLessThan(120.6);
    expect(suzhou.east).toBeGreaterThan(120.6);
    expect(suzhou.south).toBeLessThan(31.3);
    expect(suzhou.north).toBeGreaterThan(31.3);
    expect(suzhou.west).toBeGreaterThan(119);
    expect(suzhou.north).toBeLessThan(33);
    await province.setValue("浙江省");
    expect((city.element as HTMLSelectElement).value).toBe("");
    expect(city.text()).not.toContain("苏州市");
    await button("应用所选行政区").trigger("click");
    const zhejiang = wrapper.emitted("import-bounds")![1][0] as DownloadArea;
    expect(zhejiang.south).toBeLessThan(28);
    expect(zhejiang.polygons!.length).toBeGreaterThan(1);
  });

  it("支持全国和无市级数据区域的完整范围选择", async () => {
    setup(null);
    await vi.dynamicImportSettled();
    await flushPromises();
    await button("应用所选行政区").trigger("click");
    const country = wrapper.emitted("import-bounds")![0][0] as DownloadArea;
    expect(country.west).toBeLessThan(80);
    expect(country.east).toBeGreaterThan(130);
    await wrapper.get('select[aria-label="省份 / 直辖市"]').setValue("台湾省");
    expect(wrapper.get('select[aria-label="城市 / 自治州"]').attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("该区域暂无市级边界");
    expect(button("应用所选行政区").attributes("disabled")).toBeUndefined();
    await button("应用所选行政区").trigger("click");
    const taiwan = wrapper.emitted("import-bounds")![1][0] as DownloadArea;
    expect(taiwan.west).toBeGreaterThan(110);
    expect(taiwan.east).toBeLessThan(125);
  });

  it("参数改变后丢弃过期估算，保留最新请求结果", async () => {
    vi.useFakeTimers();
    let finishOld!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; }))
      .mockResolvedValue({ tile_count: 20, estimated_bytes: 2000 });
    setup();
    await vi.advanceTimersByTimeAsync(400);
    await wrapper.setProps({ bounds: east });
    await vi.advanceTimersByTimeAsync(400);
    expect(wrapper.text()).toContain("包围框估算 ≤ 20 块");
    finishOld({ tile_count: 999, estimated_bytes: 9999 });
    await flushPromises();
    expect(wrapper.text()).toContain("包围框估算 ≤ 20 块");
    expect(wrapper.text()).not.toContain("999");
  });
});

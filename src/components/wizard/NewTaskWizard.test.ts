import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import NewTaskWizard from "./NewTaskWizard.vue";
import { i18n } from "~/i18n";
import type { TileSource } from "~/types/tile-source";

// 隔离 Tauri 后端调用，只验证 TMS 导入表单到最终图源配置的前端链路。
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// 文件选择器不属于本测试范围，使用空实现避免访问桌面运行时。
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const parsedTmsSource: TileSource = {
  kind: "tms",
  name: "Google 影像",
  url_template:
    "https://mt0.google.com/vt/lyrs=s&scale=2&x={x}&y={y}&z={z}&s=Galile&gl=cn",
  url_param_order: ["x", "y", "z"],
  subdomains: [],
  crs: "WEB_MERCATOR",
  coord_type: "WGS84",
  tile_size: 256,
  north_to_south: true,
  bounds: { west: -180, east: 180, south: -85, north: 85 },
  min_zoom: 0,
  max_zoom: 20,
  headers: {},
  extra_params: {},
  param_scripts: {},
  attribution: null,
  format: "jpg",
};

describe("NewTaskWizard TMS 坐标类型", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // 固定中文界面，确保按钮与选项断言不受测试机系统语言影响。
    i18n.global.locale.value = "zh-CN";
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "parse_tms_url") return parsedTmsSource;
      return null;
    });
  });

  it("可在 TMS 导入表单选择 GCJ02，并写入确认的图源配置", async () => {
    const wrapper = mount(NewTaskWizard, {
      global: {
        plugins: [i18n],
        // 动画不影响业务状态，测试中替换为同步渲染以减少无关等待。
        stubs: { Transition: false },
      },
    });

    // 切换到 TMS/XYZ 来源后，坐标类型选择器应直接显示在导入区域。
    const tmsButton = wrapper
      .findAll("button")
      .find((button) => button.text().includes("TMS/XYZ URL"));
    expect(tmsButton).toBeDefined();
    await tmsButton!.trigger("click");

    const coordSelect = wrapper.get('[data-testid="tms-coord-type"]');
    await coordSelect.setValue("GCJ02");

    const urlInput = wrapper.get(
      'input[placeholder="https://tile.openstreetmap.org/{z}/{x}/{y}.png"]',
    );
    await urlInput.setValue(parsedTmsSource.url_template);

    const parseButton = wrapper
      .findAll("button")
      .find((button) => button.text().includes("解析"));
    expect(parseButton).toBeDefined();
    await parseButton!.trigger("click");
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith("parse_tms_url", {
      url: parsedTmsSource.url_template,
      name: null,
    });
    const confirmed = wrapper.emitted("confirm")?.[0]?.[0] as TileSource;
    expect(confirmed.coord_type).toBe("GCJ02");
  });
});

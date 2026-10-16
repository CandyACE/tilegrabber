import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import MapContainer from "./MapContainer.vue";
import { i18n } from "~/i18n";

const createMap = vi.hoisted(() => vi.fn());
vi.mock("maplibre-gl", () => ({ default: { Map: createMap } }));

// 无任务时地图仍会初始化；图形环境不可用应显示错误并支持重试。
describe("空任务地图初始化", () => {
  it("WebGL 异常保留应用内错误提示和重试入口", async () => {
    createMap.mockImplementation(() => { throw new Error("WebGL unavailable"); });
    i18n.global.locale.value = "zh-CN";
    const wrapper = mount(MapContainer, { global: { plugins: [i18n] } });
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("WebGL unavailable");
    await wrapper.get("button").trigger("click");
    expect(createMap).toHaveBeenCalledTimes(2);
    expect(wrapper.emitted("ready")).toBeUndefined();
    wrapper.unmount();
  });
});

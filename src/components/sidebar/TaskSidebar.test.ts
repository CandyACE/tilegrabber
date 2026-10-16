import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import TaskSidebar from "./TaskSidebar.vue";
import { i18n } from "~/i18n";

// 空任务列表不依赖原生事件或导出过程，只检验渲染和刷新后的交互可用性。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn(), confirm: vi.fn() }));

describe("无下载任务的侧栏", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    i18n.global.locale.value = "zh-CN";
    vi.mocked(invoke).mockResolvedValue([]);
  });
  it("首次加载和清空后刷新都展示空态，仍可创建任务", async () => {
    const wrapper = mount(TaskSidebar, { props: { open: true }, global: { plugins: [i18n] } });
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith("list_tasks");
    expect(wrapper.text()).toContain(i18n.global.t("tasks.emptyTitle"));
    // 模拟创建唯一任务后再删除，确认同一组件能恢复空态。
    vi.mocked(invoke).mockResolvedValueOnce([{
      id: "only", name: "唯一任务", sourceConfig: "{}", status: "pending",
      minZoom: 4, maxZoom: 6, totalTiles: 0, downloadedTiles: 0, failedTiles: 0,
      createdAt: "2026-10-15", tileStorePath: null,
    }]);
    const refresh = () => (wrapper.vm as unknown as { loadTasks: () => Promise<void> }).loadTasks();
    await refresh();
    await flushPromises();
    expect(wrapper.text()).toContain("唯一任务");
    await refresh();
    await flushPromises();
    expect(wrapper.text()).toContain(i18n.global.t("tasks.emptyTitle"));
    const create = wrapper.findAll("button").find(b => b.text().includes(i18n.global.t("tasks.createTask")));
    expect(create).toBeDefined();
    await create!.trigger("click");
    expect(wrapper.emitted("new-task")).toHaveLength(1);
    wrapper.unmount();
  });
});

import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import TaskNameEditor from "./TaskNameEditor.vue";
import { i18n } from "~/i18n";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

// 只覆盖重命名提交的关键链路，数据库持久化由 Rust 测试验证。
describe("任务重命名", () => {
  beforeEach(() => { vi.clearAllMocks(); i18n.global.locale.value = "zh-CN"; });
  it("去除首尾空白后保存，并通知任务列表更新", async () => {
    vi.mocked(invoke).mockResolvedValue("新名称");
    const wrapper = mount(TaskNameEditor, { props: { taskId: "one", name: "原名称" }, global: { plugins: [i18n] } });
    await wrapper.get("button").trigger("click");
    await wrapper.get("input").setValue("  新名称  ");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith("rename_task", { taskId: "one", name: "新名称" });
    expect(wrapper.emitted("saved")).toEqual([["新名称"]]);
  });
  it("拒绝空白，并在后端失败时保留输入供重试", async () => {
    const wrapper = mount(TaskNameEditor, { props: { taskId: "one", name: "原名称" }, global: { plugins: [i18n] } });
    await wrapper.get("button").trigger("click");
    await wrapper.get("input").setValue("  ");
    await wrapper.get("form").trigger("submit");
    expect(invoke).not.toHaveBeenCalled();
    expect(wrapper.get('[role="alert"]').text()).toContain("1 至 200");
    vi.mocked(invoke).mockRejectedValue(new Error("写入失败"));
    await wrapper.get("input").setValue("新名称");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get("input").element.value).toBe("新名称");
    expect(wrapper.emitted("saved")).toBeUndefined();
    expect(wrapper.get('[role="alert"]').text()).toContain("写入失败");
  });
});

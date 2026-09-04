import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
}));

// 更新检查由测试提供带私有字段的仿真句柄，避免访问真实 Tauri 运行时。
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: mocks.check,
}));

// 本测试不初始化后台事件监听器，仅提供模块加载所需的最小接口。
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

import {
  clearPendingUpdate,
  getPendingUpdate,
  runUpdateCheck,
} from "./useUpdateState";

/**
 * 仿真 Tauri Resource 的私有字段行为。
 * 若实例被 Vue 深度代理，调用 downloadAndInstall 时会抛出与线上一致的 TypeError。
 */
class FakeUpdate {
  #rid = 17;

  available = true;
  currentVersion = "0.5.3";
  version = "0.5.4";
  date = null;
  body = "修复更新安装";
  rawJson = {};
  close = vi.fn(async () => undefined);

  async downloadAndInstall(): Promise<number> {
    return this.#rid;
  }
}

describe("useUpdateState 更新句柄", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    clearPendingUpdate();
  });

  it("保留带私有字段的原始 Update 实例，安装方法可正常调用", async () => {
    const update = new FakeUpdate();
    mocks.check.mockResolvedValue(update);

    const result = await runUpdateCheck();
    const pending = getPendingUpdate();

    expect(result.hasUpdate).toBe(true);
    expect(pending).toBe(update);
    await expect(
      (pending as unknown as FakeUpdate).downloadAndInstall(),
    ).resolves.toBe(17);
  });
});

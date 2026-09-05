import { renderHook } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ConfirmCtx } from "./confirmContext";
import { useOps } from "./useOps";

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <ConfirmCtx.Provider value={async () => true}>{children}</ConfirmCtx.Provider>
);

describe("파일 작업 결과", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("백엔드 호출이 실패하면 성공으로 돌려주지 않는다", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "trash_restore") throw new Error("디스크 오류");
      return null;
    });
    const { result } = renderHook(
      () => useOps({ reload: vi.fn(), refreshMeta: vi.fn() }),
      { wrapper },
    );
    await expect(result.current.restoreFiles([1])).resolves.toBe(false);
  });

  it("부분 실패도 선택을 풀 수 있는 성공으로 돌려주지 않는다", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "trash_restore")
        return {
          batch_id: 1,
          moved: 1,
          failed: 1,
          bytes: 0,
          first_error: "한 장 실패",
        };
      if (cmd === "libraries_list" || cmd === "ops_recent") return [];
      return null;
    });
    const { result } = renderHook(
      () => useOps({ reload: vi.fn(), refreshMeta: vi.fn() }),
      { wrapper },
    );
    await expect(result.current.restoreFiles([1, 2])).resolves.toBe(false);
  });

  it("파일 작업 뒤 화면 갱신만 실패해도 작업 결과는 성공이다", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "trash_restore")
        return {
          batch_id: 1,
          moved: 1,
          failed: 0,
          bytes: 10,
          first_error: null,
        };
      if (cmd === "libraries_list") throw new Error("목록 갱신 실패");
      return null;
    });
    const { result } = renderHook(
      () => useOps({ reload: vi.fn(), refreshMeta: vi.fn() }),
      { wrapper },
    );
    await expect(result.current.restoreFiles([1])).resolves.toBe(true);
  });
});

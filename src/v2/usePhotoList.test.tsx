import { renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EMPTY } from "./picks";
import { DEFAULT_SORT } from "./sortItems";
import { usePhotoList } from "./usePhotoList";
import type { Filter } from "./viewStore";

const FILTER: Filter = {
  ...EMPTY,
  sort: DEFAULT_SORT,
  library_id: null,
  folder_path: null,
  trashed: false,
};

describe("사진 목록 오류", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("첫 페이지 조회 실패를 빈 목록으로 숨기지 않는다", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "files_page") throw new Error("DB를 읽을 수 없음");
      return null;
    });
    const { result } = renderHook(() =>
      usePhotoList(FILTER, "none", { enabled: true }),
    );
    await waitFor(() => expect(result.current.loaded).toBe(true));
    expect(result.current.rows).toEqual([]);
    expect(result.current.error).toContain("DB를 읽을 수 없음");
  });
});

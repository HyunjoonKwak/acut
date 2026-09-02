import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import EventDiscoveryDialog from "./EventDiscoveryDialog";

describe("이벤트 자동 발견", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("후보 전체를 검토하고 사진별 제외 결과만 정리 화면으로 넘긴다", async () => {
    vi.mocked(invoke).mockResolvedValue([
      {
        key: "2026-08-31:1",
        date: "2026-08-31",
        start_at: 1788105600,
        end_at: 1788109200,
        count: 2,
        items: [
          { id: 10, name: "a.jpg", taken_at: 1788105600 },
          { id: 11, name: "b.jpg", taken_at: 1788109200 },
        ],
        suggestions: [{ title: "가족여행", why: "기존 폴더명", score: 1 }],
      },
    ]);
    const choose = vi.fn();
    render(<EventDiscoveryDialog libraryId={1} libraryName="작업대" onChoose={choose} onClose={vi.fn()} />);

    await screen.findByText(/제안: 가족여행/);
    await userEvent.click(screen.getByText("사진별 검토·제외"));
    const checks = screen.getAllByRole("checkbox");
    await userEvent.click(checks[1]);
    await userEvent.click(screen.getByRole("button", { name: "선택 1장 정리…" }));
    expect(choose).toHaveBeenCalledWith([10]);
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("event_candidates", {
      libraryId: 1,
      gapMinutes: 240,
      minCount: 8,
    });
  });
});

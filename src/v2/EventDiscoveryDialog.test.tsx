import { render, screen, waitFor } from "@testing-library/react";
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
    render(
      <EventDiscoveryDialog
        libraryId={1}
        libraryName="작업대"
        onChoose={choose}
        onClose={vi.fn()}
      />,
    );

    await screen.findByText(/제안: 가족여행/);
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
    await userEvent.click(screen.getByText("사진별 검토·제외"));
    const checks = screen.getAllByRole("checkbox");
    await userEvent.click(checks[1]);
    await userEvent.click(
      screen.getByRole("button", { name: "선택 1장 정리…" }),
    );
    expect(choose).toHaveBeenCalledWith([10]);
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("event_candidates", {
      libraryId: 1,
      gapMinutes: 240,
      minCount: 8,
    });
  });

  it("같은 후보를 다시 찾으면 펼쳐 둔 목록이 열린 채 비지 않는다", async () => {
    const candidate = {
      key: "1:1788105600:1788109200",
      date: "2026-08-31",
      start_at: 1788105600,
      end_at: 1788109200,
      count: 2,
      items: [
        { id: 10, name: "a.jpg", taken_at: 1788105600 },
        { id: 11, name: "b.jpg", taken_at: 1788109200 },
      ],
      suggestions: [],
    };
    // 후보 키는 검색 조건이 같으면 그대로다 — <details> DOM 이 재사용된다
    vi.mocked(invoke).mockResolvedValue([candidate]);
    render(
      <EventDiscoveryDialog
        libraryId={1}
        libraryName="작업대"
        onChoose={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    await screen.findByText("사진별 검토·제외");
    await userEvent.click(screen.getByText("사진별 검토·제외"));
    expect(screen.getAllByRole("checkbox")).toHaveLength(2);

    await userEvent.click(screen.getByRole("button", { name: "다시 찾기" }));
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryAllByRole("checkbox")).toHaveLength(0),
    );
    const details = screen.getByText("사진별 검토·제외").closest("details");
    expect(details?.open).toBe(false);

    await userEvent.click(screen.getByText("사진별 검토·제외"));
    expect(screen.getAllByRole("checkbox")).toHaveLength(2);
  });
});

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CaptureDateDialog from "./CaptureDateDialog";

describe("촬영일 감사·교정", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("dry-run 근거와 실제 기록 범위를 보인 뒤 자동 후보만 교정한다", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "capture_date_audit") {
        return [
          {
            id: 7,
            name: "1502088228879113.jpg",
            path: "카카오/1502088228879113.jpg",
            current_at: 1_700_000_000,
            current_source: 2,
            proposed_at: 1_502_088_228,
            proposed_source: "filename",
            evidence: "파일명 1502088228879113.jpg",
            interpretation: "파일명의 날짜/시각을 지역 wall-clock으로 해석",
            write_scope: "JPEG EXIF 3필드 + mtime",
            auto_selected: true,
            existing_exif: false,
          },
        ];
      }
      if (cmd === "capture_date_apply") {
        return {
          batch_id: 9,
          corrected: 1,
          failed: 0,
          first_error: null,
          failed_ids: [],
        };
      }
      return null;
    });
    const onChanged = vi.fn();
    render(
      <CaptureDateDialog
        target={{ ids: [7] }}
        onChanged={onChanged}
        onClose={vi.fn()}
      />,
    );

    expect(
      await screen.findByText("JPEG EXIF 3필드 + mtime"),
    ).toBeInTheDocument();
    expect(screen.getByText(/지역 wall-clock/)).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "선택한 자동 후보 교정" }),
    );

    await waitFor(() => expect(onChanged).toHaveBeenCalledOnce());
    const call = vi
      .mocked(invoke)
      .mock.calls.find(([cmd]) => cmd === "capture_date_apply");
    expect(call?.[1]).toMatchObject({
      changes: [{ id: 7, takenAt: 1_502_088_228, manual: false }],
    });
  });
});

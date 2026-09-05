import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import FolderNameAuditDialog from "./FolderNameAuditDialog";

describe("폴더 이름 감사", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("dry-run 근거와 충돌을 보여 주고 선택한 폴더만 일괄 적용한다", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "folder_name_audit")
        return [
          {
            source_dir: "2026.08.31 여행",
            parent_dir: "",
            current_name: "2026.08.31 여행",
            proposed_name: "2026-08-31 여행",
            reason: "점 날짜를 YYYY-MM-DD로 통일",
            file_count: 12,
            conflict: false,
          },
          {
            source_dir: "20260901",
            parent_dir: "",
            current_name: "20260901",
            proposed_name: "2026-09-01",
            reason: "붙어 있는 날짜를 YYYY-MM-DD로 통일",
            file_count: 3,
            conflict: true,
          },
        ];
      if (command === "folder_name_apply")
        return {
          batch_id: 9,
          completed: 1,
          failed: 0,
          conflicts: 0,
          first_error: null,
        };
      return null;
    });
    const changed = vi.fn();
    render(
      <FolderNameAuditDialog
        libraryId={1}
        libraryName="작업대"
        onChanged={changed}
        onClose={vi.fn()}
      />,
    );

    expect(await screen.findByText(/점 날짜를 YYYY-MM-DD/)).toBeInTheDocument();
    expect(
      screen.getByText(/같은 이름이 있어 자동 적용하지 않습니다/),
    ).toBeInTheDocument();
    const checks = screen.getAllByRole("checkbox");
    expect(checks[0]).toBeChecked();
    expect(checks[1]).toBeDisabled();
    await userEvent.click(
      screen.getByRole("button", { name: "선택 1개 적용" }),
    );
    await waitFor(() => expect(changed).toHaveBeenCalledOnce());
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("folder_name_apply", {
      libraryId: 1,
      sourceDirs: ["2026.08.31 여행"],
    });
  });
});

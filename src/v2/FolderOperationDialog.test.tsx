import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import FolderOperationDialog from "./FolderOperationDialog";
import { useData } from "./dataStore";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

describe("FolderOperationDialog", () => {
  beforeEach(() => {
    invoke.mockReset();
    useData.setState({
      libs: [
        { id: 1, volume_uuid: "A", volume_name: "A", rel_path: "", name: "내사진", area: 1, online: true, dir: "/A", file_count: 2 },
        { id: 2, volume_uuid: "B", volume_name: "B", rel_path: "", name: "공용", area: 2, online: false, dir: null, file_count: 0 },
      ],
      folders: [],
    });
  });

  it("실행 전에 미리보기를 요구하고 Drive 및 충돌 경고를 보여 준다", async () => {
    invoke.mockResolvedValueOnce({
      source: "2026/여행", destination: "여행 (2)", planned_name: "여행 (2)",
      conflict: "name_exists", action: "rename", files: 20, directories: 3,
      bytes: 2048, cross_volume: false, drive_sync_warning: true,
    });
    render(
      <FolderOperationDialog
        target={{ action: "copy", sourceLibraryId: 1, sourceDir: "2026/여행", sourceName: "여행" }}
        onChanged={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "폴더 복사 실행" })).toBeDisabled();
    await userEvent.selectOptions(screen.getByText("같은 이름 충돌").parentElement!.querySelector("select")!, "rename");
    await userEvent.click(screen.getByRole("button", { name: "충돌 미리보기" }));
    expect(await screen.findByText(/충돌을 피해 새 이름/)).toBeInTheDocument();
    expect(screen.getByText(/Drive 동기화 폴더/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "폴더 복사 실행" })).toBeEnabled();
  });
});

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TransferDialog from "./TransferDialog";
import { useData } from "./dataStore";

describe("임의 이동·복사와 공용 발행", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    useData.setState({
      libs: [
        { id: 1, name: "내사진", area: 1, online: true, file_count: 2, volume_uuid: "v", volume_name: "Mac", rel_path: "mine", dir: "/mine" },
        { id: 2, name: "공용", area: 2, online: true, file_count: 0, volume_uuid: "v", volume_name: "Mac", rel_path: "shared", dir: "/shared" },
      ],
      folders: [],
    });
  });

  it("내사진→공용은 복사가 기본이고 미리보기 전에는 실행하지 않는다", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "transfer_preview") return {
        mode: "copy", publish: true, source_area: 1, destination_area: 2,
        drive_sync_warning: true,
        items: [{ id: 3, source: "mine/a.jpg", destination: "a.jpg", planned_name: "a.jpg", conflict: "already_published", action: "skip", source_sha256: "abc" }],
      };
      return null;
    });
    render(<TransferDialog ids={[3]} sourceLibraryId={1} onChanged={vi.fn()} onClose={vi.fn()} />);

    expect(screen.getByText(/개인 원본을 유지/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "복사 실행" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "충돌 미리보기" }));
    expect(await screen.findByText("이미 발행 · 건너뜀")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "복사 실행" })).toBeEnabled();
    const previewCall = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "transfer_preview");
    expect(previewCall?.[1]).toMatchObject({ request: { mode: "copy", publish: true, destinationLibraryId: 2 } });
    await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "transfer_execute")).toHaveLength(0));
  });
});

import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Organize from "./Organize";
import { useData } from "./dataStore";

describe("기존 정리 흐름의 공용 발행", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "organize_date") return "2026-08-31";
      if (command === "organize_suggest") return [];
      if (command === "organize_preview") return "2026/2026-08-31";
      return null;
    });
    useData.setState({
      libs: [
        { id: 1, name: "내사진", area: 1, online: true, file_count: 2, volume_uuid: "a", volume_name: "A", rel_path: "mine", dir: "/mine" },
        { id: 2, name: "공용", area: 2, online: true, file_count: 0, volume_uuid: "b", volume_name: "B", rel_path: "shared", dir: "/shared" },
      ],
    });
  });

  it("내사진→공용을 이동이 아니라 원본 유지 복사로 안내한다", async () => {
    render(<Organize ids={[10]} libraryId={1} onDone={vi.fn()} onClose={vi.fn()} />);
    expect(await screen.findByText(/내사진 원본은 그대로/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "공용에 복사" })).toBeInTheDocument();
    expect(screen.getByText(/1장을 이벤트 폴더로 복사합니다/)).toBeInTheDocument();
  });
});

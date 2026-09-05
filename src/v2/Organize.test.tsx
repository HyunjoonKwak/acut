import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
        {
          id: 1,
          name: "내사진",
          area: 1,
          online: true,
          file_count: 2,
          volume_uuid: "a",
          volume_name: "A",
          rel_path: "mine",
          dir: "/mine",
        },
        {
          id: 2,
          name: "공용",
          area: 2,
          online: true,
          file_count: 0,
          volume_uuid: "b",
          volume_name: "B",
          rel_path: "shared",
          dir: "/shared",
        },
      ],
    });
  });

  it("내사진→공용을 이동이 아니라 원본 유지 복사로 안내한다", async () => {
    render(
      <Organize ids={[10]} libraryId={1} onDone={vi.fn()} onClose={vi.fn()} />,
    );
    expect(await screen.findByText(/내사진 원본은 그대로/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "공용에 복사" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/1장을 이벤트 폴더로 복사합니다/),
    ).toBeInTheDocument();
  });
});

describe("정리 부분 실패", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    useData.setState({
      libs: [
        {
          id: 1,
          name: "작업대",
          area: 0,
          online: true,
          file_count: 3,
          volume_uuid: "boot",
          volume_name: "Macintosh HD",
          rel_path: "Pictures",
          dir: "/Pictures",
        },
      ],
    });
  });

  it("실패한 사진만 창 안에 남겨 다시 시도한다", async () => {
    let moves = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "organize_date") return "2024-01-02";
      if (command === "organize_suggest") return [];
      if (command === "organize_preview") return "2024/2024-01-02";
      if (command === "organize_move") {
        moves += 1;
        return moves === 1
          ? {
              batch_id: 1,
              moved: 2,
              copied: 0,
              failed: 1,
              already_published: 0,
              bytes: 20,
              first_error: "한 장 실패",
              failed_ids: [3],
              mode: "move",
            }
          : {
              batch_id: 2,
              moved: 1,
              copied: 0,
              failed: 0,
              already_published: 0,
              bytes: 10,
              first_error: null,
              failed_ids: [],
              mode: "move",
            };
      }
      return null;
    });
    const onDone = vi.fn();
    const onClose = vi.fn();
    render(
      <Organize
        ids={[1, 2, 3]}
        libraryId={1}
        onDone={onDone}
        onClose={onClose}
      />,
    );

    const button = screen.getByRole("button", { name: "옮기기" });
    await waitFor(() => expect(button).toBeEnabled());
    const date = screen.getByLabelText("날짜");
    await userEvent.clear(date);
    await userEvent.type(date, "2025-12-31");
    await userEvent.click(button);
    await waitFor(() =>
      expect(
        screen.getByText(/1장을 이벤트 폴더로 옮깁니다/),
      ).toBeInTheDocument(),
    );
    expect(date).toHaveValue("2025-12-31");
    await userEvent.click(button);

    const calls = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "organize_move");
    expect(calls[0]?.[1]).toMatchObject({ ids: [1, 2, 3] });
    expect(calls[1]?.[1]).toMatchObject({ ids: [3] });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === "organize_date"),
    ).toHaveLength(1);
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });
});

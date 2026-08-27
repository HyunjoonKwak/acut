import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import SmartEdit from "./SmartEdit";
import { useData } from "./dataStore";
import { EMPTY } from "./picks";

const inv = vi.mocked(invoke);

describe("스마트 앨범 편집", () => {
  beforeEach(() => {
    inv.mockReset();
    inv.mockImplementation(async (cmd: string) => {
      if (cmd === "files_facets") return [];
      if (cmd === "smart_save") return 1;
      if (cmd === "smart_delete") return undefined;
      throw new Error(`mock 없음: ${cmd}`);
    });
    useData.setState({
      libs: [
        {
          id: 1,
          name: "PHOTO 1",
          online: true,
          volume_uuid: "",
          volume_name: "",
          rel_path: "",
          area: 1,
          dir: null,
          file_count: 0,
        },
      ],
    });
  });

  it("이름이 없으면 저장할 수 없다", () => {
    render(<SmartEdit initial={null} onClose={() => {}} onSaved={() => {}} />);
    expect(screen.getByRole("button", { name: "만들기" })).toBeDisabled();
  });

  it("이름·조건·라이브러리·묶기를 한 덩어리로 저장한다", async () => {
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(<SmartEdit initial={null} onClose={onClose} onSaved={onSaved} />);
    await userEvent.type(screen.getByLabelText("이름"), "별 넷 영상");
    await userEvent.click(screen.getByRole("button", { name: "영상" }));
    await userEvent.click(screen.getByRole("button", { name: "PHOTO 1" }));
    await userEvent.click(screen.getByRole("button", { name: "만들기" }));

    const call = inv.mock.calls.find((c) => c[0] === "smart_save");
    expect(call).toBeTruthy();
    const args = call![1] as { name: string; filter: Record<string, unknown> };
    expect(args.name).toBe("별 넷 영상");
    expect(args.filter.kind).toBe(1);
    expect(args.filter.library_id).toBe(1);
    expect(args.filter.group).toBe("none");
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("이름을 바꾸면 옛 줄을 지우고 새로 넣는다", async () => {
    render(
      <SmartEdit
        initial={{
          id: 7,
          name: "옛 이름",
          filter: { ...EMPTY, kind: 2 },
          sort: null,
        }}
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    const input = screen.getByLabelText("이름");
    await userEvent.clear(input);
    await userEvent.type(input, "새 이름");
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(inv).toHaveBeenCalledWith("smart_delete", { id: 7 });
    const save = inv.mock.calls.find((c) => c[0] === "smart_save")![1] as {
      name: string;
      filter: { kind: number };
    };
    expect(save.name).toBe("새 이름");
    expect(save.filter.kind).toBe(2); // 조건은 그대로 들고 간다
  });

  it("이름이 그대로면 덮어쓴다 — 지우지 않는다", async () => {
    render(
      <SmartEdit
        initial={{ id: 7, name: "그대로", filter: EMPTY, sort: null }}
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(inv).not.toHaveBeenCalledWith("smart_delete", expect.anything());
  });
});

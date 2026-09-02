import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import SelectMenu from "./SelectMenu";
import { useSelection } from "./selectionStore";
import type { FileRow } from "./types";

const row = (p: Partial<FileRow> & { id: number }): FileRow =>
  ({
    name: `f${p.id}.jpg`,
    kind: 0,
    culling_flag: 0,
    rating: 0,
    favorite: false,
    ...p,
  }) as FileRow;

const ROWS = [
  row({ id: 1, culling_flag: 1 }),
  row({ id: 2, culling_flag: 2 }),
  row({ id: 3, favorite: true }),
  row({ id: 4, kind: 1 }),
];

const base = {
  rows: ROWS,
  matched: ROWS.length,
  compareIds: [],
  markPicked: () => {},
  onTrash: async () => true,
};

describe("선택 메뉴", () => {
  beforeEach(() => useSelection.getState().clearPicked());

  it("조건으로 고른다 — 불러온 목록 안에서", async () => {
    render(<SelectMenu {...base} />);
    await userEvent.click(screen.getByRole("button", { name: /선택/ }));
    await userEvent.click(screen.getByText("남김만 고르기"));
    expect([...useSelection.getState().picked]).toEqual([1]);

    await userEvent.click(screen.getByRole("button", { name: /1장 선택/ }));
    await userEvent.click(screen.getByText("반대로 고르기"));
    expect([...useSelection.getState().picked].sort()).toEqual([2, 3, 4]);
  });

  it("고른 것이 있어야 처리 항목이 보이고, 남김은 고른 것 전부에 찍힌다", async () => {
    const markPicked = vi.fn();
    render(<SelectMenu {...base} markPicked={markPicked} />);
    await userEvent.click(screen.getByRole("button", { name: /선택/ }));
    expect(screen.queryByText("고른 것 남김")).not.toBeInTheDocument();
    await userEvent.click(screen.getByText("모두 고르기"));
    expect(useSelection.getState().picked.size).toBe(4);

    await userEvent.click(screen.getByRole("button", { name: /4장 선택/ }));
    await userEvent.click(screen.getByText("고른 것 남김"));
    expect(markPicked).toHaveBeenCalledWith({ cullingFlag: 1 });
  });

  it("전체 결과가 아직 안 내려왔으면 선택 범위를 숨기지 않는다", async () => {
    render(<SelectMenu {...base} matched={4452} />);
    await userEvent.click(screen.getByRole("button", { name: /선택/ }));
    expect(
      screen.getByRole("menuitem", { name: /불러온 4장 모두 고르기/ }),
    ).toBeInTheDocument();
  });

  it("메뉴를 열면 첫 항목에 초점이 가고 화살표로 이동한다", async () => {
    render(<SelectMenu {...base} />);
    const trigger = screen.getByRole("button", { name: /선택/ });
    await userEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    const items = screen.getAllByRole("menuitem");
    await waitFor(() => expect(items[0]).toHaveFocus());
    await userEvent.keyboard("{ArrowDown}");
    expect(items[1]).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
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
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import SelectionPanel from "./SelectionPanel";
import { useSelection } from "./selectionStore";
import { usePrefs, DEFAULT_PREFS } from "./prefs";
import { useUi } from "./uiStore";
import type { FileRow } from "./types";

const row = (id: number, size: number): FileRow => ({
  id,
  name: `IMG_${id}.jpg`,
  taken_at: 0,
  taken_at_source: 0,
  kind: 0,
  size,
  width: null,
  height: null,
  rating: 0,
  culling_flag: 0,
  favorite: false,
  duration_ms: null,
  group: null,
  library_id: 1,
  thumb: null,
  iso: null,
  aperture: null,
  shutter: null,
  focal_mm: null,
  cam_model: null,
});
const rows = [row(1, 1000), row(2, 2000), row(3, 4000)];

describe("선택 패널", () => {
  beforeEach(() => {
    useSelection.setState({ selected: null, picked: new Set() });
    usePrefs.setState({ ...DEFAULT_PREFS });
    useUi.setState({
      comparing: null,
      organizing: false,
      organizeSelection: null,
    });
  });

  it("고른 것이 없으면 안 뜬다", () => {
    const { container } = render(
      <SelectionPanel
        rows={rows}
        compareIds={[]}
        markPicked={() => {}}
        onTrash={async () => true}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("몇 장·몇 바이트를 고른 것인지 보여 준다", () => {
    useSelection.setState({ picked: new Set([1, 3]) });
    render(
      <SelectionPanel
        rows={rows}
        compareIds={[1, 3]}
        markPicked={() => {}}
        onTrash={async () => true}
      />,
    );
    expect(screen.getByText("2장 선택")).toBeInTheDocument();
    expect(screen.getByText("4.9 KB")).toBeInTheDocument();
  });

  it("남김·제외·별점이 고른 것 전부에 간다", async () => {
    useSelection.setState({ picked: new Set([1, 2]) });
    const markPicked = vi.fn();
    render(
      <SelectionPanel
        rows={rows}
        compareIds={[1, 2]}
        markPicked={markPicked}
        onTrash={async () => true}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /남김/ }));
    expect(markPicked).toHaveBeenLastCalledWith({ cullingFlag: 1 });
    await userEvent.click(screen.getByTitle("별 4개"));
    expect(markPicked).toHaveBeenLastCalledWith({ rating: 4 });
  });

  it("나란히 보기는 두 장 이상일 때만, 누르면 비교 창이 열린다", async () => {
    useSelection.setState({ picked: new Set([2]) });
    const { rerender } = render(
      <SelectionPanel
        rows={rows}
        compareIds={[2]}
        markPicked={() => {}}
        onTrash={async () => true}
      />,
    );
    expect(screen.queryByText("나란히 보기")).not.toBeInTheDocument();

    useSelection.setState({ picked: new Set([2, 3]) });
    rerender(
      <SelectionPanel
        rows={rows}
        compareIds={[2, 3]}
        markPicked={() => {}}
        onTrash={async () => true}
      />,
    );
    await userEvent.click(screen.getByText("나란히 보기"));
    expect(useUi.getState().comparing).toEqual([2, 3]);
  });

  it("옮겨 넣을 라이브러리가 없으면 정리를 못 한다", () => {
    useSelection.setState({ picked: new Set([1]) });
    render(
      <SelectionPanel
        rows={rows}
        compareIds={[1]}
        markPicked={() => {}}
        onTrash={async () => true}
      />,
    );
    expect(screen.getByRole("button", { name: "정리" })).toBeDisabled();
  });

  it("휴지통으로 보내고 나면 선택이 풀린다 — 취소하면 그대로", async () => {
    useSelection.setState({ picked: new Set([1, 2]) });
    const onTrash = vi.fn(async () => false);
    const { rerender } = render(
      <SelectionPanel
        rows={rows}
        compareIds={[1, 2]}
        markPicked={() => {}}
        onTrash={onTrash}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "휴지통으로" }));
    expect(onTrash).toHaveBeenCalledWith([1, 2]);
    expect(useSelection.getState().picked.size).toBe(2);

    onTrash.mockResolvedValue(true);
    rerender(
      <SelectionPanel
        rows={rows}
        compareIds={[1, 2]}
        markPicked={() => {}}
        onTrash={onTrash}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "휴지통으로" }));
    expect(useSelection.getState().picked.size).toBe(0);
  });
});

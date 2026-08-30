import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import StatusActions from "./StatusActions";
import { useData } from "./dataStore";
import { useJob } from "./jobStore";
import { useView } from "./viewStore";

const noop = {
  stopJob: vi.fn(),
  restoreAll: vi.fn(),
  emptyTrash: vi.fn(),
  cleanExcluded: vi.fn(),
  unmarkExcluded: vi.fn(),
  undoLast: vi.fn(),
};

describe("상태바 오른쪽", () => {
  beforeEach(() => {
    useData.setState({ busy: "", toClean: null, batches: [], stats: null });
    useJob.setState({ job: null });
    useView.setState({ viewTrash: false });
  });

  it("도는 일이 있으면 진행과 멈추기만 보인다", async () => {
    useJob.setState({ job: { label: "썸네일", done: 1234, total: 78857 } });
    useData.setState({ toClean: { files: 5, bytes: 1 } });
    render(<StatusActions {...noop} />);
    expect(screen.getByText(/썸네일 1,234/)).toBeInTheDocument();
    expect(screen.queryByText(/치우기/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "멈추기" }));
    expect(noop.stopJob).toHaveBeenCalled();
  });

  it("제외한 것이 있으면 «확정 (N)» 과 «취소» 가 짝으로 뜬다 — 확정은 휴지통으로, 취소는 표시만 지움", async () => {
    useData.setState({ toClean: { files: 12, bytes: 3_000_000 } });
    render(<StatusActions {...noop} />);
    const ok = screen.getByRole("button", { name: "확정 (12)" });
    expect(ok.title).toMatch(/휴지통으로/);
    await userEvent.click(ok);
    expect(noop.cleanExcluded).toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(noop.unmarkExcluded).toHaveBeenCalled();
  });

  it("«썸네일 없음»을 누르면 그것만 보고, 다시 누르면 풀린다", async () => {
    useData.setState({
      stats: { files: 100, bytes: 1, thumbs_done: 86, thumbs_pending: 14 },
    });
    useView.setState({
      picks: { ...useView.getState().picks, no_thumb: false },
    });
    const { rerender } = render(<StatusActions {...noop} />);
    await userEvent.click(
      screen.getByRole("button", { name: "썸네일 없음 14장" }),
    );
    expect(useView.getState().picks.no_thumb).toBe(true);
    rerender(<StatusActions {...noop} />);
    await userEvent.click(screen.getByRole("button", { name: /만 보는 중/ }));
    expect(useView.getState().picks.no_thumb).toBe(false);
  });

  it("휴지통을 보고 있으면 되돌리기·비우기 — 휴지통에 무언가 있을 때만", () => {
    useView.setState({ viewTrash: true });
    useData.setState({ trash: { files: 0, bytes: 0 } });
    const { unmount } = render(<StatusActions {...noop} />);
    expect(screen.queryByRole("button", { name: "전부 되돌리기" })).not.toBeInTheDocument();
    unmount();
    useData.setState({ trash: { files: 4, bytes: 10 } });
    render(<StatusActions {...noop} />);
    expect(
      screen.getByRole("button", { name: "전부 되돌리기" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "영구히 비우기" }),
    ).toBeInTheDocument();
  });

  it("아직 안 물린 작업이 있으면 되돌리기가 뜬다", async () => {
    useData.setState({
      batches: [
        {
          id: 5,
          kind: "move",
          label: "정리 → 2024/여행",
          item_count: 7,
          created_at: 0,
          undone_at: null,
        },
        {
          id: 3,
          kind: "delete",
          label: "휴지통 비우기",
          item_count: 1263,
          created_at: 0,
          undone_at: null,
        },
      ],
    });
    const { unmount } = render(<StatusActions {...noop} />);
    // 가장 최근 작업(정리)만 — 단추 이름이 «무엇을 몇 장» 물리는지 말한다
    const b = screen.getByRole("button", { name: /정리 되돌리기 \(7장\)/ });
    await userEvent.click(b);
    expect(noop.undoLast).toHaveBeenCalled();
    unmount();
    // 가장 최근 작업이 휴지통 비우기면 그 전의 정리를 되돌리라고 권하지 않는다
    useData.setState({
      batches: [
        { id: 6, kind: "delete", label: "휴지통 비우기", item_count: 9, created_at: 0, undone_at: null },
        { id: 5, kind: "move", label: "정리 → 2024/여행", item_count: 7, created_at: 0, undone_at: null },
      ],
    });
    render(<StatusActions {...noop} />);
    expect(screen.queryByRole("button", { name: /되돌리기/ })).not.toBeInTheDocument();
  });
});

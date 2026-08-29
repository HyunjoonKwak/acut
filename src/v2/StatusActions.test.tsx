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

  it("제외한 것이 있으면 치우기 버튼이 뜬다 — 범위(전체/이 라이브러리)를 이름에 단다", async () => {
    useData.setState({ toClean: { files: 12, bytes: 3_000_000 } });
    render(<StatusActions {...noop} />);
    await userEvent.click(
      screen.getByRole("button", { name: /제외한 12장 휴지통으로$/ }),
    );
    expect(noop.cleanExcluded).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /^전체에서 제외한 12장 휴지통으로$/ })).toBeInTheDocument();
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
          id: 3,
          kind: "delete",
          label: "휴지통 비우기",
          item_count: 1263,
          created_at: 0,
          undone_at: null,
        },
        {
          id: 1,
          kind: "move",
          label: "정리",
          item_count: 3,
          created_at: 0,
          undone_at: 99,
        },
        {
          id: 2,
          kind: "trash",
          label: "휴지통",
          item_count: 1,
          created_at: 0,
          undone_at: null,
        },
      ],
    });
    render(<StatusActions {...noop} />);
    // 단추 이름이 «무엇을 몇 장» 물리는지 말한다 — 물린 것(1번)이 아니라 아직 안 물린 것(2번).
    // 영구히 비운 것(delete)은 되돌릴 수 없으니 맨 앞에 있어도 건너뛴다
    const b = screen.getByRole("button", { name: /되돌리기: 휴지통 \(1장\)/ });
    await userEvent.click(b);
    expect(noop.undoLast).toHaveBeenCalled();
  });
});

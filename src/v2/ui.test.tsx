import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Btn, Menu, MenuItem } from "./ui";
import { useModalFocus } from "./focus";

function MenuOpeningDialog() {
  const [dialog, setDialog] = useState(false);
  return (
    <>
      <Menu
        trigger={(_, props) => <Btn {...props}>작업</Btn>}
      >
        {(close) => (
          <MenuItem onClick={() => { setDialog(true); close(); }}>
            대화상자 열기
          </MenuItem>
        )}
      </Menu>
      {dialog && (
        <div role="dialog" aria-modal="true" aria-label="새 작업">
          <input aria-label="새 이름" autoFocus />
        </div>
      )}
    </>
  );
}

function TestModal({
  onClose,
  locked,
  autoFocusLast,
}: {
  onClose: () => void;
  locked?: boolean;
  autoFocusLast?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useModalFocus(ref, onClose, { locked });
  return (
    <div ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label="시험 모달">
      <input aria-label="첫 입력" />
      <input aria-label="가운데 입력" autoFocus={autoFocusLast} />
      <button onClick={onClose}>닫기</button>
    </div>
  );
}

function ModalHarness({
  locked,
  autoFocusLast,
}: {
  locked?: boolean;
  autoFocusLast?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button onClick={() => setOpen(true)}>열기</button>
      {open && (
        <TestModal
          onClose={() => setOpen(false)}
          locked={locked}
          autoFocusLast={autoFocusLast}
        />
      )}
    </>
  );
}

/** 메뉴 항목이 대화상자를 연다 — 항목은 대화상자가 뜨는 커밋에서 사라진다 */
function MenuOpeningModal() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Menu trigger={(_, props) => <Btn {...props}>작업</Btn>}>
        {(close) => (
          <MenuItem
            onClick={() => {
              close();
              setOpen(true);
            }}
          >
            시험 모달 열기
          </MenuItem>
        )}
      </Menu>
      {open && <TestModal onClose={() => setOpen(false)} />}
    </>
  );
}

/** 대화상자 위에 다른 모달(가져오기 상자처럼 훅을 쓰지 않는 것)이 겹친다 */
function StackedHarness({ onTopEscape }: { onTopEscape: () => void }) {
  const [open, setOpen] = useState(true);
  return (
    <>
      {open && <TestModal onClose={() => setOpen(false)} />}
      <div
        role="dialog"
        aria-modal="true"
        aria-label="위 모달"
        tabIndex={-1}
        onKeyDown={(e) => e.key === "Escape" && onTopEscape()}
      >
        <button>위 단추</button>
        <button>위 단추 둘</button>
      </div>
    </>
  );
}

describe("메뉴 초점 복원", () => {
  it("메뉴 항목이 연 대화상자의 초점을 트리거가 빼앗지 않는다", async () => {
    render(<MenuOpeningDialog />);
    await userEvent.click(screen.getByRole("button", { name: "작업" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "대화상자 열기" }));
    const input = await screen.findByRole("textbox", { name: "새 이름" });
    await waitFor(() => expect(input).toHaveFocus());
  });

  it("모달은 첫 초점을 잡고 Tab을 가두며 Esc 뒤 원래 단추로 돌아간다", async () => {
    render(<ModalHarness />);
    const trigger = screen.getByRole("button", { name: "열기" });
    await userEvent.click(trigger);
    const first = screen.getByRole("textbox", { name: "첫 입력" });
    const middle = screen.getByRole("textbox", { name: "가운데 입력" });
    const last = screen.getByRole("button", { name: "닫기" });
    await waitFor(() => expect(first).toHaveFocus());
    // 실제 Tab 처럼 초점을 옮긴다 — 안에서는 다음으로, 끝에서는 반대쪽 끝으로
    await userEvent.tab({ shift: true });
    expect(last).toHaveFocus();
    await userEvent.tab();
    expect(first).toHaveFocus();
    await userEvent.tab();
    expect(middle).toHaveFocus();
    await userEvent.tab();
    expect(last).toHaveFocus();
    await userEvent.tab();
    expect(first).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("안에 이미 초점이 있으면(autoFocus) 첫 컨트롤로 옮기지 않는다", async () => {
    render(<ModalHarness autoFocusLast />);
    await userEvent.click(screen.getByRole("button", { name: "열기" }));
    const middle = screen.getByRole("textbox", { name: "가운데 입력" });
    expect(middle).toHaveFocus();
    // rAF 가 돈 뒤에도 그대로여야 한다
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
    expect(middle).toHaveFocus();
  });

  it("실행 중(locked)에는 Esc 로 닫히지 않는다", async () => {
    render(<ModalHarness locked />);
    await userEvent.click(screen.getByRole("button", { name: "열기" }));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "첫 입력" })).toHaveFocus(),
    );
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("위에 다른 모달이 떠 있으면 Esc 와 Tab 은 그쪽 몫이다", async () => {
    const topEscape = vi.fn();
    render(<StackedHarness onTopEscape={topEscape} />);
    // 아래 상자가 첫 초점을 잡은 뒤에 위 모달로 옮겨야 rAF 와 겨루지 않는다
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "첫 입력" })).toHaveFocus(),
    );
    const topButton = screen.getByRole("button", { name: "위 단추" });
    topButton.focus();
    await userEvent.keyboard("{Escape}");
    expect(topEscape).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("dialog", { name: "시험 모달" })).toBeInTheDocument();
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "위 단추 둘" })).toHaveFocus();
  });

  it("메뉴 항목이 연 대화상자를 닫으면 메뉴 트리거로 돌아간다", async () => {
    render(<MenuOpeningModal />);
    const trigger = screen.getByRole("button", { name: "작업" });
    await userEvent.click(trigger);
    await userEvent.click(screen.getByRole("menuitem", { name: "시험 모달 열기" }));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "첫 입력" })).toHaveFocus(),
    );
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("메뉴 밖 입력을 클릭해 닫았을 때 그 입력의 초점을 유지한다", async () => {
    render(
      <>
        <Menu trigger={(_, props) => <Btn {...props}>작업</Btn>}>
          {() => <MenuItem onClick={() => {}}>항목</MenuItem>}
        </Menu>
        <input aria-label="검색" />
      </>,
    );
    await userEvent.click(screen.getByRole("button", { name: "작업" }));
    const search = screen.getByRole("textbox", { name: "검색" });
    await userEvent.click(search);
    await waitFor(() => expect(search).toHaveFocus());
  });

  it("메뉴를 여는 ArrowDown이 뒤쪽 격자 키 핸들러로 새지 않는다", () => {
    const behind = vi.fn();
    render(
      <div onKeyDown={behind}>
        <Menu trigger={(_, props) => <Btn {...props}>작업</Btn>}>
          {() => <MenuItem onClick={() => {}}>항목</MenuItem>}
        </Menu>
      </div>,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "작업" }), {
      key: "ArrowDown",
    });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(behind).not.toHaveBeenCalled();
  });
});

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

function TestModal({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useModalFocus(ref, onClose);
  return (
    <div ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label="시험 모달">
      <input aria-label="첫 입력" />
      <button onClick={onClose}>닫기</button>
    </div>
  );
}

function ModalHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button onClick={() => setOpen(true)}>열기</button>
      {open && <TestModal onClose={() => setOpen(false)} />}
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
    const last = screen.getByRole("button", { name: "닫기" });
    await waitFor(() => expect(first).toHaveFocus());
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(first).toHaveFocus();
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

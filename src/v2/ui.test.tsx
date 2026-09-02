import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Btn, Menu, MenuItem } from "./ui";

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

describe("메뉴 초점 복원", () => {
  it("메뉴 항목이 연 대화상자의 초점을 트리거가 빼앗지 않는다", async () => {
    render(<MenuOpeningDialog />);
    await userEvent.click(screen.getByRole("button", { name: "작업" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "대화상자 열기" }));
    const input = await screen.findByRole("textbox", { name: "새 이름" });
    await waitFor(() => expect(input).toHaveFocus());
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
});

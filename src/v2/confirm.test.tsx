import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConfirmProvider } from "./confirm";
import { useConfirm } from "./confirmContext";

/** 물어보고 답을 밖으로 보내는 버튼 */
function Asker({
  onResult,
  danger,
}: {
  onResult: (ok: boolean) => void;
  danger?: boolean;
}) {
  const ask = useConfirm();
  return (
    <button
      onClick={async () =>
        onResult(
          await ask({
            title: "「PHOTO 1」 등록을 지웁니다",
            lines: [
              "· 사진 78,857장의 기록이 사라집니다",
              "· 원본은 그대로입니다",
            ],
            confirmLabel: "등록 지우기",
            danger,
          }),
        )
      }
    >
      열기
    </button>
  );
}

const setup = (danger = true) => {
  const onResult = vi.fn();
  render(
    <ConfirmProvider>
      <Asker onResult={onResult} danger={danger} />
    </ConfirmProvider>,
  );
  return { onResult, user: userEvent.setup() };
};

describe("물음 상자", () => {
  it("무엇이 사라지는지 줄 단위로 보여 준다", async () => {
    const { user } = setup();
    await user.click(screen.getByText("열기"));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("「PHOTO 1」 등록을 지웁니다")).toBeInTheDocument();
    expect(
      screen.getByText("· 사진 78,857장의 기록이 사라집니다"),
    ).toBeInTheDocument();
    expect(screen.getByText("· 원본은 그대로입니다")).toBeInTheDocument();
  });

  it("확인 버튼은 시킨 말로, 위험하면 붉게", async () => {
    const { user } = setup(true);
    await user.click(screen.getByText("열기"));
    const btn = screen.getByRole("button", { name: "등록 지우기" });
    expect(btn.className).toContain("bg-drop");
  });

  it("취소하면 false, 확인하면 true — 그 뒤 상자는 닫힌다", async () => {
    const { onResult, user } = setup();
    await user.click(screen.getByText("열기"));
    await user.click(screen.getByRole("button", { name: "취소" }));
    expect(onResult).toHaveBeenLastCalledWith(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(screen.getByText("열기"));
    await user.click(screen.getByRole("button", { name: "등록 지우기" }));
    expect(onResult).toHaveBeenLastCalledWith(true);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("Esc는 취소다", async () => {
    const { onResult, user } = setup();
    await user.click(screen.getByText("열기"));
    await user.keyboard("{Escape}");
    expect(onResult).toHaveBeenLastCalledWith(false);
  });

  /** 되돌릴 수 없는 일에 Enter 한 번이 «예»가 되면 안 된다 — 확인 버튼에
   *  초점이 가 있어 Enter가 그걸 누르는 것과는 다른 얘기다. 여기서는 상자
   *  자체의 Enter 처리가 있는지만 본다. */
  it("확인 버튼이 초점을 갖고 뜬다", async () => {
    const { user } = setup();
    await user.click(screen.getByText("열기"));
    expect(screen.getByRole("button", { name: "등록 지우기" })).toHaveFocus();
  });
});

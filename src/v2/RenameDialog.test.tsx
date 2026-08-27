import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import RenameDialog from "./RenameDialog";
import { badName } from "./fileName";

describe("이름 바꾸기", () => {
  it("못 쓰는 이름을 가려낸다", () => {
    expect(badName("")).toBeTruthy();
    expect(badName("  ")).toBeTruthy();
    expect(badName("a/b.jpg")).toBeTruthy();
    expect(badName("..")).toBeTruthy();
    expect(badName("주원 첫돌.jpg")).toBeNull();
  });

  it("확장자 앞부분만 고른 채로 뜬다", () => {
    render(
      <RenameDialog
        name="IMG_2481.JPG"
        onSubmit={async () => {}}
        onClose={() => {}}
      />,
    );
    const input = screen.getByLabelText("새 이름") as HTMLInputElement;
    expect(input).toHaveFocus();
    expect(input.value.slice(input.selectionStart!, input.selectionEnd!)).toBe(
      "IMG_2481",
    );
  });

  it("그대로거나 비었으면 바꿀 수 없다", async () => {
    render(
      <RenameDialog
        name="a.jpg"
        onSubmit={async () => {}}
        onClose={() => {}}
      />,
    );
    const btn = screen.getByRole("button", { name: "바꾸기" });
    expect(btn).toBeDisabled();
    await userEvent.clear(screen.getByLabelText("새 이름"));
    expect(btn).toBeDisabled();
  });

  it("다듬은 이름으로 넘기고 닫는다", async () => {
    const onSubmit = vi.fn(async () => {});
    const onClose = vi.fn();
    render(<RenameDialog name="a.jpg" onSubmit={onSubmit} onClose={onClose} />);
    const input = screen.getByLabelText("새 이름");
    await userEvent.clear(input);
    await userEvent.type(input, "  주원.jpg  {Enter}");
    expect(onSubmit).toHaveBeenCalledWith("주원.jpg");
    expect(onClose).toHaveBeenCalled();
  });

  it("백엔드가 거절하면 그 말을 보여 주고 열려 있는다", async () => {
    const onClose = vi.fn();
    render(
      <RenameDialog
        name="a.jpg"
        onSubmit={async () => {
          throw new Error("같은 이름의 파일이 이미 있습니다: b.jpg");
        }}
        onClose={onClose}
      />,
    );
    const input = screen.getByLabelText("새 이름");
    await userEvent.clear(input);
    await userEvent.type(input, "b.jpg{Enter}");
    expect(
      await screen.findByText(/같은 이름의 파일이 이미 있습니다/),
    ).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });
});

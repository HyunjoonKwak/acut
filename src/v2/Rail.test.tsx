import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Rail from "./Rail";

describe("레일", () => {
  it("이름은 커서가 왔을 때만 뜬다", async () => {
    render(<Rail value="all" open onPick={() => {}} trashCount={0} />);
    expect(screen.queryByText("앨범")).not.toBeInTheDocument();
    await userEvent.hover(screen.getByRole("button", { name: "앨범" }));
    expect(screen.getByText("앨범")).toBeInTheDocument();
    await userEvent.unhover(screen.getByRole("button", { name: "앨범" }));
    expect(screen.queryByText("앨범")).not.toBeInTheDocument();
  });

  it("고른 갈래는 눌린 상태로 — 패널이 접혀 있으면 아니다", () => {
    const { rerender } = render(
      <Rail value="tag" open onPick={() => {}} trashCount={0} />,
    );
    expect(screen.getByRole("button", { name: "태그" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    rerender(
      <Rail value="tag" open={false} onPick={() => {}} trashCount={0} />,
    );
    expect(screen.getByRole("button", { name: "태그" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("휴지통에 든 수가 배지로 붙고 99를 넘으면 99+", () => {
    const { rerender } = render(
      <Rail value="all" open onPick={() => {}} trashCount={7} />,
    );
    expect(screen.getByText("7")).toBeInTheDocument();
    rerender(<Rail value="all" open onPick={() => {}} trashCount={250} />);
    expect(screen.getByText("99+")).toBeInTheDocument();
  });

  it("누르면 그 갈래를 알린다", async () => {
    const onPick = vi.fn();
    render(<Rail value="all" open onPick={onPick} trashCount={0} />);
    await userEvent.click(screen.getByRole("button", { name: "달력" }));
    expect(onPick).toHaveBeenCalledWith("calendar");
  });
});

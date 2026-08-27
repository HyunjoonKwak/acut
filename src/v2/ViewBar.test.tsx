import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ViewBar from "./ViewBar";

const base = {
  scaling: "cover" as const,
  onScaling: () => {},
  caption: true,
  onCaption: () => {},
  filmstrip: false,
  onFilmstrip: () => {},
};

describe("보기 방식 버튼", () => {
  it("누를 때마다 카드 → 타일 → 양쪽 맞춤 → 카드로 돈다", async () => {
    const onStyle = vi.fn();
    const { rerender } = render(
      <ViewBar {...base} style="card" onStyle={onStyle} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "보기: 카드" }));
    expect(onStyle).toHaveBeenLastCalledWith("tile");
    rerender(<ViewBar {...base} style="tile" onStyle={onStyle} />);
    await userEvent.click(screen.getByRole("button", { name: "보기: 타일" }));
    expect(onStyle).toHaveBeenLastCalledWith("justified");
    rerender(<ViewBar {...base} style="justified" onStyle={onStyle} />);
    await userEvent.click(
      screen.getByRole("button", { name: "보기: 양쪽 맞춤" }),
    );
    expect(onStyle).toHaveBeenLastCalledWith("card");
  });

  it("양쪽 맞춤에서는 담기 버튼이 없다", () => {
    const { rerender } = render(
      <ViewBar {...base} style="card" onStyle={() => {}} />,
    );
    expect(screen.getByRole("button", { name: /담기:/ })).toBeInTheDocument();
    rerender(<ViewBar {...base} style="justified" onStyle={() => {}} />);
    expect(
      screen.queryByRole("button", { name: /담기:/ }),
    ).not.toBeInTheDocument();
  });

  it("이름표에 지금 것과 다음 것이 같이 적힌다", async () => {
    render(<ViewBar {...base} style="card" onStyle={() => {}} />);
    await userEvent.hover(screen.getByRole("button", { name: "보기: 카드" }));
    expect(screen.getByText("보기: 카드 → 누르면 타일")).toBeInTheDocument();
  });

  it("켜고 끄는 것은 눌린 상태를 보인다", async () => {
    const onCaption = vi.fn();
    render(
      <ViewBar
        {...base}
        style="card"
        onStyle={() => {}}
        onCaption={onCaption}
      />,
    );
    const b = screen.getByRole("button", { name: "이름·크기 표시" });
    expect(b).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(b);
    expect(onCaption).toHaveBeenCalledWith(false);
  });
});

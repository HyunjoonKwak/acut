import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import FilterChips from "./FilterChips";
import { EMPTY } from "./picks";

const tagName = (id: number) => (id === 3 ? "가족" : undefined);

describe("조건 칩", () => {
  it("아무 조건도 없으면 아무것도 안 그린다", () => {
    const { container } = render(
      <FilterChips value={EMPTY} onChange={() => {}} tagName={tagName} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("걸린 조건이 사람 말로 뜬다", () => {
    render(
      <FilterChips
        value={{ ...EMPTY, tag_id: 3, kind: 1, place: "37.5,126.9" }}
        onChange={() => {}}
        tagName={tagName}
      />,
    );
    expect(screen.getByText("가족")).toBeInTheDocument();
    expect(screen.getByText("영상")).toBeInTheDocument();
    expect(screen.getByText("북위 37.5° 동경 126.9°")).toBeInTheDocument();
  });

  it("✕를 누르면 그 조건만 떨어진다", async () => {
    const onChange = vi.fn();
    render(
      <FilterChips
        value={{ ...EMPTY, tag_id: 3, kind: 1 }}
        onChange={onChange}
        tagName={tagName}
      />,
    );
    await userEvent.click(screen.getByTitle("영상 조건 떼기"));
    expect(onChange).toHaveBeenCalledWith({ ...EMPTY, tag_id: 3 });
  });

  it("둘 이상일 때만 «모두 지우기»가 있고, 누르면 다 푼다", async () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <FilterChips
        value={{ ...EMPTY, kind: 1 }}
        onChange={onChange}
        tagName={tagName}
      />,
    );
    expect(screen.queryByText("모두 지우기")).not.toBeInTheDocument();

    rerender(
      <FilterChips
        value={{ ...EMPTY, kind: 1, favorite_only: true }}
        onChange={onChange}
        tagName={tagName}
      />,
    );
    await userEvent.click(screen.getByText("모두 지우기"));
    expect(onChange).toHaveBeenCalledWith(EMPTY);
  });
});

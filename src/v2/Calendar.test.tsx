import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import Calendar from "./Calendar";

const base = {
  year: null,
  month: null,
  day: null,
  facetFilter: {},
  onPick: vi.fn(),
};

describe("달력 갈래", () => {
  it("첫 집계가 끝나기 전에는 빈 달력이라고 오해시키지 않는다", () => {
    render(<Calendar {...base} buckets={[]} loading />);
    expect(screen.getByText("불러오는 중…")).toBeInTheDocument();
    expect(screen.queryByText("없음")).not.toBeInTheDocument();
  });

  it("집계가 끝난 빈 결과만 없음으로 표시한다", () => {
    render(<Calendar {...base} buckets={[]} loading={false} />);
    expect(screen.getByText("없음")).toBeInTheDocument();
  });

  it("월별 집계를 연도별 합계로 묶는다", () => {
    render(
      <Calendar
        {...base}
        loading={false}
        buckets={[
          { year: 2025, month: 12, count: 3, top: 1 },
          { year: 2024, month: 2, count: 5, top: 2 },
          { year: 2024, month: 1, count: 7, top: 3 },
        ]}
      />,
    );
    expect(
      screen.getByRole("button", { name: /전체\s*15/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "2025년" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "2024년" })).toBeInTheDocument();
    expect(screen.getByText("12")).toBeInTheDocument();
  });
});

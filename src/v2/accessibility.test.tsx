import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ScrollBar from "./ScrollBar";
import { Row, Toggle } from "./settingsUi";

describe("키보드와 접근성 이름", () => {
  it("설정의 보이는 이름이 스위치 이름이 된다", () => {
    render(
      <Row label="폴더 감시" hint="바뀐 사진을 반영합니다">
        <Toggle k="watch" />
      </Row>,
    );
    expect(screen.getByRole("switch", { name: "폴더 감시" })).toBeInTheDocument();
  });

  it("촬영일 타임라인은 키보드로 다음 달과 끝으로 이동한다", () => {
    const onSeek = vi.fn();
    render(
      <ScrollBar
        buckets={[
          { year: 2024, month: 1, count: 10, top: 0 },
          { year: 2024, month: 2, count: 20, top: 10 },
        ]}
        offset={0}
        pageSize={5}
        onSeek={onSeek}
      />,
    );
    const timeline = screen.getByRole("slider", { name: "사진 촬영일 타임라인" });
    fireEvent.keyDown(timeline, { key: "ArrowDown" });
    expect(onSeek).toHaveBeenLastCalledWith(10);
    fireEvent.keyDown(timeline, { key: "End" });
    expect(onSeek).toHaveBeenLastCalledWith(25);
  });

  it("빈 타임라인에 남은 초점의 방향키가 뒤 격자로 새지 않는다", () => {
    const behind = vi.fn();
    const onSeek = vi.fn();
    render(
      <div onKeyDown={behind}>
        <ScrollBar buckets={[]} offset={0} pageSize={1} onSeek={onSeek} />
      </div>,
    );
    const timeline = screen.getByRole("slider", {
      name: "사진 촬영일 타임라인",
    });
    fireEvent.keyDown(timeline, { key: "ArrowDown" });
    expect(behind).not.toHaveBeenCalled();
    expect(onSeek).not.toHaveBeenCalled();
  });
});

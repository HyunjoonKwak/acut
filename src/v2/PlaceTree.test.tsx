import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import PlaceTree from "./PlaceTree";
import { useData } from "./dataStore";
import { EMPTY } from "./picks";

describe("위치 갈래의 지명 상태", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset().mockResolvedValue([]);
    useData.setState({ geoRev: 0 });
  });

  it("서버가 이름을 못 찾은 사진에는 실행할 수 없는 채우기 안내를 하지 않는다", async () => {
    render(
      <PlaceTree
        picks={EMPTY}
        facetFilter={{}}
        pending={0}
        unavailable={2}
        onPick={vi.fn()}
      />,
    );

    expect(
      await screen.findByText(/모든 라이브러리에서 서버가 지명을 찾지 못한 사진 2장/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/지명 채우기/)).not.toBeInTheDocument();
  });

  it("실제로 조회할 사진이 있을 때만 지명 채우기를 안내한다", async () => {
    render(
      <PlaceTree
        picks={EMPTY}
        facetFilter={{}}
        pending={3}
        unavailable={0}
        onPick={vi.fn()}
      />,
    );

    expect(
      await screen.findByText(/모든 라이브러리에서 아직 지명 처리가 필요한 사진 3장/),
    ).toBeInTheDocument();
    expect(screen.getByText(/지명 채우기/)).toBeInTheDocument();
  });
});

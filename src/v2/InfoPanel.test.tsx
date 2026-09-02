import { act, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import InfoPanel from "./InfoPanel";
import type { Detail } from "./detailText";
import type { FileRow } from "./types";

const file = (id: number): FileRow => ({
  id,
  name: `${id}.jpg`,
  taken_at: 0,
  taken_at_source: 0,
  kind: 0,
  size: 10,
  width: 10,
  height: 10,
  rating: 0,
  culling_flag: 0,
  favorite: false,
  duration_ms: null,
  group: null,
  library_id: 1,
  thumb: null,
  iso: null,
  aperture: null,
  shutter: null,
  focal_mm: null,
  cam_model: null,
});

const detail = (id: number, comment: string): Detail => ({
  name: `${id}.jpg`,
  folder: "",
  size: 10,
  takenAt: 0,
  takenAtSource: 0,
  width: 10,
  height: 10,
  camMake: null,
  camModel: null,
  lens: null,
  iso: null,
  aperture: null,
  shutter: null,
  focalMm: null,
  durationMs: null,
  rating: 0,
  cullingFlag: 0,
  favorite: false,
  kind: 0,
  comment,
});

describe("정보 패널 사진 전환", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("새 사진을 읽는 동안 앞 사진의 메모를 새 편집기에 넣지 않는다", async () => {
    const pending = new Map<number, (d: Detail) => void>();
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "file_detail")
        return new Promise((resolve) => pending.set((args as { id: number }).id, resolve));
      if (cmd === "tags_of" || cmd === "tags_list") return [];
      return null;
    });

    const view = render(<InfoPanel file={file(1)} onClose={vi.fn()} />);
    await act(async () => pending.get(1)?.(detail(1, "첫 사진 메모")));
    expect(screen.getByRole("textbox", { name: "코멘트" })).toHaveValue("첫 사진 메모");

    view.rerender(<InfoPanel file={file(2)} onClose={vi.fn()} />);
    expect(screen.queryByRole("textbox", { name: "코멘트" })).not.toBeInTheDocument();
    expect(screen.getByText("읽는 중…")).toBeInTheDocument();

    await act(async () => pending.get(2)?.(detail(2, "둘째 사진 메모")));
    expect(screen.getByRole("textbox", { name: "코멘트" })).toHaveValue("둘째 사진 메모");
  });
});

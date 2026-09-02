import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TagEditor from "./TagEditor";

describe("TagEditor 사진 전환", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("이전 사진의 늦은 reload가 새 사진 태그를 지우지 않는다", async () => {
    let firstLoads = 0;
    let resolveLate: ((tags: { id: number; name: string }[]) => void) | undefined;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "tags_list") return [];
      if (command === "tag_remove") return null;
      if (command === "tags_of" && (args as { id: number }).id === 1) {
        firstLoads += 1;
        if (firstLoads === 1) return [{ id: 10, name: "이전" }];
        return new Promise((resolve) => { resolveLate = resolve; });
      }
      if (command === "tags_of") return [{ id: 20, name: "새사진" }];
      return null;
    });

    const view = render(<TagEditor id={1} />);
    expect(await screen.findByText("이전")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "이전 태그 떼기" }));
    await waitFor(() => expect(resolveLate).toBeDefined());
    view.rerender(<TagEditor id={2} />);
    expect(await screen.findByText("새사진")).toBeInTheDocument();
    await act(async () => resolveLate?.([]));
    expect(screen.getByText("새사진")).toBeInTheDocument();
  });
});

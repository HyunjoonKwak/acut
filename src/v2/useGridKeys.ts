import { useEffect } from "react";
import { useSelection } from "./selectionStore";
import { useOverlayOpen, useUi } from "./uiStore";
import type { FileRow, Mark } from "./types";

/**
 * 그리드 키보드 — 뷰어를 열지 않고도 판정하고 옮겨 다닌다.
 * 뷰어와 같은 배열이라 손이 기억한 대로 눌린다.
 *
 * 위에 무언가 떠 있으면 그쪽이 키를 맡는다. 나란히 보기는 자기 키를 따로
 * 듣고, 단축키 한 장은 Esc만 받으면 된다.
 */
export function useGridKeys(opts: {
  rows: FileRow[];
  cols: number;
  compareIds: number[];
  markOne: (id: number, patch: Mark) => void;
  undoLast: () => void;
}) {
  const overlay = useOverlayOpen();
  const helping = useUi((s) => s.helping);
  const { rows, cols, compareIds, markOne, undoLast } = opts;

  useEffect(() => {
    if (overlay) return;
    const onKey = (e: KeyboardEvent) => {
      // 찾기 입력칸에 쓰는 중이면 가로채지 않는다
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;

      const sel = useSelection.getState();
      const ui = useUi.getState();
      const i =
        sel.selected === null
          ? -1
          : rows.findIndex((r) => r.id === sel.selected);
      const move = (d: number) => {
        const n = i < 0 ? 0 : i + d;
        if (n < 0 || n >= rows.length) return;
        e.preventDefault();
        sel.moveTo(rows[n].id, e.shiftKey);
      };

      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        undoLast();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "a") {
        e.preventDefault();
        sel.setPicked(rows.map((r) => r.id));
        return;
      }
      if (e.key === "?") {
        e.preventDefault();
        ui.set({ helping: !ui.helping });
        return;
      }
      if (helping) {
        // 한 장이 떠 있는 동안에는 Esc 말고는 아무것도 안 듣는다 —
        // 안 보이는 사진에 판정이 찍히면 알아챌 방법이 없다.
        if (e.key === "Escape") {
          e.preventDefault();
          ui.set({ helping: false });
        }
        return;
      }
      if (e.key === "c" && compareIds.length >= 2) {
        e.preventDefault();
        ui.set({ comparing: compareIds });
        return;
      }
      if (e.key === "Escape" && sel.picked.size > 0) {
        e.preventDefault();
        sel.clearPicked();
        return;
      }
      switch (e.key) {
        case " ":
        case "Enter":
          if (i < 0) return;
          e.preventDefault();
          ui.set({ viewerAt: i });
          return;
        case "ArrowRight":
          return move(1);
        case "ArrowLeft":
          return move(-1);
        case "ArrowDown":
          return move(cols);
        case "ArrowUp":
          return move(-cols);
      }
      if (i < 0) return;
      const r = rows[i];
      if (/^[0-5]$/.test(e.key)) markOne(r.id, { rating: +e.key });
      else if (e.key === "p")
        markOne(r.id, { cullingFlag: r.culling_flag === 1 ? 0 : 1 });
      else if (e.key === "x")
        markOne(r.id, { cullingFlag: r.culling_flag === 2 ? 0 : 2 });
      else if (e.key === "f") markOne(r.id, { favorite: !r.favorite });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [overlay, helping, rows, cols, compareIds, markOne, undoLast]);
}

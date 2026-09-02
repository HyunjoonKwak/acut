import { useEffect } from "react";
import { useSelection } from "./selectionStore";
import { useOverlayOpen, useUi } from "./uiStore";
import { usePrefs } from "./prefs";
import { toast } from "./toastStore";
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
  markOne: (id: number, patch: Mark) => Promise<void>;
  /** 여러 장을 골랐을 때 — 화면(선택 패널·메뉴)이 P·X·F를 «고른 것»에 붙여 두었다 */
  markMany: (ids: number[], patch: Mark) => Promise<void>;
  undoLast: () => void;
}) {
  const overlay = useOverlayOpen();
  const helping = useUi((s) => s.helping);
  const { rows, cols, compareIds, markOne, markMany, undoLast } = opts;

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
      // 정보 패널 — 뷰어의 I와 같은 키
      if (e.key === "i" && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        const p = usePrefs.getState();
        p.set("infoPanel", !p.infoPanel);
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
      // 여러 장을 골라 두었으면 그 전부에 — 화면(선택 패널·메뉴)과 같은 뜻으로 **세운다**.
      // 초점 한 장 기준으로 켜고 끄면 초점이 이미 «남김»일 때 1,199장의 판정이 지워진다 (리뷰 H1).
      // 대상은 고른 수로만 정한다 — 초점이 고른 것 밖이어도 고른 것에 붙는다
      const many = sel.picked.size > 1;
      const mark = (patch: Mark) =>
        many
          ? void markMany([...sel.picked], patch).catch((err) => toast(String(err), "drop"))
          : void markOne(r.id, patch).catch((err) => toast(String(err), "drop"));
      if (/^[0-5]$/.test(e.key)) mark({ rating: +e.key });
      else if (e.key === "p")
        mark({ cullingFlag: many ? 1 : r.culling_flag === 1 ? 0 : 1 });
      else if (e.key === "x")
        mark({ cullingFlag: many ? 2 : r.culling_flag === 2 ? 0 : 2 });
      else if (e.key === "f") mark({ favorite: many ? true : !r.favorite });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [overlay, helping, rows, cols, compareIds, markOne, markMany, undoLast]);
}

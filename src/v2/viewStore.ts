import { useMemo } from "react";
import { create } from "zustand";
import { EMPTY, picksFrom, type Picks } from "./picks.ts";
import { usePrefs, usePref } from "./prefs.ts";
import type { Sort } from "./sortItems.ts";

/**
 * 무엇을 보고 있나 — 폴더·조건·휴지통.
 *
 * 켰다 꺼도 남는 것(정렬·라이브러리)은 prefs에, 그때그때 거는 것은 여기에.
 * 둘을 합친 것이 백엔드에 보내는 `filter`다 (`useFilter`).
 */

/** 사이드바에서 고른 폴더. `libId`는 그 폴더가 든 라이브러리 —
 *  두 라이브러리에 같은 rel_path가 있을 수 있어 짝으로 들고 있어야 한다. */
export type Sel = { libId: number; path: string; rel: string } | null;

type Store = {
  sel: Sel;
  picks: Picks;
  viewTrash: boolean;
  setSel: (s: Sel) => void;
  setPicks: (p: Picks) => void;
  patchPicks: (p: Partial<Picks>) => void;
  setViewTrash: (v: boolean) => void;
  /** 「모든 사진」 — 라이브러리·폴더·조건을 다 푼다 */
  showAll: () => void;
  /** 스마트 앨범을 편다 — 저장해 둔 조건을 그대로 건다 */
  applySmart: (filter: unknown, sort: unknown) => void;
};

export const useView = create<Store>()((set) => ({
  sel: null,
  picks: EMPTY,
  viewTrash: false,
  setSel: (sel) => set({ sel }),
  setPicks: (picks) => set({ picks }),
  patchPicks: (p) => set((s) => ({ picks: { ...s.picks, ...p } })),
  setViewTrash: (viewTrash) => set({ viewTrash }),
  showAll: () => {
    usePrefs.getState().set("libId", null);
    set({ sel: null, picks: EMPTY, viewTrash: false });
  },
  // 저장한 것은 Filter 통짜라 라이브러리·폴더·정렬까지 들어 있다. 조건만
  // 골라 담지 않고 전부 되돌리는 편이 «그때 보던 그대로»에 가깝다.
  applySmart: (f, srt) => {
    const v = (f ?? {}) as {
      library_id?: number | null;
      folder_path?: string | null;
      trashed?: boolean;
    };
    const prefs = usePrefs.getState();
    prefs.set("libId", v.library_id ?? null);
    if (srt) prefs.set("sort", srt as Sort);
    set({
      picks: picksFrom(f),
      // 폴더는 경로만 되살린다 — 트리에서 고른 것과 같은 모양이면 충분하다
      sel:
        v.folder_path && v.library_id != null
          ? { libId: v.library_id, path: v.folder_path, rel: v.folder_path }
          : null,
      viewTrash: v.trashed ?? false,
    });
  },
}));

/** 백엔드 `db::query::Filter`. 조건 + 정렬 + 라이브러리 + 폴더 + 휴지통. */
export type Filter = Picks & {
  sort: Sort;
  library_id: number | null;
  folder_path: string | null;
  trashed: boolean;
};

export function useFilter(): Filter {
  const sel = useView((s) => s.sel);
  const picks = useView((s) => s.picks);
  const viewTrash = useView((s) => s.viewTrash);
  const [sort] = usePref("sort");
  const [libId] = usePref("libId");
  return useMemo(
    () => ({
      ...picks,
      sort,
      // 폴더를 고르면 그 폴더가 든 라이브러리로 좁힌다
      library_id: sel ? sel.libId : libId,
      folder_path: viewTrash ? null : (sel?.rel ?? null),
      trashed: viewTrash,
    }),
    [picks, sort, libId, sel, viewTrash],
  );
}

/** 갈래 목록을 셀 때 쓰는 필터. 그 갈래 자신은 빼야 다른 값도 보인다. */
export const facetOf = (f: Filter): Filter => ({
  ...f,
  year: null,
  month: null,
  camera: null,
  min_rating: null,
  tag_id: null,
  place: null,
});

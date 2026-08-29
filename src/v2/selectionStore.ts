import { create } from "zustand";

/**
 * 초점과 선택 — 다른 일이다.
 *
 * 초점(`selected`)은 키보드·뷰어·상태바가 기준으로 삼는 **한 장**.
 * 선택(`picked`)은 정리·판정을 한꺼번에 먹일 **여러 장**.
 */
type Store = {
  selected: number | null;
  picked: Set<number>;
  setSelected: (id: number | null) => void;
  setPicked: (ids: Iterable<number>) => void;
  clearPicked: () => void;
  /** 타일을 누를 때. ⌘은 하나씩 더하고, ⇧는 기준점부터 여기까지. */
  pick: (
    id: number,
    mods: { meta: boolean; shift: boolean },
    order: number[],
  ) => void;
  /** 키보드로 옮길 때. ⇧를 잡고 있으면 묶음이 늘어난다. */
  moveTo: (id: number, extend: boolean) => void;
  /** 목록이 바뀌었을 때 — 초점이 사라졌으면 첫 장으로. 선택은 안 건드린다. */
  focusWithin: (order: number[]) => void;
};

export const useSelection = create<Store>()((set) => ({
  selected: null,
  picked: new Set(),
  setSelected: (selected) => set({ selected }),
  setPicked: (ids) => set({ picked: new Set(ids) }),
  clearPicked: () => set({ picked: new Set() }),
  pick: (id, { meta, shift }, order) =>
    set((s) => {
      if (meta) {
        const next = new Set(s.picked);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return { picked: next, selected: id };
      }
      if (shift && s.selected !== null) {
        const a = order.indexOf(s.selected);
        const b = order.indexOf(id);
        if (a >= 0 && b >= 0) {
          const [lo, hi] = a < b ? [a, b] : [b, a];
          return { picked: new Set(order.slice(lo, hi + 1)), selected: id };
        }
      }
      return { picked: new Set([id]), selected: id };
    }),
  moveTo: (id, extend) =>
    set((s) => ({
      selected: id,
      picked: extend ? new Set([...s.picked, id]) : new Set([id]),
    })),
  focusWithin: (order) =>
    set((s) => {
      if (order.length === 0) return s;
      // 고른 것도 목록 안의 것만 — 다른 폴더로 옮겨 가서 «제외»를 누르면 보이지 않는
      // 1,200장에 찍히던 길 (리뷰 H8)
      const inList = new Set(order);
      const picked =
        [...s.picked].every((id) => inList.has(id))
          ? s.picked
          : new Set([...s.picked].filter((id) => inList.has(id)));
      const selected =
        s.selected !== null && inList.has(s.selected) ? s.selected : order[0];
      if (picked === s.picked && selected === s.selected) return s;
      return { selected, picked };
    }),
}));

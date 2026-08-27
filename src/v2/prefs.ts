import { useCallback } from "react";
import { create } from "zustand";
import {
  createJSONStorage,
  persist,
  type StateStorage,
} from "zustand/middleware";
import { invoke } from "@tauri-apps/api/core";
import type { GridStyle, Scaling } from "./gridStyle";
import type { GroupBy } from "./GroupMenu";
import type { Source } from "./railItems";
import { DEFAULT_SORT, type Sort } from "./sortItems";

/**
 * 켰다 꺼도 남는 것들 — 보기 방식·크기·정렬·사이드바.
 *
 * 켤 때마다 초기화되는 앱은 매일 쓰기 시작하면 첫날 걸린다. Lap은 전부
 * 남긴다. 값은 DB의 `settings` 테이블에 JSON 한 줄로 든다 — 사진 판정과
 * 같은 파일이라 백업 한 벌에 같이 실린다.
 *
 * App.tsx의 useState 열한 개가 여기로 왔다. 구조 정리(1단계)의 첫 걸음이다.
 */
export type Prefs = {
  thumbSize: number;
  gridStyle: GridStyle;
  scaling: Scaling;
  caption: boolean;
  filmstrip: boolean;
  panelW: number;
  panelOpen: boolean;
  source: Source;
  sort: Sort;
  group: GroupBy;
  /** 마지막에 보던 라이브러리. 없어졌으면 목록이 비어 알 수 있다. */
  libId: number | null;
};

export const DEFAULT_PREFS: Prefs = {
  thumbSize: 180,
  gridStyle: "card",
  scaling: "cover",
  caption: true,
  filmstrip: false,
  panelW: 224,
  panelOpen: true,
  source: "all",
  sort: DEFAULT_SORT,
  group: "none",
  libId: null,
};

type Store = Prefs & {
  set: <K extends keyof Prefs>(key: K, value: Prefs[K]) => void;
};

/** settings 테이블을 저장소로. 한 열쇠에 통째로 든다. */
const tauriStorage: StateStorage = {
  getItem: (name) => invoke<string | null>("settings_get", { key: name }),
  setItem: (name, value) => invoke("settings_set", { key: name, value }),
  removeItem: (name) => invoke("settings_remove", { key: name }),
};

export const usePrefs = create<Store>()(
  persist(
    (set) => ({
      ...DEFAULT_PREFS,
      set: (key, value) => set({ [key]: value } as Partial<Prefs>),
    }),
    {
      name: "prefs",
      version: 1,
      storage: createJSONStorage(() => tauriStorage),
      // 함수는 저장하지 않는다
      partialize: (s) =>
        Object.fromEntries(
          (Object.keys(DEFAULT_PREFS) as (keyof Prefs)[]).map((k) => [k, s[k]]),
        ) as Prefs,
      // 옛 저장본에 없는 열쇠는 기본값으로 — 조건이 늘어도 깨지지 않는다
      merge: (saved, cur) => ({
        ...cur,
        ...DEFAULT_PREFS,
        ...(saved as Partial<Prefs>),
      }),
    },
  ),
);

/**
 * `useState`처럼 쓰는 한 값. `const [thumbSize, setThumbSize] = usePref("thumbSize")`.
 * 값이 바뀔 때만 다시 그린다.
 */
export function usePref<K extends keyof Prefs>(
  key: K,
): [Prefs[K], (v: Prefs[K]) => void] {
  const value = usePrefs((s) => s[key]);
  const set = usePrefs((s) => s.set);
  const setter = useCallback((v: Prefs[K]) => set(key, v), [key, set]);
  return [value, setter];
}

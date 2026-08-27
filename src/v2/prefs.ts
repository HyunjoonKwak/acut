import { useCallback } from "react";
import { create } from "zustand";
import {
  createJSONStorage,
  persist,
  type StateStorage,
} from "zustand/middleware";
import { invoke } from "@tauri-apps/api/core";
import type { GridStyle } from "./gridStyle.ts";
import type { GroupBy } from "./groupItems.ts";
import type { Source } from "./railItems.ts";
import { DEFAULT_SORT, type Sort } from "./sortItems.ts";

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
  caption: boolean;
  filmstrip: boolean;
  panelW: number;
  panelOpen: boolean;
  source: Source;
  sort: Sort;
  group: GroupBy;
  /** 마지막에 보던 라이브러리. 없어졌으면 목록이 비어 알 수 있다. */
  libId: number | null;
  /** 폴더 감시 — 파인더로 넣은 사진이 저절로 나타난다 */
  watch: boolean;
};

export const DEFAULT_PREFS: Prefs = {
  thumbSize: 180,
  gridStyle: "card",
  caption: true,
  filmstrip: false,
  panelW: 224,
  panelOpen: true,
  source: "all",
  sort: DEFAULT_SORT,
  group: "none",
  libId: null,
  watch: true,
};

type Store = Prefs & {
  set: <K extends keyof Prefs>(key: K, value: Prefs[K]) => void;
};

/**
 * settings 테이블을 저장소로. 한 열쇠에 통째로 든다.
 *
 * Tauri가 없는 곳(node 테스트, 브라우저 미리보기)에서는 조용히 기본값으로
 * 간다 — 여기서 던지면 스토어를 import한 테스트가 통째로 죽는다.
 */
const tauriStorage: StateStorage = {
  getItem: async (name) => {
    try {
      return await invoke<string | null>("settings_get", { key: name });
    } catch {
      return null;
    }
  },
  setItem: async (name, value) => {
    try {
      await invoke("settings_set", { key: name, value });
    } catch {
      /* Tauri 밖 */
    }
  },
  removeItem: async (name) => {
    try {
      await invoke("settings_remove", { key: name });
    } catch {
      /* Tauri 밖 */
    }
  },
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

import { useSyncExternalStore } from "react";

/**
 * 창 너비 — 좁아질 때 «접히는 순서 사다리»(2026-08-31)를 태우는 기준.
 *
 * 단계: <1280 단추 라벨·(<1080) 슬라이더 접힘 → <1080 브레드크럼 «… › 현재 폴더» 압축 ·
 * 보기 아이콘만 → <880 필터 칩 «필터 N» 접힘, 상태바 곁가지는 ⋯ 메뉴.
 * 최소 창 폭은 tauri.conf.json 이 800 으로 못박는다.
 */
const subscribe = (cb: () => void) => {
  window.addEventListener("resize", cb);
  return () => window.removeEventListener("resize", cb);
};

export function useViewportW(): number {
  return useSyncExternalStore(subscribe, () => window.innerWidth);
}

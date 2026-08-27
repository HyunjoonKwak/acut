import { create } from "zustand";
import type { MenuAt } from "./ContextMenu";

/**
 * 위에 무엇이 떠 있나 — 뷰어·상자·메뉴.
 *
 * 키보드 핸들러가 «지금 누가 키를 맡나»를 여기서 본다. 한 군데 모아 두면
 * 새 상자를 붙일 때 핸들러를 잊지 않는다.
 */
type Store = {
  /** 뷰어에 띄운 사진의 rows 안 위치. null이면 닫힌 상태 */
  viewerAt: number | null;
  /** 뷰어를 창 전체로 — 기본은 사이드바를 남겨 둔다 */
  viewerFull: boolean;
  organizing: boolean;
  /** 나란히 보기 — 골라 둔 것 중 앞의 넷. null이면 닫힌 상태 */
  comparing: number[] | null;
  helping: boolean;
  importing: boolean;
  culling: boolean;
  /** 우클릭 메뉴가 뜬 자리와 그때 잡힌 사진들 */
  ctxAt: MenuAt;
  ctxIds: number[];
  /** 「⋯」를 연 라이브러리 (앨범 트리) */
  menuFor: number | null;
  /** 이름을 바꾸는 중인 사진 */
  renaming: number | null;
  /** 파인더에서 끌어다 놓은 것들 — 가져오기 상자의 시작점 */
  dropped: string[];
  /** 창 위에 무언가를 끌고 있는 중 */
  dragging: boolean;
  set: (p: Partial<Omit<Store, "set">>) => void;
};

export const useUi = create<Store>()((set) => ({
  viewerAt: null,
  viewerFull: false,
  organizing: false,
  comparing: null,
  helping: false,
  importing: false,
  culling: false,
  ctxAt: null,
  ctxIds: [],
  menuFor: null,
  renaming: null,
  dropped: [],
  dragging: false,
  set: (p) => set(p),
}));

/** 무언가 떠 있어 그리드가 키를 맡지 않아야 하는가 */
export const useOverlayOpen = () =>
  useUi(
    (s) =>
      s.viewerAt !== null || s.culling || s.organizing || s.comparing !== null,
  );

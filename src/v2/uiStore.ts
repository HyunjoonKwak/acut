import { create } from "zustand";
import type { MenuAt } from "./ContextMenu";
import type { FolderOperationTarget } from "./FolderOperationDialog";

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
  /** 선택 사진 또는 폴더의 촬영일 감사·교정 */
  captureDate: { ids: number[]; libraryId?: number; relPath?: string } | null;
  /** 선택 사진을 기존/새 폴더로 이동·복사 */
  transfer: { ids: number[]; sourceLibraryId: number } | null;
  /** 생성·이름변경·이동·복사·휴지통 폴더 작업 */
  folderOperation: FolderOperationTarget | null;
  /** 라이브러리 등록 중 — 영역을 고르는 폴더 */
  areaPick: string | null;
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
  /** 「⋯」를 연 폴더 (앨범 트리) — 트리 경로 */
  folderMenu: string | null;
  /** 다른 디스크로 옮기는 중인 폴더 */
  offload: { folderId: number; name: string; libraryId: number } | null;
  /** 사진 없는 폴더 정리 창 — 라이브러리 ⋯ 메뉴에서 */
  husks: { libraryId: number; name: string } | null;
  /** 이름을 바꾸는 중인 사진 */
  renaming: number | null;
  /** 파인더에서 끌어다 놓은 것들 — 가져오기 상자의 시작점 */
  dropped: string[];
  /** 창 위에 무언가를 끌고 있는 중 */
  dragging: boolean;
  /** 「비슷한 사진」의 기준 사진 */
  similarFor: number | null;
  /** 글로 찾기 — 물은 글. null이면 닫힘 */
  textSearch: string | null;
  set: (p: Partial<Omit<Store, "set">>) => void;
};

export const useUi = create<Store>()((set) => ({
  viewerAt: null,
  viewerFull: false,
  organizing: false,
  captureDate: null,
  transfer: null,
  folderOperation: null,
  areaPick: null,
  comparing: null,
  helping: false,
  importing: false,
  culling: false,
  ctxAt: null,
  ctxIds: [],
  menuFor: null,
  folderMenu: null,
  offload: null,
  husks: null,
  renaming: null,
  dropped: [],
  dragging: false,
  similarFor: null,
  textSearch: null,
  set: (p) => set(p),
}));

/** 무언가 떠 있어 그리드가 키를 맡지 않아야 하는가 */
/** 위에 무언가 떠 있어 격자가 키를 받으면 안 되는 상태 — 비슷한 사진·글로 찾기·가져오기·
 *  이름 바꾸기·옮겨두기·구역 고르기·우클릭 메뉴까지. 빠뜨리면 Esc 가 선택을 지우고
 *  x·p·숫자키가 뒤의 격자에 찍힌다 (리뷰 MEDIUM) */
export const useOverlayOpen = () =>
  useUi(
    (s) =>
      s.viewerAt !== null ||
      s.culling ||
      s.organizing ||
      s.captureDate !== null ||
      s.transfer !== null ||
      s.folderOperation !== null ||
      s.comparing !== null ||
      s.similarFor !== null ||
      s.textSearch !== null ||
      s.importing ||
      s.renaming !== null ||
      s.offload !== null ||
      s.husks !== null ||
      s.areaPick !== null ||
      s.ctxAt !== null,
  );

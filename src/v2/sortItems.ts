/**
 * 정렬 갈래 — 값과 이름만. 그림은 SortMenu.tsx.
 *
 * 컴포넌트 파일에서 상수를 내보내면 Fast Refresh가 화면 상태를 날린다.
 */

/** 백엔드 `db::query::SortBy`와 이름이 같아야 한다 */
export type SortBy =
  | "taken_at"
  | "created_at"
  | "modified_at"
  | "name"
  | "size"
  | "pixels"
  | "duration";

export type Sort = { by: SortBy; desc: boolean };

export const DEFAULT_SORT: Sort = { by: "taken_at", desc: true };

/** Lap의 정렬 목록과 같다 */
export const SORT_ITEMS: { by: SortBy; label: string }[] = [
  { by: "taken_at", label: "촬영일" },
  { by: "created_at", label: "생성일" },
  { by: "modified_at", label: "수정일" },
  { by: "name", label: "이름" },
  { by: "size", label: "크기" },
  { by: "pixels", label: "픽셀 크기" },
  { by: "duration", label: "재생시간" },
];

export const sortLabel = (s: Sort): string =>
  SORT_ITEMS.find((i) => i.by === s.by)?.label ?? "정렬";

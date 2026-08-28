/**
 * 레일에 놓는 갈래들 — 값과 이름만.
 *
 * 컴포넌트 파일에서 상수까지 내보내면 Fast Refresh가 파일 전체를 다시
 * 얹으면서 화면 상태가 날아간다. 그래서 데이터는 여기, 그림은 Rail.tsx.
 * (그림 자체는 icons.tsx에 있고 `icon` 열쇠로 짝지운다.)
 */

export type Source =
  | "all"
  | "album"
  | "smart"
  | "search"
  | "tag"
  | "people"
  | "calendar"
  | "location"
  | "camera"
  | "trash"
  | "settings";

/**
 * 레일은 아이콘만 보인다. `label`은 커서를 올렸을 때 뜨는 이름이고,
 * `title`은 사이드바 패널의 머리다.
 */
export type Entry = { v: Source; label: string; title?: string };

export const SOURCES: Entry[] = [
  { v: "all", label: "모든 사진" },
  { v: "album", label: "앨범" },
  { v: "smart", label: "스마트 앨범" },
  { v: "search", label: "검색" },
  { v: "tag", label: "태그" },
  { v: "people", label: "사람" },
  { v: "calendar", label: "달력" },
  { v: "location", label: "위치" },
  { v: "camera", label: "카메라" },
];

/** 성격이 달라 아래쪽에 따로 놓는 것들 */
export const FOOT: Entry[] = [
  { v: "trash", label: "휴지통" },
  { v: "settings", label: "설정" },
];

export const ALL_SOURCES: Entry[] = [...SOURCES, ...FOOT];

/** 패널 머리에 쓸 이름 */
export const sourceTitle = (v: Source): string => {
  const s = ALL_SOURCES.find((x) => x.v === v);
  return s?.title ?? s?.label ?? "";
};

/**
 * 레일에 놓는 갈래들 — 값과 이름만.
 *
 * 컴포넌트 파일에서 상수까지 내보내면 Fast Refresh가 파일 전체를 다시
 * 얹으면서 화면 상태가 날아간다. 그래서 데이터는 여기, 그림은 Rail.tsx.
 */

export type Source =
  | "all"
  | "album"
  | "smart"
  | "search"
  | "tag"
  | "calendar"
  | "location"
  | "camera"
  | "trash"
  | "settings";

/** `label`은 레일의 좁은 칸에, `title`은 패널 머리에 쓴다 */
export type Entry = {
  v: Source;
  icon: string;
  label: string;
  title?: string;
};

export const SOURCES: Entry[] = [
  { v: "all", icon: "▦", label: "모든", title: "모든 사진" },
  { v: "album", icon: "🗀", label: "앨범" },
  { v: "smart", icon: "✦", label: "스마트", title: "스마트 앨범" },
  { v: "search", icon: "⌕", label: "검색" },
  { v: "tag", icon: "🏷", label: "태그" },
  { v: "calendar", icon: "🗓", label: "달력" },
  { v: "location", icon: "📍", label: "위치" },
  { v: "camera", icon: "📷", label: "카메라" },
];

/** 성격이 달라 아래쪽에 따로 놓는 것들 */
export const FOOT: Entry[] = [
  { v: "trash", icon: "🗑", label: "휴지통" },
  { v: "settings", icon: "⚙", label: "설정" },
];

export const ALL_SOURCES: Entry[] = [...SOURCES, ...FOOT];

/** 패널 머리에 쓸 이름 */
export const sourceTitle = (v: Source): string => {
  const s = ALL_SOURCES.find((x) => x.v === v);
  return s?.title ?? s?.label ?? "";
};

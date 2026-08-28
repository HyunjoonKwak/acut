/**
 * 사람 갈래의 값과 이름 — 그림은 PeoplePanel.tsx.
 */
export type Person = {
  id: number;
  name: string | null;
  count: number;
  /** 대표 얼굴이 든 썸네일 — `라이브러리/상대경로` */
  cover_thumb: string | null;
  /** 그 안의 얼굴 상자 — 비율 0~1 */
  cover_bbox: { x: number; y: number; w: number; h: number } | null;
};

/** 이름이 없으면 번호로 부른다 — 이름을 붙이기 전에도 서로 구별은 돼야 한다 */
export const personLabel = (p: Person): string =>
  p.name?.trim() || `이름 없음 #${p.id}`;

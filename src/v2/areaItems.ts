/**
 * 영역 — 라이브러리의 역할. 값과 이름만, 그림은 AlbumTree·Organize.
 *
 * 이 앱에서 **물리적 위치가 곧 처리 단계**다. 사진은 작업대에 들어와
 * 고르기를 거쳐 내사진으로, 나눌 것은 공용으로 옮겨 간다.
 * 작업대 → 공용 직행도, 공용 → 내사진 내리기도 된다 (관리자는 나 하나).
 */
export type Area = 0 | 1 | 2 | 3;

export type AreaEntry = { v: Area; label: string; hint: string };

export const AREAS: AreaEntry[] = [
  { v: 0, label: "작업대", hint: "처리 대기 — 고르고 정리해서 비우는 곳" },
  { v: 1, label: "내사진", hint: "정리 끝난 내 사진. NAS 개인 폴더와 1:1" },
  { v: 2, label: "공용", hint: "가족과 나누는 사진. NAS 공용 폴더와 1:1" },
  { v: 3, label: "기타", hint: "옛 백업·아카이브처럼 흐름 밖의 것" },
];

export const areaLabel = (v: number): string =>
  AREAS.find((a) => a.v === v)?.label ?? "기타";

/** 정리 대화상자가 처음 고르는 목적지 — 흐름의 다음 칸 */
export const nextArea = (v: number): Area | null =>
  v === 0 ? 1 : v === 1 ? 2 : null;

/**
 * 라이브러리를 뿌리로 한 트리 행들을 영역별로 나눈다. 행은 «뿌리, 그 자손들…»
 * 순으로 이어져 있으니 뿌리를 만날 때마다 그 라이브러리의 영역으로 묶는다.
 */
export function groupByArea<T extends { depth: number; library_id: number }>(
  rows: T[],
  areaOf: (libraryId: number) => number,
): { area: number; rows: T[] }[] {
  const out: { area: number; rows: T[] }[] = [];
  let cur: number | null = null;
  for (const r of rows) {
    if (r.depth === 0) cur = areaOf(r.library_id);
    const a = cur ?? 3;
    // 같은 영역의 라이브러리는 떨어져 있어도 한 묶음
    const g = out.find((x) => x.area === a);
    if (g) g.rows.push(r);
    else out.push({ area: a, rows: [r] });
  }
  return out.sort((x, y) => x.area - y.area);
}

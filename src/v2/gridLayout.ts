/**
 * 그리드 배치 — 머리글과 사진 줄을 하나의 목록으로 편다.
 *
 * 묶기를 켜면 그룹이 바뀌는 자리에 머리글 줄이 들어가고, **그룹마다 새 줄에서
 * 시작**한다. 그래야 머리글 아래 사진이 그 그룹의 것만 보인다.
 *
 * 가상 스크롤이 인덱스로만 위치를 잡으므로, 화면에 그릴 순서를 이렇게 미리
 * 펴 두어야 한다.
 */

export type Row<T> =
  | { kind: "header"; label: string; count: number }
  | { kind: "photos"; items: T[]; start: number };

/**
 * @param files  이미 정렬된 사진들
 * @param group  각 사진의 그룹 값. null이면 묶지 않는다.
 * @param cols   한 줄에 몇 장
 */
export function layout<T>(
  files: T[],
  group: (f: T) => string | null,
  cols: number,
): Row<T>[] {
  if (cols < 1) return [];
  const out: Row<T>[] = [];

  // 묶지 않으면 그냥 cols 개씩 자른다
  if (files.length > 0 && group(files[0]) === null) {
    for (let i = 0; i < files.length; i += cols) {
      out.push({ kind: "photos", items: files.slice(i, i + cols), start: i });
    }
    return out;
  }

  let i = 0;
  while (i < files.length) {
    const key = group(files[i]);
    // 같은 그룹이 어디까지인지 — 정렬돼 있으므로 이어진 구간이다
    let end = i;
    while (end < files.length && group(files[end]) === key) end++;

    out.push({ kind: "header", label: key ?? "", count: end - i });
    for (let j = i; j < end; j += cols) {
      out.push({
        kind: "photos",
        items: files.slice(j, Math.min(j + cols, end)),
        start: j,
      });
    }
    i = end;
  }
  return out;
}

/** 머리글 줄의 높이 (px) */
export const HEADER_H = 30;

/** 그룹 값을 사람이 읽는 말로. `2024-08` → `2024년 8월` */
export function headerLabel(raw: string, by: string): string {
  if (by === "month" && /^\d{4}-\d{2}$/.test(raw)) {
    return `${raw.slice(0, 4)}년 ${Number(raw.slice(5))}월`;
  }
  if (by === "year" && /^\d{4}$/.test(raw)) return `${raw}년`;
  if (by === "day" && /^\d{4}-\d{2}-\d{2}$/.test(raw)) {
    return `${raw.slice(0, 4)}년 ${Number(raw.slice(5, 7))}월 ${Number(raw.slice(8))}일`;
  }
  if (by === "rating") {
    const n = Number(raw);
    return n > 0 ? "★".repeat(n) : "평점 없음";
  }
  if (by === "folder") return raw === "" ? "(최상단)" : raw;
  return raw;
}

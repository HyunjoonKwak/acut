/**
 * 사이드바 폴더 트리의 계산.
 *
 * 백엔드(`db/tree.rs`)가 준 평평한 목록을 접기 상태에 맞춰 걸러 낸다.
 * 트리는 라이브러리를 뿌리로 삼는다 — 뿌리의 경로는 `#<라이브러리 id>`이고
 * 그 아래는 `#3/연도별/2001`처럼 앞에 붙는다. 라이브러리인지 아닌지는
 * 경로 모양이 아니라 `is_library` 값으로 안다 (`#`으로 시작하는 진짜
 * 폴더가 있다).
 */

export type Row = {
  path: string;
  depth: number;
};

/**
 * 접힌 마디의 자식은 빼고 돌려준다.
 *
 * 3,161줄을 통째로 그리면 사이드바가 느려지고 스크롤 막대가 실오라기가 된다.
 * 조상이 **전부** 펴져 있어야 보인다 — 부모만 봐서는 할아버지가 접혔을 때
 * 손자가 떠오른다.
 */
export function visible<T extends Row>(rows: T[], open: Set<string>): T[] {
  return rows.filter((f) => {
    if (f.depth === 0) return true;
    let p = f.path.slice(0, f.path.lastIndexOf("/"));
    while (p) {
      if (!open.has(p)) return false;
      const i = p.lastIndexOf("/");
      p = i < 0 ? "" : p.slice(0, i);
    }
    return true;
  });
}

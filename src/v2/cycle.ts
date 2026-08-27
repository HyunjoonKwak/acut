/** 목록에서 다음 것. 끝이면 처음으로. 보기 방식 버튼이 돌아가는 규칙. */
export function next<T extends { v: string }>(items: T[], cur: string): T {
  const i = items.findIndex((x) => x.v === cur);
  return items[(i + 1) % items.length];
}

/**
 * 시작 구간 표식 — 웹뷰가 뜬 뒤(navigation start) 몇 ms에 무엇이 됐나.
 *
 * 프로세스 시작 시각은 Rust가 안다. 여기 값은 웹뷰 기준이라 둘을 나란히
 * 놓으면 «웹뷰 뜨기 전»과 «뜬 뒤» 어디서 시간이 가는지 갈린다.
 */
const marks: Record<string, number> = {};

export function mark(name: string): void {
  if (!(name in marks)) marks[name] = Math.round(performance.now());
}

export const startupMarks = (): Record<string, number> => ({ ...marks });

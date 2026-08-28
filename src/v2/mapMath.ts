/**
 * 지도의 셈 — 확대 단계에 맞는 격자 크기, 칸의 테두리, 영역 글자.
 * 그림은 MapView.tsx.
 */

/** 확대 단계 → 격자 한 칸의 크기(도). 멀리서는 굵게, 가까이서는 잘게. */
export function precisionForZoom(zoom: number): number {
  if (zoom < 5) return 1;
  if (zoom < 9) return 0.1;
  if (zoom < 13) return 0.01;
  return 0.001;
}

/** 이 칸을 눌렀을 때 — 더 잘게 볼 수 있으면 확대, 아니면 영역으로 고른다 */
export const isFinest = (precision: number): boolean => precision <= 0.001;

/** 좌표가 든 칸의 테두리 [남, 서, 북, 동] */
export function cellBounds(
  lat: number,
  lon: number,
  precision: number,
): [number, number, number, number] {
  const s = Math.floor(lat / precision) * precision;
  const w = Math.floor(lon / precision) * precision;
  return [round(s), round(w), round(s + precision), round(w + precision)];
}

/** `남,서,북,동` — Filter.bbox가 읽는 꼴 */
export const bboxString = (b: [number, number, number, number]): string =>
  b.map(round).join(",");

export function parseBbox(
  s: string | null,
): [number, number, number, number] | null {
  if (!s) return null;
  const v = s.split(",").map(Number);
  if (v.length !== 4 || v.some((x) => Number.isNaN(x))) return null;
  return [v[0], v[1], v[2], v[3]];
}

/** 소수 여섯째 자리 — 1cm 단위. 부동소수 꼬리(37.550000000004)를 자른다 */
const round = (x: number): number => Math.round(x * 1e6) / 1e6;

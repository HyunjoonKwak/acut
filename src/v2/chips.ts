/**
 * 지금 걸린 조건을 사람이 읽는 말로.
 *
 * 사이드바에서 태그나 자리를 고르면 목록이 확 줄어드는데, 화면 어디에도
 * "무엇 때문에 줄었는지"가 없었다. 사이드바를 접으면 단서가 아예 사라진다.
 * 툴바에 조건을 늘어놓고 하나씩 뗄 수 있게 한다.
 */

import { EMPTY, type Picks } from "./picks.ts";

export type Chip = {
  /** 이 조건을 끌 때 되돌릴 열쇠 */
  key: keyof Picks;
  label: string;
};

const KIND = ["사진", "영상", "RAW"];
const FLAG = ["미판정", "남김", "제외"];

/**
 * 자리 값(`37.5,126.9`)을 읽을 수 있는 말로.
 *
 * 백엔드 `FacetKind::Place`의 라벨과 같은 모양이어야 한다 — 사이드바에서
 * 고른 것과 툴바에 뜬 것이 달라 보이면 같은 조건인 줄 모른다.
 */
export function formatPlace(value: string): string {
  if (value === "") return "위치 없음";
  const [a, b] = value.split(",");
  const lat = Number(a);
  const lon = Number(b);
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) return value;
  const ns = lat >= 0 ? "북위" : "남위";
  const ew = lon >= 0 ? "동경" : "서경";
  return `${ns} ${Math.abs(lat).toFixed(1)}° ${ew} ${Math.abs(lon).toFixed(1)}°`;
}

/**
 * 걸린 조건들. 안 걸린 것은 나오지 않는다.
 *
 * @param tagName 태그 id로 이름을 찾는다. 아직 못 읽었으면 id를 그대로 쓴다.
 */
export function chips(
  p: Picks,
  tagName: (id: number) => string | undefined,
): Chip[] {
  const out: Chip[] = [];
  const add = (key: keyof Picks, label: string) => out.push({ key, label });

  if (p.name_like) add("name_like", `"${p.name_like}"`);
  if (p.tag_id !== null) add("tag_id", tagName(p.tag_id) ?? `태그 ${p.tag_id}`);
  if (p.place !== null) add("place", formatPlace(p.place));
  // 날을 고르면 달·연도는 그 안에 이미 들어 있다 — 셋 다 띄우면 겹쳐 보인다
  // `2024-08` → `2024년 8월`. 사이드바 달력이 「8월」로 쓰니 여기도 맞춘다.
  if (p.day) {
    const [y, m, d] = p.day.split("-");
    add("day", `${y}년 ${Number(m)}월 ${Number(d)}일`);
  } else if (p.month) {
    const [y, m] = p.month.split("-");
    add("month", `${y}년 ${Number(m)}월`);
  } else if (p.year) add("year", `${p.year}년`);
  if (p.camera !== null) add("camera", p.camera || "카메라 없음");
  if (p.lens !== null) add("lens", p.lens || "렌즈 없음");
  if (p.kind !== null) add("kind", KIND[p.kind] ?? String(p.kind));
  if (p.culling_flag !== null)
    add("culling_flag", FLAG[p.culling_flag] ?? String(p.culling_flag));
  if (p.min_rating !== null)
    add(
      "min_rating",
      p.min_rating === 0 ? "평점 없음" : `★${p.min_rating} 이상`,
    );
  if (p.favorite_only) add("favorite_only", "♥ 즐겨찾기");
  if (p.no_thumb) add("no_thumb", "썸네일 없음");
  if (p.person_id !== null) add("person_id", "사람");
  if (p.bbox) add("bbox", "지도 영역");
  if (p.nas !== null) add("nas", p.nas ? "NAS에 있음" : "NAS에 없음");

  return out;
}

/**
 * 조건 하나를 뗀다.
 *
 * 달을 떼면 연도도 같이 떨어진다 — 「2024년 8월」을 껐는데 2024년만 남으면
 * 끈 것 같지가 않다.
 */
export function without(p: Picks, key: keyof Picks): Picks {
  const next = { ...p, [key]: EMPTY[key] } as Picks;
  if (key === "month") next.year = null;
  return next;
}

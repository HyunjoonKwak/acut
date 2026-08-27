/**
 * 사용자가 고른 조건 — 백엔드 `db::query::Filter`에서 사람이 만지는 부분만.
 *
 * 라이브러리·폴더·정렬은 화면 상태라 여기 없다. 그 셋을 뺀 나머지가 곧
 * "찾기"다.
 *
 * 컴포넌트 파일이 아니라 여기 두는 이유: 스마트 앨범이 저장한 JSON을 되돌릴
 * 때도 쓰고, 시험도 해야 한다.
 */
export type Picks = {
  /** 0 사진 · 1 영상 · 2 RAW */
  kind: number | null;
  /** 0 미판정 · 1 남김 · 2 제외 */
  culling_flag: number | null;
  /** 이 값 이상만 */
  min_rating: number | null;
  favorite_only: boolean;
  name_like: string | null;
  /** 사이드바에서 고른 연도 (`2024`) */
  year: string | null;
  /** 사이드바에서 고른 달 (`2024-08`) */
  month: string | null;
  /** 사이드바에서 고른 날 (`2024-08-27`) */
  day: string | null;
  /** 사이드바에서 고른 카메라 모델 */
  camera: string | null;
  /** 사이드바에서 고른 렌즈. 빈 문자열이면 «렌즈 없음» */
  lens: string | null;
  /** 사이드바에서 고른 태그 */
  tag_id: number | null;
  /** 사이드바에서 고른 자리 (`37.5,126.9`). 빈 문자열이면 위치 없는 것만 */
  place: string | null;
  /** 썸네일이 없는 것만 — 못 만들었거나 아직 안 만든 것 */
  no_thumb: boolean;
};

export const EMPTY: Picks = {
  kind: null,
  culling_flag: null,
  min_rating: null,
  favorite_only: false,
  name_like: null,
  year: null,
  month: null,
  day: null,
  camera: null,
  lens: null,
  tag_id: null,
  place: null,
  no_thumb: false,
};

/**
 * 조건마다 담기는 값의 형. 스마트 앨범이 돌려준 JSON을 검사할 때 쓴다.
 *
 * `Record<keyof Picks, ...>`라 조건을 늘리면 여기를 안 채우는 순간
 * 빌드가 멈춘다 — 조용히 빠지는 조건이 생기지 않게 하는 장치다.
 */
const TYPES: Record<keyof Picks, "number" | "string" | "boolean"> = {
  kind: "number",
  culling_flag: "number",
  min_rating: "number",
  favorite_only: "boolean",
  name_like: "string",
  year: "string",
  month: "string",
  day: "string",
  camera: "string",
  lens: "string",
  tag_id: "number",
  place: "string",
  no_thumb: "boolean",
};

/** 아무 조건도 안 걸린 상태인가 — 툴바의 표시등이 이걸로 켜진다 */
export const isEmpty = (p: Picks): boolean =>
  (Object.keys(EMPTY) as (keyof Picks)[]).every(
    (k) => p[k] === EMPTY[k] || p[k] === null || p[k] === undefined,
  );

/**
 * 스마트 앨범이 저장해 둔 `Filter` JSON에서 조건만 골라 낸다.
 *
 * 필드를 하나하나 적지 않고 `EMPTY`의 열쇠를 훑는 이유: Picks에 조건이
 * 늘어날 때 여기를 같이 고치는 걸 잊으면, 저장은 되는데 불러올 때 조용히
 * 빠지는 조건이 생긴다.
 */
export function picksFrom(filter: unknown): Picks {
  const src = (filter ?? {}) as Record<string, unknown>;
  const out = { ...EMPTY };
  for (const k of Object.keys(EMPTY) as (keyof Picks)[]) {
    const v = src[k];
    if (v === undefined || v === null) continue;
    // 저장된 JSON은 우리가 쓴 것이지만, 손으로 고쳤을 수도 있다.
    // 형이 안 맞으면 그 조건만 버리고 나머지는 살린다.
    if (typeof v === TYPES[k]) {
      (out as Record<string, unknown>)[k] = v;
    }
  }
  return out;
}

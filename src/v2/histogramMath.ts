/**
 * 히스토그램 — 밝기 분포를 세는 셈.
 *
 * 판정할 때 눈으로는 잘 안 보이는 것이 두 가지 있다: 하이라이트가 날아간
 * 것과 그림자가 뭉갠 것. 작은 화면에서는 특히 그렇다. 분포를 보면 한눈에
 * 안다.
 *
 * 그리는 일과 세는 일을 나눠 둔다 — 세는 쪽은 캔버스 없이 시험할 수 있다.
 */

export type Bins = {
  /** 0–255 구간별 픽셀 수 */
  r: Uint32Array;
  g: Uint32Array;
  b: Uint32Array;
  /** 가장 높은 칸의 값. 세로 눈금을 맞출 때 쓴다. */
  peak: number;
  /** 완전히 검은 픽셀의 비율 (0–1) */
  clippedShadow: number;
  /** 완전히 흰 픽셀의 비율 (0–1) */
  clippedHighlight: number;
};

/** 날아갔다고 볼 경계 — 이 위(아래)면 정보가 남아 있지 않다 */
const HI = 253;
const LO = 2;

/**
 * RGBA 바이트 배열에서 분포를 센다.
 *
 * 양 끝 칸(0·255)은 봉우리 계산에서 뺀다. 하늘이 조금만 날아가도 255 칸이
 * 치솟아 나머지가 바닥에 깔려 아무것도 안 보인다.
 */
export function bins(rgba: Uint8ClampedArray): Bins {
  const r = new Uint32Array(256);
  const g = new Uint32Array(256);
  const b = new Uint32Array(256);
  let shadow = 0;
  let highlight = 0;
  const n = rgba.length >> 2;

  for (let i = 0; i < rgba.length; i += 4) {
    const R = rgba[i];
    const G = rgba[i + 1];
    const B = rgba[i + 2];
    r[R]++;
    g[G]++;
    b[B]++;
    if (R >= HI && G >= HI && B >= HI) highlight++;
    else if (R <= LO && G <= LO && B <= LO) shadow++;
  }

  let peak = 1;
  for (let v = 1; v < 255; v++) {
    if (r[v] > peak) peak = r[v];
    if (g[v] > peak) peak = g[v];
    if (b[v] > peak) peak = b[v];
  }

  return {
    r,
    g,
    b,
    peak,
    clippedShadow: n === 0 ? 0 : shadow / n,
    clippedHighlight: n === 0 ? 0 : highlight / n,
  };
}

/**
 * 한 채널을 폴리라인 좌표로. 위로 갈수록 값이 큰 화면 좌표계다.
 *
 * 봉우리를 넘는 칸은 천장에 붙인다 — 양 끝을 빼고 잰 봉우리라 넘칠 수 있다.
 */
export function polyline(
  ch: Uint32Array,
  peak: number,
  w: number,
  h: number,
): string {
  const pts: string[] = [];
  for (let v = 0; v < 256; v++) {
    const x = (v / 255) * w;
    const y = h - Math.min(1, ch[v] / peak) * h;
    pts.push(`${x.toFixed(1)},${y.toFixed(1)}`);
  }
  return pts.join(" ");
}

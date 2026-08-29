/**
 * 그리드 보기 방식 — Lap의 네 가지와 같다.
 *
 *   카드      정사각형 상자에 사진 전체, 아래 이름줄
 *   타일      폭은 칸, 높이는 썸네일 크기로 고정, 채워서 자른다
 *   양쪽 맞춤 줄 높이가 같고 폭이 비율대로 — 오른쪽 끝이 맞는다
 *   메이슨리  칸 폭이 같고 높이가 비율대로 — 짧은 열에 다음 장이 간다
 *
 * «담는 방식»(전체·채우기·늘리기)은 없앴다. 카드는 전체, 나머지는 채우기가
 * 맞고 고를 이유가 없었다.
 */

export type GridStyle = "card" | "tile" | "justified" | "masonry";

export const STYLES: { v: GridStyle; label: string }[] = [
  { v: "card", label: "카드" },
  { v: "tile", label: "타일" },
  { v: "justified", label: "양쪽 맞춤" },
  { v: "masonry", label: "메이슨리" },
];

/** 사진을 상자에 어떻게 담나 — 카드만 전체, 나머지는 채운다 */
export const fitOf = (s: GridStyle): "object-contain" | "object-cover" =>
  s === "card" ? "object-contain" : "object-cover";

/** 이름줄이 붙는 보기 — 카드뿐 (Lap과 같다) */
export const hasCaption = (s: GridStyle): boolean => s === "card";

/**
 * 사진 아래 이름·정보가 차지하는 높이 (px).
 *
 * 두 줄이다 — 이름 한 줄, 날짜·크기 한 줄. 가상 스크롤이 줄 높이를 미리
 * 알아야 해서 상수로 둔다. 글자 크기를 바꾸면 여기도 같이 바꿔야 한다.
 */
export const CAPTION_H = 30;

/** 칸 사이 틈 (px). 가로·세로 같다. */
export const GAP = 10;
/** 그리드 바깥 여백 (px). `main`의 p-2.5와 같아야 한다. */
export const PAD = 10;

export type Metrics = {
  /** 안쪽 폭 — 바깥 여백을 뺀 것 */
  contentW: number;
  cols: number;
  /** 칸 하나의 폭. 남는 폭을 나눠 가져 썸네일 크기보다 크다. */
  cellW: number;
  /** 그림 상자의 높이 — 칸 폭과 담는 모양에서 나온다 */
  imageH: number;
  /** 한 줄이 차지하는 높이. 가상 스크롤이 다음 줄을 놓는 간격이다. */
  rowH: number;
};

/**
 * 격자의 치수 — 전부 여기서 나온다.
 *
 * 예전에는 줄 높이를 «썸네일 크기 + 이름줄»로 잡았는데, 칸은 남는 폭을
 * 나눠 가져 썸네일보다 넓고 그림은 정사각형이라 실제 줄이 더 높았다.
 * 가상 스크롤은 잡아 둔 높이대로 다음 줄을 놓으니 줄이 겹쳐 이름이 묻히고
 * 그림이 붙었다. 칸 폭을 먼저 정하고 높이를 그 폭에서 끌어낸다.
 *
 * @param clientW  스크롤 요소의 clientWidth (여백 포함, 스크롤바 제외)
 * @param thumb    썸네일 슬라이더 값 — 칸의 **최소** 폭
 */
export function metrics(
  clientW: number,
  thumb: number,
  style: GridStyle,
  caption: boolean,
): Metrics {
  const contentW = Math.max(0, clientW - PAD * 2);
  // cols·thumb + (cols−1)·GAP ≤ contentW 를 만족하는 가장 큰 cols
  const cols = Math.max(1, Math.floor((contentW + GAP) / (thumb + GAP)));
  const cellW = Math.max(0, (contentW - (cols - 1) * GAP) / cols);
  // 타일은 높이가 썸네일 크기로 고정이다 (Lap: height = size). 나머지 격자는 정사각형.
  const imageH = style === "tile" ? thumb : cellW;
  const rowH = imageH + (caption ? CAPTION_H : 0) + GAP;
  return { contentW, cols, cellW, imageH, rowH };
}

/** 사진 한 장의 가로세로비. 모르면 정사각형으로 친다. */
export function ratio(w: number | null, h: number | null): number {
  if (!w || !h || w <= 0 || h <= 0) return 1;
  const r = w / h;
  // 파노라마 한 장이 줄 전체를 먹지 않게 막는다
  return Math.min(Math.max(r, 0.25), 4);
}

export type JustifiedRow<T> = {
  items: { file: T; width: number }[];
  height: number;
};

/**
 * 양쪽 맞춤 배치 — 한 줄의 사진들을 폭에 딱 맞게 늘린다.
 *
 * 가로세로비의 합으로 줄 높이가 정해진다: 높이 = (가용폭) / (비의 합).
 * 목표 높이를 넘어가면 줄을 끊는다. 마지막 줄은 늘리지 않는다 — 사진 두 장이
 * 화면을 가로질러 거대해지는 꼴을 막는다.
 */
export function justify<T>(
  files: T[],
  ratioOf: (f: T) => number,
  totalWidth: number,
  targetHeight: number,
  gap: number,
): JustifiedRow<T>[] {
  const out: JustifiedRow<T>[] = [];
  if (totalWidth <= 0 || targetHeight <= 0) return out;

  let line: T[] = [];
  let sum = 0;

  const flush = (stretch: boolean) => {
    if (line.length === 0) return;
    const avail = totalWidth - gap * (line.length - 1);
    const h = stretch ? avail / sum : targetHeight;
    out.push({
      items: line.map((f) => ({ file: f, width: ratioOf(f) * h })),
      height: h,
    });
    line = [];
    sum = 0;
  };

  for (const f of files) {
    line.push(f);
    sum += ratioOf(f);
    const avail = totalWidth - gap * (line.length - 1);
    // 이 줄을 폭에 맞추면 높이가 목표보다 낮아진다 → 여기서 끊는다
    if (avail / sum <= targetHeight) flush(true);
  }
  flush(false);
  return out;
}

// ── 메이슨리 ────────────────────────────────────────────────────────────

export type MasonryBox<T> = {
  file: T;
  /** rows 안 위치 — 뷰어를 열 때 쓴다 */
  index: number;
  x: number;
  y: number;
  w: number;
  h: number;
};
export type MasonryHeader = { label: string; count: number; y: number };

/**
 * y 오름차순 상자 목록에서 [top, bottom] 과 겹칠 수 있는 구간 [start, end).
 *
 * 메이슨리는 «가장 짧은 열»에 놓으므로 상자의 y 는 순번대로 줄지 않는다 — 이분 탐색이
 * 된다. `maxH` 는 가장 큰 상자 높이: 그보다 위에서 시작한 상자는 top 에 닿지 못한다.
 * 스크롤마다 2만 상자를 두 번 훑던 길(리뷰 H18)을 O(log n + 보이는 수)로.
 */
export function visibleRange(
  boxes: { y: number }[],
  top: number,
  bottom: number,
  maxH: number,
): [number, number] {
  const t = top - maxH;
  let lo = 0;
  let hi = boxes.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (boxes[mid].y < t) lo = mid + 1;
    else hi = mid;
  }
  const start = lo;
  hi = boxes.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (boxes[mid].y <= bottom) lo = mid + 1;
    else hi = mid;
  }
  return [start, lo];
}
export type MasonryLayout<T> = {
  boxes: MasonryBox<T>[];
  headers: MasonryHeader[];
  /** 전체 높이 */
  height: number;
};

/**
 * 메이슨리 — 열 폭은 같고 높이는 사진 비율대로. 다음 장은 가장 짧은 열로.
 *
 * 묶기가 켜져 있으면(그룹 값이 null이 아니면) 묶음마다 머리글을 놓고 열을
 * 새로 시작한다 — 안 그러면 한 묶음의 마지막 장과 다음 묶음의 첫 장이
 * 옆 열에서 위아래로 섞인다. (Lap이 그룹마다 따로 layout을 도는 이유)
 */
export function masonry<T>(
  files: T[],
  groupOf: (f: T) => string | null,
  ratioOf: (f: T) => number,
  contentW: number,
  cols: number,
  gap: number,
  headerH: number,
): MasonryLayout<T> {
  const boxes: MasonryBox<T>[] = [];
  const headers: MasonryHeader[] = [];
  if (files.length === 0 || contentW <= 0 || cols <= 0)
    return { boxes, headers, height: 0 };
  const w = (contentW - (cols - 1) * gap) / cols;
  const grouped = groupOf(files[0]) !== null;

  let y0 = 0; // 지금 묶음이 시작하는 높이
  let heights = new Array<number>(cols).fill(0);
  let i = 0;
  while (i < files.length) {
    // 묶음의 끝
    const key = groupOf(files[i]);
    let end = i + 1;
    if (grouped)
      while (end < files.length && groupOf(files[end]) === key) end++;
    else end = files.length;

    if (grouped) {
      headers.push({ label: key ?? "", count: end - i, y: y0 });
      y0 += headerH;
      heights = new Array<number>(cols).fill(y0);
    }
    for (let k = i; k < end; k++) {
      let c = 0;
      for (let j = 1; j < cols; j++) if (heights[j] < heights[c]) c = j;
      const h = w / ratioOf(files[k]);
      boxes.push({
        file: files[k],
        index: k,
        x: c * (w + gap),
        y: heights[c],
        w,
        h,
      });
      heights[c] += h + gap;
    }
    y0 = Math.max(...heights);
    i = end;
  }
  const height = boxes.length ? Math.max(...heights) - gap : y0;
  return { boxes, headers, height: Math.max(0, height) };
}

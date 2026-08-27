/**
 * 그리드 보기 방식 — Lap의 style/scaling과 같은 갈래.
 *
 * 카드·타일은 칸이 격자로 고정되고, 양쪽 맞춤은 **줄마다 높이가 달라진다** —
 * 사진의 가로세로비를 지키면서 줄의 오른쪽 끝을 맞추기 때문이다.
 */

export type GridStyle = "card" | "tile" | "justified";
export type Scaling = "contain" | "cover" | "fill";

export const STYLES: { v: GridStyle; label: string }[] = [
  { v: "card", label: "카드 보기" },
  { v: "tile", label: "타일 보기" },
  { v: "justified", label: "양쪽 맞춤" },
];

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
  const imageH = style === "tile" ? (cellW * 3) / 4 : cellW;
  const rowH = imageH + (caption ? CAPTION_H : 0) + GAP;
  return { contentW, cols, cellW, imageH, rowH };
}

export const SCALINGS: { v: Scaling; label: string }[] = [
  { v: "cover", label: "채우기" },
  { v: "contain", label: "사진 전체" },
  { v: "fill", label: "늘리기" },
];

export const objectFit = (s: Scaling) =>
  s === "cover"
    ? "object-cover"
    : s === "contain"
      ? "object-contain"
      : "object-fill";

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

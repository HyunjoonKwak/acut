/**
 * 타임라인 스크롤바의 계산만 따로 뺐다.
 *
 * 눈금 위치·손잡이 크기는 어긋나면 바로 눈에 띄지만 React 안에 있으면
 * 확인할 방법이 없다. 순수 함수로 두고 시험한다.
 */

export type Bucket = {
  year: number;
  month: number;
  count: number;
  top: number;
};
/** 눈금 하나 — `start`는 이 달의 첫 사진이 전체에서 몇 번째인가 */
export type Item = Bucket & { start: number };

export type Mark = {
  key: string;
  /** 열 안에서의 픽셀 위치 */
  y: number;
  isYear: boolean;
  /** 빈 문자열이면 글자 없이 선만 그린다 */
  label: string;
};

/** 눈금 라벨끼리 최소한 이만큼은 떨어져야 읽힌다 (px) */
export const MIN_LABEL = 13;
/** 라벨 없는 눈금선끼리의 최소 간격 (px) */
export const MIN_TICK = 5;
/** 손잡이가 이보다 얇으면 잡히지 않는다 (px) */
export const MIN_THUMB = 28;

/** 달마다 시작 순번을 매기고 전체 장수를 센다. */
export function cumulative(buckets: Bucket[]): {
  items: Item[];
  total: number;
} {
  let acc = 0;
  const items = buckets.map((b) => {
    const start = acc;
    acc += b.count;
    return { ...b, start };
  });
  return { items, total: acc };
}

/** 세로 위치를 전역 순번으로. 열 밖으로 나가면 양 끝으로 붙인다. */
export function yToIndex(y: number, h: number, total: number): number {
  if (h <= 0 || total <= 0) return 0;
  const r = Math.max(0, Math.min(1, y / h));
  return Math.min(total - 1, Math.floor(r * total));
}

/** 순번이 속한 달. 눈금이 정렬돼 있으니 이분 탐색. */
export function bucketAt(items: Item[], index: number): Item | null {
  if (items.length === 0) return null;
  let lo = 0;
  let hi = items.length - 1;
  let found = items[0];
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (items[mid].start <= index) {
      found = items[mid];
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

/**
 * 그릴 눈금을 고른다.
 *
 * 10년치면 120개월이라 그대로 그리면 글자가 겹친다. 연도를 먼저 자리잡게 하고,
 * 남는 틈에만 월을 넣고, 그래도 남은 달은 짧은 선으로만 표시한다.
 */
export function thinMarks(items: Item[], total: number, h: number): Mark[] {
  if (h <= 0 || total <= 0 || items.length === 0) return [];

  const rows = items.map((it) => ({
    it,
    y: (it.start / total) * h,
    isYear: false,
  }));
  let prevYear: number | null = null;
  for (const r of rows) {
    if (r.it.year !== prevYear) {
      r.isYear = true;
      prevYear = r.it.year;
    }
  }

  // 1차: 연도 라벨을 먼저 잡는다
  const yearY: number[] = [];
  const labeled = new Set<(typeof rows)[number]>();
  let lastYearY = -Infinity;
  for (const r of rows) {
    if (!r.isYear) continue;
    if (r.y - lastYearY >= MIN_LABEL) {
      labeled.add(r);
      yearY.push(r.y);
      lastYearY = r.y;
    }
  }

  // 2차: 남는 틈에 월 라벨. 바로 뒤에 올 연도 라벨과도 떨어져 있어야 한다.
  let lastLabelY = -Infinity;
  let nextYear = 0;
  for (const r of rows) {
    while (nextYear < yearY.length && yearY[nextYear] <= r.y) nextYear++;
    const ahead = nextYear < yearY.length ? yearY[nextYear] : Infinity;
    if (labeled.has(r)) {
      lastLabelY = r.y;
      continue;
    }
    if (r.y - lastLabelY >= MIN_LABEL && ahead - r.y >= MIN_LABEL) {
      labeled.add(r);
      lastLabelY = r.y;
    }
  }

  // 3차: 라벨이 없는 달은 짧은 선으로. 이것도 촘촘하면 솎아낸다.
  const out: Mark[] = [];
  let lastTickY = -Infinity;
  for (const r of rows) {
    const hasLabel = labeled.has(r);
    if (!hasLabel && r.y - lastTickY < MIN_TICK) continue;
    lastTickY = r.y;
    out.push({
      key: `${r.it.year}-${r.it.month}`,
      y: r.y,
      isYear: r.isYear,
      label: hasLabel
        ? r.isYear
          ? String(r.it.year)
          : String(r.it.month).padStart(2, "0")
        : "",
    });
  }
  return out;
}

/**
 * 손잡이의 크기와 위치.
 *
 * 크기는 "한 화면에 몇 장 보이나"의 비율이다. 6만 장에서 60장이 보이면 손잡이가
 * 머리카락이 되므로 최소 높이를 둔다.
 */
export function thumbGeometry(
  total: number,
  pageSize: number,
  at: number,
  h: number,
) {
  if (h <= 0 || total <= 0)
    return { top: 0, height: Math.max(0, h), maxTop: 0 };
  const height = Math.min(h, Math.max(MIN_THUMB, (pageSize / total) * h));
  const maxTop = Math.max(0, h - height);
  const maxOffset = Math.max(1, total - pageSize);
  const top = Math.min(
    maxTop,
    (Math.max(0, Math.min(at, maxOffset)) / maxOffset) * maxTop,
  );
  return { top, height, maxTop };
}

/** 손잡이를 이 픽셀 위치로 옮기면 전체에서 몇 번째로 가는가. */
export function topToIndex(
  top: number,
  total: number,
  pageSize: number,
  h: number,
): number {
  const { maxTop } = thumbGeometry(total, pageSize, 0, h);
  if (maxTop <= 0) return 0;
  const r = Math.max(0, Math.min(1, top / maxTop));
  return Math.round(r * Math.max(1, total - pageSize));
}

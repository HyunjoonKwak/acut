import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { layout, HEADER_H } from "./gridLayout";
import {
  CAPTION_H,
  GAP,
  hasCaption,
  justify,
  masonry as masonryOf,
  metrics,
  ratio,
  type GridStyle,
  type JustifiedRow,
} from "./gridStyle";
import type { FileRow } from "./types";

/** 메이슨리에서 화면 밖으로 이만큼은 미리 그려 둔다 (px) */
const OVERSCAN = 600;

/**
 * 그리드의 치수와 가상 스크롤.
 *
 * 스크롤 요소의 폭·높이를 재고(ResizeObserver), 거기서 칸 수·줄 높이를
 * 내고(gridStyle.metrics), 줄을 가상화한다. 양쪽 맞춤은 줄마다 높이가
 * 달라 따로 잰다. 메이슨리는 줄이 없어 상자 자리를 다 셈해 두고(수만
 * 장이라도 밀리초) 보이는 범위만 그린다.
 */
export function useGridLayout(
  rows: FileRow[],
  opts: {
    thumbSize: number;
    gridStyle: GridStyle;
    caption: boolean;
    /** 끝에 가까워지면 부른다 */
    loadMore: () => void;
    /** 이 사진이 보이도록 스크롤을 따라 옮긴다 */
    selected: number | null;
  },
) {
  const [viewW, setViewW] = useState(0);
  const [viewH, setViewH] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);

  /// 스크롤 요소. **콜백 ref**로 잡는다 — useEffect + useRef로 하면 요소가
  /// 없을 때(설정 화면·필름스트립이 그리드를 떼어 둔 동안) 효과가 한 번 돌고
  /// 끝나, 돌아와도 폭이 0으로 남아 칸이 0px가 된다. 실제로 그렇게 사진이
  /// 안 보였다. 요소가 붙을 때마다 다시 잰다.
  const elRef = useRef<HTMLDivElement | null>(null);
  const obs = useRef<ResizeObserver | null>(null);
  const scrollRef = useCallback((el: HTMLDivElement | null) => {
    obs.current?.disconnect();
    obs.current = null;
    elRef.current = el;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setViewH(el.clientHeight);
      setViewW(el.clientWidth);
    });
    ro.observe(el);
    obs.current = ro;
    setViewH(el.clientHeight);
    setViewW(el.clientWidth);
    setScrollTop(el.scrollTop);
  }, []);

  const { thumbSize, gridStyle } = opts;
  // 이름줄은 카드에서만 붙는다 — 다른 보기에서는 줄 높이에 넣지 않는다
  const caption = opts.caption && hasCaption(gridStyle);
  const { contentW, cols, imageH, rowH } = useMemo(
    () => metrics(viewW, thumbSize, gridStyle, caption),
    [viewW, thumbSize, gridStyle, caption],
  );

  /// 머리글과 사진 줄을 한 목록으로 편다. 묶기를 끄면 사진 줄만 나온다.
  const grid = useMemo(() => layout(rows, (r) => r.group, cols), [rows, cols]);

  /// 양쪽 맞춤일 때 각 사진 줄의 칸 크기. 줄마다 높이가 다르다.
  const justified = useMemo(() => {
    if (gridStyle !== "justified" || contentW <= 0) return null;
    const out = new Map<number, JustifiedRow<FileRow>[]>();
    grid.forEach((row, i) => {
      if (row.kind !== "photos") return;
      out.set(
        i,
        justify(
          row.items,
          (f) => ratio(f.width, f.height),
          contentW,
          thumbSize,
          GAP,
        ),
      );
    });
    return out;
  }, [grid, gridStyle, contentW, thumbSize]);

  /// 메이슨리 — 상자 자리 전부. 스크롤과 무관하다 — 스크롤마다 다시 셈하면 2만 상자를
  /// 프레임마다 새로 만든다 (리뷰 H18)
  const masonryAll = useMemo(() => {
    if (gridStyle !== "masonry" || contentW <= 0) return null;
    return masonryOf(
      rows,
      (r) => r.group,
      (r) => ratio(r.width, r.height),
      contentW,
      cols,
      GAP,
      HEADER_H,
    );
  }, [gridStyle, rows, contentW, cols]);
  /// 그중 지금 그릴 것 — 스크롤에 따라
  const masonry = useMemo(() => {
    const L = masonryAll;
    if (L === null) return null;
    const top = scrollTop - OVERSCAN;
    const bottom = scrollTop + viewH + OVERSCAN;
    const visible = L.boxes.filter((b) => b.y + b.h >= top && b.y <= bottom);
    // 스크롤바가 쓸 «맨 위 사진 순번»과 «한 화면 장수»
    const onScreen = L.boxes.filter(
      (b) => b.y + b.h >= scrollTop && b.y <= scrollTop + viewH,
    );
    return {
      ...L,
      visible,
      firstIndex: onScreen.length
        ? Math.min(...onScreen.map((b) => b.index))
        : 0,
      onScreen: Math.max(1, onScreen.length),
    };
  }, [masonryAll, scrollTop, viewH]);

  const virt = useVirtualizer({
    count: masonry ? 0 : grid.length,
    getScrollElement: () => elRef.current,
    // 머리글과 사진 줄은 높이가 다르다
    estimateSize: (i) => {
      if (grid[i]?.kind === "header") return HEADER_H;
      const j = justified?.get(i);
      // 양쪽 맞춤은 한 「줄」이 여러 소줄로 나뉜다
      if (j)
        return j.reduce(
          (a, r) => a + r.height + GAP + (caption ? CAPTION_H : 0),
          0,
        );
      return rowH;
    },
    overscan: 4,
  });

  // 줄 높이의 입력이 바뀌면 가상 스크롤의 높이 캐시를 비운다. tanstack은
  // estimateSize가 바뀌어도 앞서 계산한 자리를 그대로 쓴다 — 실측: 양쪽 맞춤에서
  // 썸네일 크기를 바꾸면 옛 높이로 자리를 잡아 사진이 겹쳐 보였다.
  useEffect(() => {
    virt.measure();
  }, [virt, justified, rowH]);

  // 끝에 가까워지면 다음 쪽
  const items = virt.getVirtualItems();
  const lastIndex = items[items.length - 1]?.index ?? -1;
  const { loadMore } = opts;
  const masonryLast = masonry?.visible.length
    ? Math.max(...masonry.visible.map((b) => b.index))
    : -1;
  useEffect(() => {
    if (masonry) {
      if (masonryLast >= rows.length - 12) loadMore();
      return;
    }
    if (lastIndex >= 0 && lastIndex >= grid.length - 3) loadMore();
  }, [lastIndex, grid.length, loadMore, masonry, masonryLast, rows.length]);

  /// 초점이 화면 밖으로 나가면 그 줄까지 옮긴다. 이미 보이는 것은 건드리지
  /// 않는다 — 누를 때마다 가운데로 끌어오면 눈이 어지럽다.
  const { selected } = opts;
  useEffect(() => {
    if (selected === null) return;
    if (masonry) {
      const b = masonry.boxes.find((x) => x.file.id === selected);
      const el = elRef.current;
      if (!b || !el) return;
      if (b.y < el.scrollTop) el.scrollTo({ top: b.y - GAP });
      else if (b.y + b.h > el.scrollTop + el.clientHeight)
        el.scrollTo({ top: b.y + b.h - el.clientHeight + GAP });
      return;
    }
    const at = grid.findIndex(
      (r) => r.kind === "photos" && r.items.some((i) => i.id === selected),
    );
    if (at >= 0) virt.scrollToIndex(at, { align: "auto" });
    // grid·masonry는 목록이 바뀔 때마다 새로 만들어진다. 초점이 그대로면 굳이
    // 다시 맞출 이유가 없어 의존성에서 뺀다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  const onScroll = useCallback(
    (e: React.UIEvent<HTMLDivElement>) =>
      setScrollTop(e.currentTarget.scrollTop),
    [],
  );
  const resetScroll = useCallback(() => {
    elRef.current?.scrollTo({ top: 0 });
    setScrollTop(0);
  }, []);

  /// 스크롤바가 알아야 하는 둘 — 지금 맨 위 사진의 상대 순번과 한 화면 장수
  const topOffset = masonry
    ? masonry.firstIndex
    : Math.floor(scrollTop / rowH) * cols;
  const pageSize = masonry
    ? masonry.onScreen
    : Math.max(cols, Math.ceil(viewH / rowH) * cols);

  return {
    scrollRef,
    onScroll,
    resetScroll,
    cols,
    rowH,
    imageH,
    grid,
    justified,
    masonry,
    virt,
    topOffset,
    pageSize,
  };
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { layout, HEADER_H } from "./gridLayout";
import {
  CAPTION_H,
  GAP,
  justify,
  metrics,
  ratio,
  type GridStyle,
  type JustifiedRow,
} from "./gridStyle";
import type { FileRow } from "./types";

/**
 * 그리드의 치수와 가상 스크롤.
 *
 * 스크롤 요소의 폭·높이를 재고(ResizeObserver), 거기서 칸 수·줄 높이를
 * 내고(gridStyle.metrics), 줄을 가상화한다. 양쪽 맞춤은 줄마다 높이가
 * 달라 따로 잰다.
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
  const scrollRef = useRef<HTMLDivElement>(null);
  const [viewW, setViewW] = useState(0);
  const [viewH, setViewH] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setViewH(el.clientHeight);
      setViewW(el.clientWidth);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const { thumbSize, gridStyle, caption } = opts;
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

  const virt = useVirtualizer({
    count: grid.length,
    getScrollElement: () => scrollRef.current,
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

  // 끝에 가까워지면 다음 쪽
  const items = virt.getVirtualItems();
  const lastIndex = items[items.length - 1]?.index ?? -1;
  const { loadMore } = opts;
  useEffect(() => {
    if (lastIndex >= 0 && lastIndex >= grid.length - 3) loadMore();
  }, [lastIndex, grid.length, loadMore]);

  /// 초점이 화면 밖으로 나가면 그 줄까지 옮긴다. `align: "auto"`라 이미
  /// 보이는 것은 건드리지 않는다 — 누를 때마다 가운데로 끌어오면 눈이 어지럽다.
  const { selected } = opts;
  useEffect(() => {
    if (selected === null) return;
    const at = grid.findIndex(
      (r) => r.kind === "photos" && r.items.some((i) => i.id === selected),
    );
    if (at >= 0) virt.scrollToIndex(at, { align: "auto" });
    // grid는 목록이 바뀔 때마다 새로 만들어진다. 초점이 그대로면 굳이
    // 다시 맞출 이유가 없어 의존성에서 뺀다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  const onScroll = useCallback(
    (e: React.UIEvent<HTMLDivElement>) =>
      setScrollTop(e.currentTarget.scrollTop),
    [],
  );
  const resetScroll = useCallback(() => {
    scrollRef.current?.scrollTo({ top: 0 });
    setScrollTop(0);
  }, []);

  /// 스크롤바가 알아야 하는 둘 — 지금 맨 위 사진의 상대 순번과 한 화면 장수
  const topOffset = Math.floor(scrollTop / rowH) * cols;
  const pageSize = Math.max(cols, Math.ceil(viewH / rowH) * cols);

  return {
    scrollRef,
    onScroll,
    resetScroll,
    cols,
    rowH,
    imageH,
    grid,
    justified,
    virt,
    topOffset,
    pageSize,
  };
}

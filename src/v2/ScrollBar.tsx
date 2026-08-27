import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  bucketAt,
  cumulative,
  thinMarks,
  thumbGeometry,
  topToIndex,
  yToIndex,
  type Bucket,
} from "./scrollbarMath";

export type { Bucket };

/**
 * 타임라인 스크롤바 — 왼쪽은 연·월 눈금, 오른쪽은 진짜 스크롤바다.
 *
 * 눈금 간격을 **장수에 비례**시킨다. 5,000장인 달과 20장인 달을 같은 높이로
 * 두면 손잡이와 눈금이 어긋나 지금 어디인지 알 수 없다.
 *
 * 위치는 전역 순번으로 주고받는다. 목록은 keyset으로 읽지만 스크롤바가 아는 건
 * "전체의 몇 번째"뿐이라, 순번을 커서로 바꾸는 일은 백엔드가 한다.
 */
export default function ScrollBar({
  buckets,
  offset,
  pageSize,
  onSeek,
}: {
  buckets: Bucket[];
  /** 지금 화면 맨 위에 있는 사진의 전역 순번 */
  offset: number;
  /** 한 화면에 보이는 장수 */
  pageSize: number;
  onSeek: (index: number) => void;
}) {
  const colRef = useRef<HTMLDivElement>(null);
  const [h, setH] = useState(0);
  const [hoverY, setHoverY] = useState<number | null>(null);
  /// 끄는 동안에는 응답을 기다리지 않고 바로 따라 움직인다
  const [dragAt, setDragAt] = useState<number | null>(null);
  const dragging = useRef<"thumb" | "marks" | null>(null);

  useEffect(() => {
    const el = colRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setH(el.clientHeight));
    ro.observe(el);
    setH(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  const { items, total } = useMemo(() => cumulative(buckets), [buckets]);
  const marks = useMemo(() => thinMarks(items, total, h), [items, total, h]);

  const at = dragAt ?? offset;
  const thumb = thumbGeometry(total, pageSize, at, h);

  const endDrag = useCallback(() => {
    if (!dragging.current) return;
    dragging.current = null;
    setDragAt(null);
  }, []);

  // 창 밖에서 손을 떼도 드래그가 끝나야 한다
  useEffect(() => {
    window.addEventListener("pointerup", endDrag);
    return () => window.removeEventListener("pointerup", endDrag);
  }, [endDrag]);

  // ── 손잡이: 순번 단위로 연속 이동 ────────────────────────────────
  const grab = useRef(0);

  const seekFromTop = useCallback(
    (top: number) => {
      const index = topToIndex(top, total, pageSize, h);
      setDragAt(index);
      onSeek(index);
    },
    [total, pageSize, h, onSeek],
  );

  const onThumbDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    dragging.current = "thumb";
    grab.current = e.clientY - e.currentTarget.getBoundingClientRect().top;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragAt(at);
  };

  const onThumbMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (dragging.current !== "thumb") return;
    const track = e.currentTarget.parentElement!.getBoundingClientRect();
    seekFromTop(e.clientY - track.top - grab.current);
  };

  /// 트랙 빈 곳을 누르면 손잡이가 그 자리로 온다
  const onTrackDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    seekFromTop(e.clientY - rect.top - thumb.height / 2);
    setDragAt(null);
  };

  // ── 눈금 열: 달 단위 ────────────────────────────────────────────
  // 손잡이가 연속이라면 이쪽은 "2018년 5월"처럼 딱 떨어지는 자리로 간다.
  const lastMonth = useRef(-1);

  const scrubMonths = useCallback(
    (clientY: number, el: HTMLElement) => {
      const y = clientY - el.getBoundingClientRect().top;
      const b = bucketAt(items, yToIndex(y, h, total));
      if (!b || b.start === lastMonth.current) return;
      lastMonth.current = b.start;
      setDragAt(b.start);
      onSeek(b.start);
    },
    [items, h, total, onSeek],
  );

  if (total === 0) return null;

  const hoverBucket =
    hoverY === null ? null : bucketAt(items, yToIndex(hoverY, h, total));
  const curY = (Math.min(at, total - 1) / total) * h;

  return (
    <div className="w-[58px] shrink-0 flex relative bg-[#181D1F] border-l border-[#242C2E] select-none">
      {/* 연·월 눈금 — 누르거나 끌면 그 달로 간다 */}
      <div
        ref={colRef}
        className="flex-1 relative cursor-row-resize touch-none"
        onPointerMove={(e) => {
          setHoverY(e.clientY - e.currentTarget.getBoundingClientRect().top);
          if (dragging.current === "marks")
            scrubMonths(e.clientY, e.currentTarget);
        }}
        onPointerLeave={() => setHoverY(null)}
        onPointerDown={(e) => {
          dragging.current = "marks";
          lastMonth.current = -1;
          e.currentTarget.setPointerCapture(e.pointerId);
          scrubMonths(e.clientY, e.currentTarget);
        }}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        {marks.map((m) => (
          <div
            key={m.key}
            className="absolute right-0 flex items-center justify-end pr-1.5 pointer-events-none"
            style={{ top: m.y, transform: "translateY(-50%)" }}
          >
            {m.label ? (
              <span
                className="font-mono tabular-nums"
                style={{
                  fontSize: m.isYear ? 10 : 9,
                  fontWeight: m.isYear ? 700 : 400,
                  color: m.isYear ? "#8D9A9C" : "#5A6668",
                }}
              >
                {m.label}
              </span>
            ) : (
              <span
                className="block rounded-full bg-[#3A4547]"
                style={{ height: 1, width: m.isYear ? 8 : 4 }}
              />
            )}
          </div>
        ))}

        {/* 지금 보고 있는 자리 */}
        <div
          className="absolute left-0 right-0 pointer-events-none"
          style={{
            top: curY - 1,
            height: 2,
            background: "#49B8B4",
            opacity: 0.9,
          }}
        />

        {/* 마우스가 가리키는 자리 */}
        {hoverY !== null && (
          <div
            className="absolute left-1 right-0 pointer-events-none"
            style={{ top: hoverY - 1, height: 2, background: "#8D9A9C" }}
          />
        )}
      </div>

      {/* 트랙 */}
      <div
        className="w-2.5 relative mr-1 rounded-full bg-[#232A2C] touch-none"
        onPointerDown={onTrackDown}
      >
        <div
          className="absolute inset-x-0 rounded-full hover:bg-[#4E5C5F]"
          style={{
            top: thumb.top,
            height: thumb.height,
            background: dragging.current === "thumb" ? "#49B8B4" : "#3E4A4C",
            cursor: "grab",
          }}
          onPointerDown={onThumbDown}
          onPointerMove={onThumbMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        />
      </div>

      {/* 말풍선 */}
      {hoverY !== null && hoverBucket && (
        <div
          className="absolute z-40 right-[62px] px-2 py-1 rounded-md bg-[#2C3436] text-[#EAEFEF] text-[11.5px] whitespace-nowrap shadow-lg pointer-events-none"
          style={{ top: hoverY, transform: "translateY(-50%)" }}
        >
          {hoverBucket.year}년 {hoverBucket.month}월{" "}
          <span className="text-[#7C8A8D] tabular-nums">
            {hoverBucket.count.toLocaleString()}장
          </span>
        </div>
      )}
    </div>
  );
}

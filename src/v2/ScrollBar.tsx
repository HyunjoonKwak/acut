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
  const [h, setH] = useState(0);
  const [hoverY, setHoverY] = useState<number | null>(null);
  /// 끄는 동안에는 응답을 기다리지 않고 바로 따라 움직인다
  const [dragAt, setDragAt] = useState<number | null>(null);
  const dragging = useRef<"thumb" | "marks" | null>(null);

  /// 눈금 열의 높이. **콜백 ref**로 잰다 — useEffect + useRef로 하면
  /// 첫 렌더에 눈금이 없어 열이 아직 안 그려진 상태에서 효과가 한 번 돌고
  /// 끝나 버린다. 나중에 눈금이 와도 높이는 0으로 남아 눈금도 손잡이도
  /// 안 보이고 끌리지도 않는다. (실제로 그렇게 죽어 있었다)
  const obs = useRef<ResizeObserver | null>(null);
  const colRef = useCallback((el: HTMLDivElement | null) => {
    obs.current?.disconnect();
    if (!el) {
      obs.current = null;
      return;
    }
    const ro = new ResizeObserver(() => setH(el.clientHeight));
    ro.observe(el);
    obs.current = ro;
    setH(el.clientHeight);
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

  // 눈금이 없어도 자리는 지킨다. 사라졌다 나타나면 사진이 통째로 밀린다.
  const empty = total === 0;
  const hoverBucket =
    hoverY === null || empty
      ? null
      : bucketAt(items, yToIndex(hoverY, h, total));
  const curY = empty ? 0 : (Math.min(at, total - 1) / total) * h;
  const currentBucket = empty ? null : bucketAt(items, Math.min(at, total - 1));

  const onTimelineKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (
      !["ArrowDown", "ArrowUp", "PageDown", "PageUp", "Home", "End"].includes(
        e.key,
      )
    )
      return;
    // 사진이 0장이어도 프로그램 초점이 남아 있을 수 있다. 이 키가 뒤의 격자
    // 이동으로 새면 타임라인과 선택이 동시에 움직인다.
    e.preventDefault();
    e.stopPropagation();
    if (empty) return;
    const current = Math.min(at, total - 1);
    const month = items.findIndex(
      (b) => current >= b.start && current < b.start + b.count,
    );
    let next: number | null = null;
    if (e.key === "ArrowDown")
      next =
        items[Math.min(items.length - 1, Math.max(0, month + 1))]?.start ??
        current;
    else if (e.key === "ArrowUp")
      next = items[Math.max(0, month < 0 ? 0 : month - 1)]?.start ?? current;
    else if (e.key === "PageDown")
      next = Math.min(total - 1, current + Math.max(1, pageSize));
    else if (e.key === "PageUp")
      next = Math.max(0, current - Math.max(1, pageSize));
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = Math.max(0, total - Math.max(1, pageSize));
    if (next === null) return;
    if (next !== current) onSeek(next);
  };

  return (
    <div className="w-[58px] shrink-0 flex relative bg-rail border-l border-line select-none">
      {/* 연·월 눈금 — 누르거나 끌면 그 달로 간다 */}
      <div
        ref={colRef}
        role="slider"
        tabIndex={empty ? -1 : 0}
        aria-disabled={empty ? "true" : undefined}
        aria-label="사진 촬영일 타임라인"
        aria-orientation="vertical"
        aria-valuemin={0}
        aria-valuemax={Math.max(0, total - 1)}
        aria-valuenow={empty ? 0 : Math.min(at, total - 1)}
        aria-valuetext={
          currentBucket
            ? `${currentBucket.year}년 ${currentBucket.month}월, 전체 ${total.toLocaleString()}장 중 ${Math.min(at, total - 1) + 1}번째`
            : "사진 없음"
        }
        className="flex-1 relative cursor-row-resize touch-none focus:outline-none focus:ring-1 focus:ring-inset focus:ring-focus"
        onKeyDown={onTimelineKey}
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
                  fontSize: m.isYear ? 11 : 10,
                  fontWeight: m.isYear ? 700 : 400,
                  color: m.isYear
                    ? "var(--color-fg-dim)"
                    : "var(--color-fg-mute)",
                }}
              >
                {m.label}
              </span>
            ) : (
              <span
                className="block rounded-full bg-fg-faint"
                style={{ height: 1, width: m.isYear ? 8 : 4 }}
              />
            )}
          </div>
        ))}

        {/* 지금 보고 있는 자리 */}
        {!empty && (
          <div
            className="absolute left-0 right-0 pointer-events-none"
            style={{
              top: curY - 1,
              height: 2,
              background: "var(--color-accent)",
              opacity: 0.9,
            }}
          />
        )}

        {/* 마우스가 가리키는 자리 */}
        {hoverY !== null && (
          <div
            className="absolute left-1 right-0 pointer-events-none"
            style={{
              top: hoverY - 1,
              height: 2,
              background: "var(--color-fg-dim)",
            }}
          />
        )}
      </div>

      {/* 트랙 */}
      <div
        className="w-2.5 relative mr-1 rounded-full bg-raised touch-none"
        onPointerDown={empty ? undefined : onTrackDown}
      >
        {!empty && (
          <div
            className="absolute inset-x-0 rounded-full hover:bg-fg-mute"
            style={{
              top: thumb.top,
              height: thumb.height,
              // 끄는 중인지는 state로 본다. ref를 그리기에 쓰면 값이 바뀌어도
              // 다시 그릴 이유가 없어 색이 그대로 남는다.
              background:
                dragAt !== null
                  ? "var(--color-accent)"
                  : "var(--color-fg-faint)",
              cursor: "grab",
            }}
            onPointerDown={onThumbDown}
            onPointerMove={onThumbMove}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
          />
        )}
      </div>

      {/* 말풍선 */}
      {hoverY !== null && hoverBucket && (
        <div
          className="absolute z-40 right-[62px] px-2 py-1 rounded-md bg-raised text-fg text-[12.5px] whitespace-nowrap shadow-lg pointer-events-none"
          style={{ top: hoverY, transform: "translateY(-50%)" }}
        >
          {hoverBucket.year}년 {hoverBucket.month}월{" "}
          <span className="text-fg-mute tabular-nums">
            {hoverBucket.count.toLocaleString()}장
          </span>
        </div>
      )}
    </div>
  );
}

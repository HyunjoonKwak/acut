import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { fmtDuration } from "./format";

/** 스트립이 쓰는 것만. 그리드의 행을 그대로 받아도 되게 최소로 잡는다. */
export type StripFile = {
  id: number;
  name: string;
  kind: number;
  culling_flag: number;
  duration_ms: number | null;
  /** 캐시 루트 기준 상대경로. 주소는 부르는 쪽이 만든다. */
  thumb: string | null;
  library_id: number | null;
};

/** 한 칸의 크기 (px). 세로는 여백까지 합쳐 STRIP_H가 된다. */
const W = 128;
const H = 96;
const GAP = 6;
export const STRIP_H = H + 22;

/**
 * 위쪽 필름스트립 — 지금 보는 곳 둘레의 사진들.
 *
 * 그리드를 크게 키워 놓으면 한 화면에 몇 장 안 들어와 앞뒤 맥락이 사라진다.
 * 스트립은 그 맥락을 좁고 길게 되돌려 준다. 초점이 옮겨 가면 따라 움직인다.
 *
 * 목록은 그리드와 **같은 배열**을 본다. 따로 읽지 않으니 정렬·필터가 저절로
 * 맞고, 끝에 닿으면 그리드와 같은 다음 쪽을 불러온다.
 */
export default function Filmstrip<T extends StripFile>({
  files,
  thumbUrl,
  selectedId,
  onPick,
  onOpen,
  onNearEnd,
  position = "top",
}: {
  /** 위에 있으면 아래쪽에, 아래에 있으면 위쪽에 선을 긋는다 */
  position?: "top" | "bottom";
  files: T[];
  thumbUrl: (f: T) => string | null;
  selectedId: number | null;
  onPick: (id: number, e: React.MouseEvent) => void;
  onOpen: (index: number) => void;
  onNearEnd: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);

  const virt = useVirtualizer({
    horizontal: true,
    count: files.length,
    getScrollElement: () => box.current,
    estimateSize: () => W + GAP,
    overscan: 6,
  });

  const at =
    selectedId === null ? -1 : files.findIndex((f) => f.id === selectedId);

  // 초점이 옮겨 가면 그 칸이 보이도록 따라간다. 화면 안에 이미 있으면
  // 가만히 둔다 — 매번 가운데로 끌어오면 눈이 어지럽다.
  useEffect(() => {
    if (at >= 0) virt.scrollToIndex(at, { align: "auto" });
  }, [at, virt]);

  // 끝에 가까워지면 다음 쪽
  const items = virt.getVirtualItems();
  const last = items[items.length - 1];
  useEffect(() => {
    if (last && last.index >= files.length - 5) onNearEnd();
  }, [last, files.length, onNearEnd]);

  if (files.length === 0) return null;

  return (
    <div
      ref={box}
      className={`shrink-0 overflow-x-auto overflow-y-hidden bg-chrome px-2 py-2 border-line ${position === "top" ? "border-b" : "border-t"}`}
      style={{ height: STRIP_H }}
    >
      <div
        className="relative"
        style={{ width: virt.getTotalSize(), height: H }}
      >
        {items.map((v) => {
          const f = files[v.index];
          if (!f) return null;
          const on = f.id === selectedId;
          const url = thumbUrl(f);
          return (
            <button
              key={f.id}
              onClick={(e) => onPick(f.id, e)}
              onDoubleClick={() => onOpen(v.index)}
              title={f.name}
              className="absolute top-0 rounded overflow-hidden bg-canvas"
              style={{
                left: v.start,
                width: W,
                height: H,
                boxShadow: on ? "0 0 0 2px var(--color-accent)" : undefined,
                opacity: on ? 1 : 0.75,
              }}
            >
              {url ? (
                <img
                  src={url}
                  loading="lazy"
                  decoding="async"
                  className="w-full h-full object-cover"
                />
              ) : (
                <span className="w-full h-full flex items-center justify-center text-fg-faint text-[11px]">
                  {f.kind === 1 ? "영상" : "…"}
                </span>
              )}

              {/* 판정만 표시한다. 좁은 칸에 별점까지 넣으면 아무것도 안 읽힌다 */}
              {f.culling_flag !== 0 && (
                <span
                  className="absolute top-0.5 right-0.5 w-3.5 h-3.5 rounded text-[10px] font-bold flex items-center justify-center"
                  style={
                    f.culling_flag === 1
                      ? {
                          background: "var(--color-keep)",
                          color: "var(--color-keep-fg)",
                        }
                      : {
                          background: "var(--color-drop)",
                          color: "var(--color-drop-fg)",
                        }
                  }
                >
                  {f.culling_flag === 1 ? "★" : "✕"}
                </span>
              )}
              {f.kind === 1 && (
                <span className="absolute bottom-0.5 left-0.5 px-1 rounded bg-black/55 text-fg text-[10px] tabular-nums">
                  ▶{f.duration_ms ? fmtDuration(f.duration_ms) : ""}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

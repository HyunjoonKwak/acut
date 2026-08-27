import { useCallback, useEffect, useRef, useState } from "react";
import { objectFit, type GridStyle, type Scaling } from "./gridStyle";
import {
  HOVER_DELAY_MS,
  claimHoverPreview,
  releaseHoverPreview,
} from "./hoverPreview";
import { fmtBytes, fmtDate, fmtDuration } from "./format";

export type TileFile = {
  id: number;
  name: string;
  size: number;
  kind: number;
  rating: number;
  culling_flag: number;
  favorite: boolean;
  taken_at: number;
  duration_ms: number | null;
};

/**
 * 그리드 타일 한 칸.
 *
 * 영상은 마우스를 올리면 그 자리에서 재생된다. 정지 프레임만으로는 무엇이
 * 찍혔는지 알 수 없어 고르기가 안 된다. 0.4초 기다렸다 트는 이유는 그리드를
 * 훑고 지나갈 때마다 400MB짜리를 열지 않기 위해서다.
 */
export default function Tile({
  file,
  url,
  picked,
  focused,
  onClick,
  onDoubleClick,
  onContextMenu,
  caption = true,
  style = "card",
  scaling = "cover",
  aspect,
}: {
  file: TileFile;
  /** 썸네일 주소. 아직 없으면 null */
  url: string | null;
  picked: boolean;
  focused: boolean;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  /** 사진 아래에 이름과 날짜·크기를 적을지 */
  caption?: boolean;
  style?: GridStyle;
  scaling?: Scaling;
  /** 양쪽 맞춤에서 계산된 칸 크기. 없으면 정사각형 격자. */
  aspect?: { width: number; height: number };
}) {
  const [playing, setPlaying] = useState(false);
  const [ready, setReady] = useState(false);
  const timer = useRef<number | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const isVideo = file.kind === 1;

  const stop = useCallback(() => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    const v = video.current;
    if (v) {
      v.pause();
      // src를 떼고 load()까지 해야 버퍼가 풀린다. 안 그러면 메모리가 쌓인다.
      v.removeAttribute("src");
      v.load();
    }
    setReady(false);
    setPlaying(false);
    releaseHoverPreview(stop);
  }, []);

  useEffect(() => stop, [stop]);

  const enter = () => {
    if (!isVideo || playing || timer.current !== null) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      claimHoverPreview(stop);
      setPlaying(true);
    }, HOVER_DELAY_MS);
  };

  return (
    <button
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onContextMenu={onContextMenu}
      onMouseEnter={enter}
      onMouseLeave={stop}
      className="text-left"
      style={aspect ? { width: aspect.width } : undefined}
    >
      <div
        className="rounded overflow-hidden bg-canvas relative"
        style={{
          ...(aspect
            ? { height: aspect.height }
            : { aspectRatio: style === "card" ? "1/1" : "4/3" }),
          boxShadow: picked
            ? "0 0 0 2px var(--color-accent)"
            : focused
              ? "0 0 0 2px var(--color-focus)"
              : undefined,
        }}
      >
        {url ? (
          <img
            src={url}
            loading="lazy"
            decoding="async"
            className={`w-full h-full ${objectFit(scaling)}`}
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-fg-faint text-[10px]">
            {isVideo ? "영상" : "…"}
          </div>
        )}

        {playing && (
          <video
            ref={video}
            src={`video://localhost/${file.id}`}
            className={`absolute inset-0 w-full h-full ${objectFit(scaling)} pointer-events-none transition-opacity duration-150`}
            style={{ opacity: ready ? 1 : 0 }}
            muted
            autoPlay
            loop
            playsInline
            preload="metadata"
            onCanPlay={() => setReady(true)}
            onPlaying={() => setReady(true)}
            onError={stop}
          />
        )}

        {/* 위 — 상태 배지 (Lap: statusBadges). 판정·별점·즐겨찾기 */}
        <div className="absolute top-1 right-1 flex items-center gap-1">
          {file.favorite && (
            <span className="px-1 h-4 rounded bg-black/55 text-drop text-[10px] flex items-center">
              ♥
            </span>
          )}
          {file.rating > 0 && (
            <span className="px-1 h-4 rounded bg-black/55 text-keep text-[10px] flex items-center tracking-tighter">
              {"★".repeat(file.rating)}
            </span>
          )}
          {file.culling_flag !== 0 && (
            <span
              className="w-4 h-4 rounded text-[10px] font-bold flex items-center justify-center"
              style={
                file.culling_flag === 1
                  ? {
                      background: "var(--color-keep)",
                      color: "var(--color-keep-fg)",
                    }
                  : {
                      background: "var(--color-drop)",
                      color: "var(--color-drop-fg)",
                    }
              }
              title={file.culling_flag === 1 ? "남김" : "제외"}
            >
              {file.culling_flag === 1 ? "★" : "✕"}
            </span>
          )}
        </div>

        {/* 아래 — 미디어 배지 (Lap: bottom badges). 형식과 길이 */}
        <div className="absolute bottom-1 left-1 flex items-center gap-1">
          {file.kind === 2 && (
            <span className="px-1 h-4 rounded bg-black/55 text-keep text-[9px] flex items-center font-semibold">
              RAW
            </span>
          )}
          {isVideo && !ready && (
            <span className="px-1 h-4 rounded bg-black/55 text-fg text-[9px] flex items-center gap-0.5">
              ▶{file.duration_ms ? fmtDuration(file.duration_ms) : ""}
            </span>
          )}
        </div>
      </div>
      {/* 사진 아래 — 이름과 날짜·크기.
          썸네일만으로는 어느 파일인지 알 수 없다. 파인더에서 찾거나 남에게
          말할 때 필요한 건 결국 이름이다. */}
      {caption && (
        <div className="mt-1 leading-[1.25]">
          <div className="text-[11px] text-fg-dim truncate" title={file.name}>
            {file.name}
          </div>
          <div className="text-[10px] text-fg-mute truncate tabular-nums">
            {fmtDate(file.taken_at)}
            <span className="text-fg-faint"> · </span>
            {fmtBytes(file.size)}
          </div>
        </div>
      )}
    </button>
  );
}

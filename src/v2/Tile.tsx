import { useCallback, useEffect, useRef, useState } from "react";
import {
  HOVER_DELAY_MS,
  claimHoverPreview,
  releaseHoverPreview,
} from "./hoverPreview";

export type TileFile = {
  id: number;
  name: string;
  kind: number;
  rating: number;
  culling_flag: number;
  favorite: boolean;
  taken_at: number;
  duration_ms: number | null;
};

const fmtDuration = (ms: number) => {
  const t = Math.round(ms / 1000);
  const m = Math.floor(t / 60);
  const s = t % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
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
  label,
}: {
  file: TileFile;
  /** 썸네일 주소. 아직 없으면 null */
  url: string | null;
  picked: boolean;
  focused: boolean;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  label: string;
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
      onMouseEnter={enter}
      onMouseLeave={stop}
      className="text-left"
    >
      <div
        className="rounded overflow-hidden bg-[#0F1314] relative"
        style={{
          aspectRatio: "1/1",
          boxShadow: picked
            ? "0 0 0 2px #49B8B4"
            : focused
              ? "0 0 0 2px #6C6CE8"
              : undefined,
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
          <div className="w-full h-full flex items-center justify-center text-[#3A4547] text-[10px]">
            {isVideo ? "영상" : "…"}
          </div>
        )}

        {playing && (
          <video
            ref={video}
            src={`video://localhost/${file.id}`}
            className="absolute inset-0 w-full h-full object-cover pointer-events-none transition-opacity duration-150"
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

        {file.kind === 2 && (
          <span className="absolute top-1 left-1 text-[9px] px-1 rounded bg-black/60 text-[#F0B429]">
            RAW
          </span>
        )}
        {isVideo && !ready && (
          <span className="absolute top-1 left-1 text-[9px] px-1 rounded bg-black/60 text-[#EAEFEF]">
            ▶{file.duration_ms ? ` ${fmtDuration(file.duration_ms)}` : ""}
          </span>
        )}

        {file.culling_flag !== 0 && (
          <span
            className="absolute top-1 right-1 w-4 h-4 rounded text-[10px] font-bold flex items-center justify-center"
            style={
              file.culling_flag === 1
                ? { background: "#F0B429", color: "#231A00" }
                : { background: "#E2685C", color: "#2A0D09" }
            }
            title={file.culling_flag === 1 ? "남김" : "제외"}
          >
            {file.culling_flag === 1 ? "★" : "✕"}
          </span>
        )}
        {(file.rating > 0 || file.favorite) && (
          <div className="absolute bottom-0 inset-x-0 flex items-center gap-1 px-1 py-0.5 bg-gradient-to-t from-black/70 to-transparent">
            {file.rating > 0 && (
              <span className="text-[9px] text-[#F0B429] tracking-tighter">
                {"★".repeat(file.rating)}
              </span>
            )}
            <div className="flex-1" />
            {file.favorite && (
              <span className="text-[9px] text-[#E2685C]">♥</span>
            )}
          </div>
        )}
      </div>
      <div className="text-[10.5px] text-[#6D7B7E] mt-1 truncate tabular-nums">
        {label}
      </div>
    </button>
  );
}

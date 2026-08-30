import { useState } from "react";
import { fmtDuration } from "./format";
import type { FileRow } from "./types";
import { usePref } from "./prefs";

/**
 * 필름스트립 아래 — 고른 한 장을 크게.
 *
 * 그리드 대신 이게 뜬다 (Lap의 Content.vue: 위는 띠, 아래는 미리보기).
 * 크게 보기(뷰어)와 다른 점: 확대·정보 패널이 없고 자리를 안 덮는다.
 * 두 번 누르면 뷰어로 간다.
 */
export default function Preview({
  file,
  onOpen,
}: {
  file: FileRow | null;
  /** 두 번 누르면 크게 보기 */
  onOpen: () => void;
}) {
  const [failedId, setFailedId] = useState<number | null>(null);
  const [autoplay] = usePref("autoplay");
  const [loopVideo] = usePref("loopVideo");
  if (!file) {
    return (
      <div className="flex-1 flex items-center justify-center text-fg-mute text-[14px]">
        위 띠에서 사진을 고르세요
      </div>
    );
  }
  const failed = failedId === file.id;
  const isVideo = file.kind === 1;
  const src = `photo://localhost/${file.id}`;

  return (
    <div
      className="flex-1 min-w-0 min-h-0 flex items-center justify-center bg-canvas p-2 select-none"
      onDoubleClick={onOpen}
      title="두 번 누르면 크게 보기"
    >
      {failed ? (
        <div className="text-fg-mute text-[14px]">
          읽을 수 없는 파일입니다 — {file.name}
        </div>
      ) : isVideo ? (
        <video
          key={file.id}
          src={`video://localhost/${file.id}`}
          poster={src}
          controls
          autoPlay={autoplay}
          loop={loopVideo}
          playsInline
          onError={() => setFailedId(file.id)}
          className="max-w-full max-h-full rounded"
        />
      ) : (
        <img
          key={file.id}
          src={src}
          draggable={false}
          onError={() => setFailedId(file.id)}
          className="max-w-full max-h-full object-contain rounded"
        />
      )}
      {isVideo && file.duration_ms ? (
        <span className="absolute bottom-3 right-3 px-2 py-0.5 rounded bg-black/55 text-fg text-[12px] tabular-nums pointer-events-none">
          ▶ {fmtDuration(file.duration_ms)}
        </span>
      ) : null}
    </div>
  );
}

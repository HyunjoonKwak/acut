import { fmtBytes } from "./format";
import { thumbUrlOf } from "./types";

/** 개별 비교의 사진 한 칸 — 남길 사진은 ★, 나머지는 «선택» 단추 */
export type CullMember = {
  file_id: number;
  library_id: number | null;
  name: string;
  size: number;
  is_best: boolean;
  thumb: string | null;
  culling_flag: number;
  library: string;
  folder: string;
  area: number;
};

export default function CullTile<M extends CullMember>({
  m,
  i,
  kind,
  onPick,
  onView,
  onKeep,
}: {
  m: M;
  i: number;
  kind: number;
  onPick: (fileId: number) => void;
  onView: (index: number) => void;
  /** 이 사진을 남기기로 — 부르는 쪽의 더 넓은 구성원 타입을 그대로 받는다 */
  onKeep: (m: M) => void;
}) {
  const u =
    m.thumb && m.library_id !== null ? thumbUrlOf(m.library_id, m.thumb) : null;
  return (
    <div className="min-w-0">
      {/* block·w-full — 인라인 단추는 썸네일 원래 폭(512px)으로 커져 칸을 넘어 옆 사진과 겹친다
          (넷 이상 겹치는 무리에서 실측 2026-08-30) */}
      <button
        onClick={() => onPick(m.file_id)}
        onDoubleClick={() => onView(i)}
        className="block w-full min-w-0 text-left"
      >
        <div
          className="relative rounded-lg overflow-hidden bg-canvas"
          style={{
            aspectRatio: "3/2",
            boxShadow: m.is_best
              ? "0 0 0 2px var(--color-keep), 0 8px 22px -10px rgb(240 180 41 / 0.5)"
              : "0 0 0 1px var(--color-line-strong)",
          }}
        >
          {u ? (
            <img
              src={u}
              loading="lazy"
              className="w-full h-full object-cover"
              style={{ opacity: m.is_best ? 1 : 0.55 }}
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center text-fg-faint">
              …
            </div>
          )}
          {m.is_best && m.culling_flag !== 2 ? (
            <span className="absolute top-2 left-2 h-5 px-2 rounded bg-keep text-keep-fg text-[11px] font-bold flex items-center">
              ★ 남김
            </span>
          ) : (
            <span className="absolute top-2 left-2 h-5 px-2 rounded bg-drop/90 text-drop-fg text-[11px] font-bold flex items-center">
              ✕ 제외
            </span>
          )}
          {i < 9 && (
            <span className="absolute top-2 right-2 w-5 h-5 rounded bg-black/55 text-white text-[11px] flex items-center justify-center tabular-nums">
              {i + 1}
            </span>
          )}
        </div>
        <div className="flex justify-between items-baseline mt-1.5 gap-2">
          <span className="text-[11.5px] text-fg-dim truncate">{m.name}</span>
          <span className="text-[11px] text-fg-mute tabular-nums shrink-0">
            {fmtBytes(m.size)}
          </span>
        </div>
        {/* 어디 있는 사본인가 — 어느 쪽을 남길지는 결국 폴더로 정한다 */}
        <div
          className={`text-[10.5px] truncate ${
            m.area === 1 || m.area === 2 ? "text-keep" : "text-fg-mute"
          }`}
          title={`${m.library} / ${m.folder || "/"}`}
        >
          {m.library} · {m.folder || "/"}
        </div>
      </button>
      {kind === 0 && (
        <div className="flex gap-1.5 mt-1.5 h-7 items-center">
          {m.is_best ? (
            // 이미 남길 사진 — 단추가 아니라 상태. 다른 쪽을 «선택»하면 이쪽이 제외로 바뀐다
            <span className="text-[12px] text-keep font-semibold">
              ★ 남길 사진
            </span>
          ) : (
            <button
              onClick={() => onKeep(m)}
              title="이 사진을 남기고, 지금 ★인 사진을 비롯한 나머지에 제외 표시 — 바로 확정됩니다"
              className="h-7 px-2.5 rounded-md bg-keep text-keep-fg text-[12px] font-semibold"
            >
              선택
            </button>
          )}
        </div>
      )}
    </div>
  );
}

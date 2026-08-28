import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Viewer from "./Viewer";
import { thumbUrl, type FileRow, type Mark } from "./types";
import { Btn } from "./ui";

type Hit = { file: FileRow; score: number };

/**
 * 비슷한 사진 — 기준 한 장과 그것에 가까운 것들.
 *
 * 결과를 누르면 그것이 새 기준이 된다 — 꼬리를 물고 찾아간다. 두 번 누르면
 * 크게 본다(뷰어는 id만 있으면 되므로 목록에 없는 사진도 된다).
 */
export default function Similar({
  id,
  onPick,
  onMark,
  onClose,
}: {
  id: number;
  /** 결과를 새 기준으로 */
  onPick: (id: number) => void;
  onMark: (id: number, patch: Mark) => void;
  onClose: () => void;
}) {
  // 결과를 «어느 기준에 대한 것인지»와 함께 둔다. 기준이 바뀌면 안 맞아
  // 저절로 «찾는 중»이 된다 — 효과 안에서 비울 일이 없다.
  const [got, setGot] = useState<{
    id: number;
    hits: Hit[] | null;
    err: string | null;
  } | null>(null);
  const hits = got?.id === id ? got.hits : null;
  const err = got?.id === id ? got.err : null;
  const [src, setSrc] = useState<FileRow | null>(null);
  const [viewer, setViewer] = useState<number | null>(null);

  useEffect(() => {
    let live = true;
    invoke<Hit[]>("ai_similar", { id, limit: 48 })
      .then((h) => live && setGot({ id, hits: h, err: null }))
      .catch((e) => live && setGot({ id, hits: null, err: String(e) }));
    // 기준 사진 자신 — 목록 한 줄이면 충분하다
    invoke<FileRow[]>("files_by_ids", { ids: [id] })
      .then((r) => live && setSrc(r[0] ?? null))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (viewer !== null) return; // 뷰어가 키를 맡는다
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, viewer]);

  const ids = hits?.map((h) => h.file.id) ?? [];

  return (
    <div className="fixed inset-0 z-[60] bg-canvas flex flex-col">
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 bg-raised/95 border-b border-line text-[12.5px]">
        <span className="text-fg font-semibold">비슷한 사진</span>
        {src && (
          <span className="flex items-center gap-2 text-fg-mute min-w-0">
            <img
              src={thumbUrl(src) ?? undefined}
              className="h-7 w-7 rounded object-cover"
            />
            <span className="truncate">{src.name}</span>
          </span>
        )}
        {hits && (
          <span className="text-fg-mute tabular-nums">{hits.length}장</span>
        )}
        <div className="flex-1" />
        <Btn onClick={onClose} hint="Esc">
          닫기
        </Btn>
      </div>

      <div className="flex-1 overflow-y-auto p-3">
        {err && (
          <div className="h-full flex items-center justify-center text-fg-mute text-[13px]">
            {err}
          </div>
        )}
        {!err && hits === null && (
          <div className="h-full flex items-center justify-center text-fg-faint text-[13px]">
            찾는 중…
          </div>
        )}
        {hits && hits.length === 0 && (
          <div className="h-full flex items-center justify-center text-fg-mute text-[13px]">
            비슷한 사진이 없습니다
          </div>
        )}
        {hits && hits.length > 0 && (
          <div
            className="grid gap-2.5"
            style={{
              gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))",
            }}
          >
            {hits.map((h, i) => (
              <button
                key={h.file.id}
                onClick={() => onPick(h.file.id)}
                onDoubleClick={() => setViewer(i)}
                title={`${h.file.name} — 누르면 이것을 기준으로, 두 번 누르면 크게`}
                className="text-left group"
              >
                <div className="relative aspect-square rounded overflow-hidden bg-raised">
                  {thumbUrl(h.file) ? (
                    <img
                      src={thumbUrl(h.file)!}
                      loading="lazy"
                      className="w-full h-full object-cover"
                    />
                  ) : (
                    <div className="w-full h-full flex items-center justify-center text-fg-faint text-[10px]">
                      …
                    </div>
                  )}
                  {/* 닮은 정도 — 코사인. 0.9 위면 거의 같은 장면 */}
                  <span
                    className={`absolute top-1 right-1 px-1 h-4 rounded text-[9.5px] tabular-nums flex items-center ${
                      h.score >= 0.9
                        ? "bg-keep text-keep-fg font-semibold"
                        : "bg-black/55 text-fg"
                    }`}
                  >
                    {(h.score * 100).toFixed(0)}%
                  </span>
                </div>
                <div className="mt-1 text-[10.5px] text-fg-mute truncate">
                  {h.file.name}
                </div>
              </button>
            ))}
          </div>
        )}
      </div>

      {viewer !== null && hits && (
        <Viewer
          ids={ids}
          index={viewer}
          onIndex={setViewer}
          onClose={() => setViewer(null)}
          onMark={onMark}
          fullScreen
          onToggleFullScreen={() => {}}
          kindOf={(fid) => hits.find((h) => h.file.id === fid)?.file.kind}
        />
      )}
    </div>
  );
}

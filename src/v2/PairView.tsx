import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { toast } from "./toastStore";
import { thumbUrl } from "./types";
import type { FolderIn } from "./twoFoldersLogic";

/**
 * 폴더 짝 «보기» — A 폴더와 B 폴더의 사진을 나란히 놓고 직접 골라 제외·남김 표시한다.
 *
 * 내용이 같은 사진은 양쪽에 «동일» 배지가 붙고, 한쪽에 마우스를 올리면 반대쪽 짝이 같이
 * 밝아진다. 표시만 붙이고 파일은 안 옮긴다 — 실제 이동은 비교 화면의 ③ «휴지통으로 보내기».
 */
type Photo = {
  file_id: number;
  name: string;
  sub: string;
  size: number;
  taken_at: number;
  culling_flag: number;
  library_id: number;
  thumb: string | null;
  twin: number | null;
};

export default function PairView({
  a,
  b,
  aIds,
  bIds,
  onClose,
}: {
  a: FolderIn;
  b: FolderIn;
  aIds: number[];
  bIds: number[];
  /** 닫으면 비교 목록을 새로 읽는다 */
  onClose: () => void;
}) {
  const [photos, setPhotos] = useState<{ a: Photo[]; b: Photo[] } | null>(null);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [hover, setHover] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setPhotos(await invoke<{ a: Photo[]; b: Photo[] }>("cull_folder_pair_photos", { aIds, bIds }));
    } catch (e) {
      toast(String(e), "drop");
    }
  }, [aIds, bIds]);
  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const all = useMemo(() => [...(photos?.a ?? []), ...(photos?.b ?? [])], [photos]);
  const pickedBytes = all.filter((p) => picked.has(p.file_id)).reduce((s, p) => s + p.size, 0);
  const toggle = (id: number) =>
    setPicked((cur) => {
      const next = new Set(cur);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const pickTwins = (side: "a" | "b") => {
    const list = side === "a" ? (photos?.a ?? []) : (photos?.b ?? []);
    setPicked(new Set(list.filter((p) => p.twin !== null).map((p) => p.file_id)));
  };

  const mark = async (flag: 1 | 2 | 0) => {
    if (picked.size === 0) return;
    setBusy(true);
    try {
      await invoke("files_mark", { ids: [...picked], rating: null, cullingFlag: flag, favorite: null });
      toast(
        flag === 2
          ? `${picked.size.toLocaleString()}장에 제외 표시했습니다 — 비교 화면의 «휴지통으로 보내기»로 옮깁니다`
          : flag === 1
            ? `${picked.size.toLocaleString()}장에 남김 표시했습니다`
            : `${picked.size.toLocaleString()}장의 표시를 지웠습니다`,
        "ok",
      );
      setPicked(new Set());
      await load();
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setBusy(false);
    }
  };

  const twinOfHover = useMemo(() => all.find((p) => p.file_id === hover)?.twin ?? null, [all, hover]);

  return (
    <div className="absolute inset-0 z-30 bg-canvas flex flex-col">
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 border-b border-line text-[12.5px] bar-fixed">
        <span className="text-fg font-semibold">폴더 보기</span>
        <span className="text-fg-dim">
          같은 사진에는 <span className="text-ok font-semibold">동일</span> 표시 — 클릭해서 고르고 아래에서 표시를 붙입니다
        </span>
        <div className="flex-1" />
        <button onClick={() => pickTwins("a")} className="h-7 px-2.5 rounded-md text-fg-dim ring-1 ring-line-strong text-[12px]">
          A쪽 동일 사진 전부 선택
        </button>
        <button onClick={() => pickTwins("b")} className="h-7 px-2.5 rounded-md text-fg-dim ring-1 ring-line-strong text-[12px]">
          B쪽 동일 사진 전부 선택
        </button>
        <button onClick={onClose} className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px]">
          닫기 <span className="text-[10px] font-mono">Esc</span>
        </button>
      </div>

      <div className="flex-1 min-h-0 grid grid-cols-2 divide-x divide-line">
        {(["a", "b"] as const).map((side) => {
          const f = side === "a" ? a : b;
          const list = side === "a" ? photos?.a : photos?.b;
          return (
            <div key={side} className="min-h-0 flex flex-col">
              <div className="shrink-0 px-4 py-1.5 text-[12px] border-b border-line flex items-center gap-2">
                <span className="text-fg-mute font-semibold">{side.toUpperCase()}</span>
                <span className="truncate" title={`${f.library} / ${f.folder || "/"}`}>
                  {f.library} · {f.folder || "/"}
                </span>
                {list && (
                  <span className="text-fg-mute tabular-nums ml-auto">
                    {list.length.toLocaleString()}장 · 동일 {list.filter((p) => p.twin !== null).length.toLocaleString()}
                  </span>
                )}
              </div>
              <div className="flex-1 min-h-0 overflow-y-auto scroll-thin p-3">
                {!list ? (
                  <div className="text-fg-mute text-[12.5px]">읽는 중…</div>
                ) : (
                  <div className="grid gap-2" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(132px, 1fr))" }}>
                    {list.map((p) => {
                      const on = picked.has(p.file_id);
                      const lit = hover === p.file_id || twinOfHover === p.file_id;
                      const u = thumbUrl(p);
                      return (
                        <button
                          key={p.file_id}
                          onClick={() => toggle(p.file_id)}
                          onMouseEnter={() => setHover(p.file_id)}
                          onMouseLeave={() => setHover(null)}
                          className="text-left"
                          title={`${p.sub ? p.sub + "/" : ""}${p.name} · ${fmtBytes(p.size)}`}
                        >
                          <div
                            className="relative rounded-md overflow-hidden bg-raised"
                            style={{
                              aspectRatio: "1",
                              boxShadow: on
                                ? "0 0 0 2px var(--color-accent)"
                                : lit
                                  ? "0 0 0 2px var(--color-ok)"
                                  : "0 0 0 1px var(--color-line-strong)",
                            }}
                          >
                            {u ? (
                              <img src={u} loading="lazy" className="w-full h-full object-cover" style={{ opacity: on ? 1 : 0.9 }} />
                            ) : (
                              <div className="w-full h-full flex items-center justify-center text-fg-faint">…</div>
                            )}
                            {p.twin !== null && (
                              <span className="absolute top-1 left-1 h-4 px-1.5 rounded bg-ok/90 text-[#0b2a1a] text-[10px] font-bold flex items-center">
                                동일
                              </span>
                            )}
                            {p.culling_flag === 2 && (
                              <span className="absolute top-1 right-1 h-4 px-1.5 rounded bg-drop/90 text-drop-fg text-[10px] font-bold flex items-center">
                                제외
                              </span>
                            )}
                            {p.culling_flag === 1 && (
                              <span className="absolute top-1 right-1 h-4 px-1.5 rounded bg-keep text-keep-fg text-[10px] font-bold flex items-center">
                                남김
                              </span>
                            )}
                            {on && (
                              <span className="absolute bottom-1 right-1 w-4 h-4 rounded-full bg-accent text-accent-fg text-[10px] flex items-center justify-center">
                                ✓
                              </span>
                            )}
                          </div>
                          <div className="mt-1 text-[10.5px] text-fg-mute truncate">
                            {p.sub ? <span className="text-fg-faint">{p.sub}/</span> : null}
                            {p.name}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>

      <div className="h-12 shrink-0 flex items-center gap-2 px-4 bg-chrome border-t border-line text-[12.5px] bar-fixed">
        <span className="tabular-nums">
          <b className="text-accent">{picked.size.toLocaleString()}장</b> 선택
          <span className="text-fg-mute"> · {fmtBytes(pickedBytes)}</span>
        </span>
        <button
          onClick={() => mark(2)}
          disabled={busy || picked.size === 0}
          className="h-control px-3 rounded-md bg-keep text-keep-fg font-semibold disabled:opacity-40"
        >
          고른 사진 제외 표시
        </button>
        <button
          onClick={() => mark(1)}
          disabled={busy || picked.size === 0}
          className="h-control px-3 rounded-md text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
        >
          남김 표시
        </button>
        <button
          onClick={() => mark(0)}
          disabled={busy || picked.size === 0}
          className="h-control px-3 rounded-md text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
        >
          표시 지우기
        </button>
        <div className="flex-1" />
        {picked.size > 0 && (
          <button onClick={() => setPicked(new Set())} className="h-control px-2 rounded-md text-fg-dim">
            선택 해제
          </button>
        )}
      </div>
    </div>
  );
}

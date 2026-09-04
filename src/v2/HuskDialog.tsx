import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalFocus } from "./focus";
import { fmtBytes } from "./format";
import { toast } from "./toastStore";
import { useData } from "./dataStore";

type Husk = { rel: string; files: number; bytes: number; kinds: [string, number][] };

/**
 * 사진 없는 폴더 정리 — 사진을 다 치운 뒤 카메라 메모(txt)·썸네일(thm)·zip 만 남은 «껍데기» 폴더들.
 * 목록을 보여 주고 고른 것을 라이브러리 휴지통(.acut/휴지통/_폴더)으로 통째로 옮긴다.
 * Finder 로 되살릴 수 있고, «영구히 비우기»에서 같이 사라진다.
 */
export default function HuskDialog({ libraryId, name, onClose }: { libraryId: number; name: string; onClose: () => void }) {
  const [list, setList] = useState<Husk[] | null>(null);
  const [off, setOff] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  /// 챙길 만한 것이 든 폴더 — 편집 파일·압축·일러스트. 기본으로 체크를 풀어 둔다
  const keepish = (h: Husk) => h.kinds.some(([k]) => ["psd", "ai", "zip", "pct", "lrcat", "xmp"].includes(k));
  useEffect(() => {
    let live = true;
    invoke<Husk[]>("husk_list", { libraryId })
      .then((r) => {
        if (!live) return;
        setList(r);
        setOff(new Set(r.filter(keepish).map((h) => h.rel)));
      })
      .catch((e) => {
        if (!live) return;
        toast(String(e), "drop");
        setList([]);
      });
    return () => {
      live = false;
    };
  }, [libraryId]);
  const dialogRef = useRef<HTMLDivElement>(null);
  useModalFocus(dialogRef, onClose, { locked: busy });

  const picked = useMemo(() => (list ?? []).filter((h) => !off.has(h.rel)), [list, off]);
  const bytes = picked.reduce((s, h) => s + h.bytes, 0);
  const files = picked.reduce((s, h) => s + h.files, 0);
  const run = async () => {
    if (picked.length === 0) return;
    setBusy(true);
    try {
      const [n, err] = await invoke<[number, string | null]>("husk_trash", { libraryId, rels: picked.map((h) => h.rel) });
      toast(err ? `${n}개 폴더 옮김 · 일부 실패 — ${err}` : `${n.toLocaleString()}개 폴더를 휴지통(_폴더)으로 옮겼습니다 — «영구히 비우기»에서 같이 사라집니다`, err ? "drop" : "ok");
      await useData.getState().loadFolders();
      onClose();
    } catch (e) {
      toast(String(e), "drop");
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[70] bg-canvas/80 backdrop-blur-sm flex items-center justify-center p-6">
      <div ref={dialogRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={`「${name}」의 사진 없는 폴더 정리`} className="w-[720px] max-w-full max-h-[85vh] bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5 flex flex-col">
        <div className="text-[16px] font-semibold text-fg mb-1">「{name}」의 사진 없는 폴더 정리</div>
        <div className="text-[13px] text-fg-mute mb-3">
          사진을 다 치운 뒤 카메라 메모(txt)·썸네일(thm)·zip 같은 파일만 남은 폴더들입니다. 체크된 폴더를 통째로 라이브러리 휴지통(<code>.acut/휴지통/_폴더</code>)으로 옮깁니다 — Finder 로 되살릴 수 있고, «영구히 비우기»에서 같이 사라집니다. 편집 파일(psd·ai·zip 등)이 든 폴더는 체크를 풀어 두었습니다.
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto scroll-thin rounded-md ring-1 ring-line">
          {list === null ? (
            <div className="p-4 text-fg-mute text-[13.5px]">훑는 중…</div>
          ) : list.length === 0 ? (
            <div className="p-4 text-fg-mute text-[13.5px]">사진 없는 폴더가 없습니다</div>
          ) : (
            <table className="w-full text-[13.5px] tabular-nums">
              <thead className="text-[11.5px] text-fg-mute uppercase tracking-wider sticky top-0 bg-canvas">
                <tr className="text-left">
                  <th className="py-1.5 pl-3 w-8">
                    <input
                      type="checkbox"
                      className="accent-accent w-3.5 h-3.5"
                      checked={off.size === 0}
                      onChange={(e) => setOff(e.target.checked ? new Set() : new Set(list.map((h) => h.rel)))}
                      title="전부 고르기 / 풀기"
                    />
                  </th>
                  <th className="py-1.5 pr-3 font-medium">폴더</th>
                  <th className="py-1.5 pr-3 font-medium text-right">파일</th>
                  <th className="py-1.5 pr-3 font-medium text-right">용량</th>
                  <th className="py-1.5 pr-3 font-medium">무엇이</th>
                </tr>
              </thead>
              <tbody>
                {list.map((h) => {
                  const on = !off.has(h.rel);
                  return (
                    <tr key={h.rel} className={`border-t border-line ${on ? "" : "opacity-50"}`}>
                      <td className="py-1 pl-3">
                        <input
                          type="checkbox"
                          className="accent-accent w-3.5 h-3.5"
                          checked={on}
                          onChange={() =>
                            setOff((cur) => {
                              const next = new Set(cur);
                              if (next.has(h.rel)) next.delete(h.rel);
                              else next.add(h.rel);
                              return next;
                            })
                          }
                        />
                      </td>
                      <td className="py-1 pr-3 truncate max-w-[360px]" title={h.rel}>
                        {h.rel}
                      </td>
                      <td className="py-1 pr-3 text-right">{h.files.toLocaleString()}</td>
                      <td className="py-1 pr-3 text-right text-fg-dim">{fmtBytes(h.bytes)}</td>
                      <td className={`py-1 pr-3 ${keepish(h) ? "text-keep" : "text-fg-mute"}`}>
                        {h.kinds.map(([k, n]) => `${k} ${n}`).join(" · ")}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
        <div className="flex items-center gap-2 mt-4">
          <span className="text-[13.5px] text-fg-dim tabular-nums">
            고른 폴더 <b className="text-fg">{picked.length.toLocaleString()}</b>개 · {files.toLocaleString()}파일 · {fmtBytes(bytes)}
          </span>
          <div className="flex-1" />
          <button onClick={onClose} className="h-control px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[13.5px]">
            닫기
          </button>
          <button
            onClick={run}
            disabled={busy || picked.length === 0}
            className="h-control px-3 rounded-md bg-drop text-drop-fg font-semibold text-[13.5px] disabled:opacity-40"
          >
            {busy ? "옮기는 중…" : `고른 ${picked.length.toLocaleString()}개 폴더 휴지통으로`}
          </button>
        </div>
      </div>
    </div>
  );
}

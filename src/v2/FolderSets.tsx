import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";

/**
 * 폴더 비교 — 내용이 완전히 같은 폴더들을 묶어 나열한다.
 *
 * 후보1번에도, 후보2번에도, 공용에도 같은 폴더가 있으면 셋을 한 줄에. 사진은
 * 보여 주지 않는다 — 폴더가 같은데 사진을 볼 이유가 없다. 남길 폴더 하나를
 * 고르고(NAS 것이 기본) 나머지에 제외 표시 — 격자·상태바와 같은 낱말.
 */

type FolderIn = {
  folder_id: number;
  library_id: number;
  library: string;
  folder: string;
  area: number;
};
type FolderSet = { folders: FolderIn[]; files: number; bytes: number; pending: boolean };
type ApplyAll = { groups: number; kept: number; rejected: number; skipped: number };

const settled = (f: FolderIn) => f.area === 1 || f.area === 2;
/** 묶음의 열쇠 — 폴더 id 조합. 목록 순번은 새로 읽을 때마다 바뀐다 */
const setKey = (s: FolderSet) => s.folders.map((f) => f.folder_id).join("-");

export default function FolderSets({ onChanged }: { onChanged: () => void }) {
  const ask = useConfirm();
  const [sets, setSets] = useState<FolderSet[] | null>(null);
  /// 묶음마다 남길 폴더 — 기본은 맨 앞(정착 구역 우선으로 정렬돼 온다).
  /// 열쇠는 폴더 id 조합 — 배열 순번으로 두면 목록이 새로 오며 다른 묶음에 붙는다 (리뷰 H4)
  const [keep, setKeep] = useState<Record<string, number>>({});
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let live = true;
    invoke<FolderSet[]>("cull_folder_sets").then((s) => live && setSets(s));
    return () => {
      live = false;
    };
  }, [tick]);

  const applyOne = useCallback(
    async (s: FolderSet, quiet = false) => {
      const k = Math.min(keep[setKey(s)] ?? 0, s.folders.length - 1);
      const keepF = s.folders[k];
      const drops = s.folders.filter((_, j) => j !== k);
      if (!quiet) {
        const risky = drops.filter(settled);
        const ok = await ask({
          title: `«${keepF.library} · ${keepF.folder || "/"}»만 남기고 나머지 ${drops.length}개 폴더에 제외 표시`,
          lines: [
            `${drops.map((d) => `${d.library} · ${d.folder || "/"}`).join(" / ")} — ${s.files.toLocaleString()}장씩, ${fmtBytes(s.bytes * drops.length)} 빔`,
            ...(risky.length
              ? [`주의: ${risky.map((d) => d.library).join(", ")}은 NAS 동기화 폴더입니다 — 치우면 NAS에서도 지워집니다`]
              : []),
            "파일은 아직 옮기지 않습니다 — 격자의 «제외 N장 치우기»로 휴지통에 보냅니다",
          ],
          confirmLabel: "제외 표시",
          danger: risky.length > 0,
        });
        if (!ok) return false;
      }
      await invoke<ApplyAll>("cull_folder_set_apply", {
        keepFolderId: keepF.folder_id,
        dropFolderIds: drops.map((d) => d.folder_id),
      });
      return true;
    },
    [keep, ask],
  );

  const applyAllNas = useCallback(async () => {
    if (!sets) return;
    // 남길 것은 사용자가 ●로 고른 폴더(기본은 맨 앞). 그것이 NAS 폴더이고 나머지가 전부
    // NAS 밖인 묶음만 — 위험한 것은 사람이. ●를 다른 데 두어 조건에 안 맞는 묶음은 건너뛴다
    const keepAt = (s: FolderSet) => Math.min(keep[setKey(s)] ?? 0, s.folders.length - 1);
    const todo = sets.filter((s) => {
      const k = keepAt(s);
      return s.pending && settled(s.folders[k]) && s.folders.every((f, j) => j === k || !settled(f));
    });
    const passed = sets.filter((s) => s.pending).length - todo.length;
    if (todo.length === 0) {
      toast("NAS 것을 남기고 처리할 묶음이 없습니다");
      return;
    }
    const bytes = todo.reduce((a, s) => a + s.bytes * (s.folders.length - 1), 0);
    const ok = await ask({
      title: `${todo.length.toLocaleString()}묶음 — NAS 폴더를 남기고 나머지에 제외 표시`,
      lines: [
        `${fmtBytes(bytes)} 빔`,
        "NAS 밖(T7·작업대) 폴더에만 표시합니다",
        ...(passed > 0 ? [`${passed.toLocaleString()}묶음은 남길 폴더가 NAS 것이 아니라 건너뜁니다`] : []),
        "파일은 아직 옮기지 않습니다",
      ],
      confirmLabel: "전부 처리",
    });
    if (!ok) return;
    let failed = 0;
    let firstErr = "";
    for (const s of todo) {
      const k = keepAt(s);
      try {
        await invoke<ApplyAll>("cull_folder_set_apply", {
          keepFolderId: s.folders[k].folder_id,
          dropFolderIds: s.folders.filter((_, j) => j !== k).map((d) => d.folder_id),
        });
      } catch (e) {
        failed += 1;
        firstErr ||= String(e);
      }
    }
    toast(
      failed
        ? `${(todo.length - failed).toLocaleString()}묶음 처리 · ${failed}묶음 실패 (${firstErr}) — 목록을 새로 읽습니다`
        : `${todo.length.toLocaleString()}묶음 처리했습니다 — 격자에서 «치우기»`,
      failed ? "drop" : "ok",
    );
    setTick((t) => t + 1);
    onChanged();
  }, [sets, keep, ask, onChanged]);

  if (sets === null)
    return (
      <div className="h-full flex items-center justify-center gap-2 text-fg-mute">
        <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 폴더를 견주는 중…
      </div>
    );
  const pending = sets.filter((s) => s.pending);
  const gain = pending.reduce((a, s) => a + s.bytes * (s.folders.length - 1), 0);
  if (sets.length === 0)
    return (
      <div className="h-full flex items-center justify-center text-fg-mute">
        내용이 완전히 같은 폴더가 없습니다 — 부분만 겹치는 것은 «개별 비교»에서
      </div>
    );

  return (
    <div className="h-full flex flex-col">
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 border-b border-line text-[12.5px]">
        <span className="text-fg-dim tabular-nums">
          완전히 같은 폴더 <b className="text-fg">{sets.length.toLocaleString()}묶음</b>
          {pending.length > 0 && (
            <>
              {" "}· 아직 안 한 것 {pending.length.toLocaleString()}묶음 · 하나만 남기면{" "}
              <b className="text-keep">{fmtBytes(gain)}</b> 빔
            </>
          )}
        </span>
        <span className="text-fg-mute">
          — 남길 폴더의 ○을 누르면 ● 남김이 되고 나머지는 제외. 건너뛰려면 그냥 두세요
        </span>
        <div className="flex-1" />
        {pending.length > 0 && (
          <button
            onClick={applyAllNas}
            className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px]"
          >
            NAS 것 남기고 전부 처리
          </button>
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
        {sets.map((s) => {
          const key = setKey(s);
          const k = Math.min(keep[key] ?? 0, s.folders.length - 1);
          return (
            <div
              key={key}
              className={`px-4 py-3 border-b border-line ${s.pending ? "" : "opacity-45"}`}
            >
              <div className="flex items-center gap-3 mb-1.5 text-[12px] text-fg-mute tabular-nums">
                <span>
                  {s.folders.length}곳에 같은 폴더 · {s.files.toLocaleString()}장 · 폴더당 {fmtBytes(s.bytes)}
                </span>
                {!s.pending && <span className="text-ok">처리됨</span>}
                <div className="flex-1" />
                {s.pending && (
                  <button
                    onClick={async () => {
                      if (await applyOne(s)) {
                        toast("제외 표시했습니다 — 격자에서 «치우기»", "ok");
                        setTick((t) => t + 1);
                        onChanged();
                      }
                    }}
                    className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12px]"
                  >
                    고른 것만 남기고 나머지 제외 표시
                  </button>
                )}
              </div>
              <div className="grid gap-1" style={{ gridTemplateColumns: "auto 1fr auto" }}>
                {s.folders.map((f, j) => (
                  <label
                    key={f.folder_id}
                    className={`contents cursor-pointer ${s.pending ? "" : "pointer-events-none"}`}
                  >
                    <span className="flex items-center justify-center w-16 text-[11px]">
                      <input
                        type="radio"
                        name={`keep-${key}`}
                        checked={k === j}
                        onChange={() => setKeep((m) => ({ ...m, [key]: j }))}
                        className="accent-keep mr-1.5 w-3.5 h-3.5"
                      />
                      <span className={k === j ? "text-keep font-semibold" : "text-fg-mute"}>
                        {k === j ? "남김" : "제외"}
                      </span>
                    </span>
                    <span className="truncate text-[13px]" title={`${f.library} / ${f.folder || "/"}`}>
                      <span className={settled(f) ? "text-keep" : "text-fg-mute"}>{f.library}</span>
                      <span className="text-fg-mute"> · </span>
                      <span className="text-fg">{f.folder || "/"}</span>
                    </span>
                    <span className="text-[11px] text-fg-mute pl-3">
                      {settled(f) ? "NAS 동기화 폴더" : ""}
                    </span>
                  </label>
                ))}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

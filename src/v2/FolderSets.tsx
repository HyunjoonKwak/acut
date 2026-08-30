import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import PairView from "./PairView";

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
/** `ids` 는 folders 와 같은 순서 — 각 폴더 나무의 폴더 행 id(하위 포함) */
type FolderSet = { folders: FolderIn[]; ids: number[][]; files: number; bytes: number; pending: boolean; flagged: number };
type ApplyAll = { groups: number; kept: number; rejected: number; skipped: number };
type Outcome = { moved: number; failed: number; first_error: string | null; bytes: number; folders_removed?: number };

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
  const [sweeping, setSweeping] = useState(false);
  /// «보기» — 남길 폴더(●)와 다른 폴더 하나를 나란히
  const [viewing, setViewing] = useState<{ a: FolderIn; b: FolderIn; aIds: number[]; bIds: number[] } | null>(null);

  /// 폴더 비교로 붙인 표시를 되돌린다 — 휴지통에 가기 전이면 언제든
  const unmark = useCallback(
    async (targets: FolderSet[]) => {
      const ids = [...new Set(targets.flatMap((s) => s.ids.flat()))];
      const n = targets.reduce((a, s) => a + s.flagged, 0);
      if (ids.length === 0 || n === 0) return;
      const ok = await ask({
        title: `제외 표시 ${n.toLocaleString()}장을 되돌립니다`,
        lines: ["이 묶음 폴더들의 남김·제외 표시를 미판정으로 돌리고, 닫았던 무리는 개별 비교에 다시 나옵니다", "파일은 그대로입니다"],
        confirmLabel: "표시 지우기",
      });
      if (!ok) return;
      try {
        const [files] = await invoke<[number, number]>("cull_folder_set_unapply", { folderIds: ids });
        toast(`${files.toLocaleString()}장의 표시를 되돌렸습니다`, "ok");
      } catch (e) {
        toast(String(e), "drop");
      }
      setTick((t) => t + 1);
      onChanged();
    },
    [ask, onChanged],
  );

  /// 표시한 것을 휴지통으로 — 이 화면의 묶음 폴더들 안에서만
  const sweep = useCallback(async () => {
    if (!sets) return;
    const flagged = sets.reduce((a, s) => a + s.flagged, 0);
    if (flagged === 0) return;
    const folderIds = [...new Set(sets.flatMap((s) => s.ids.flat()))];
    const ok = await ask({
      title: `제외한 ${flagged.toLocaleString()}장을 휴지통으로 옮깁니다`,
      lines: [
        "여기 나온 묶음의 폴더 안에서 제외 표시된 사진만 — 다른 폴더는 건드리지 않습니다",
        "사진이 다 나간 폴더는 디스크에서도 지웁니다",
        "라이브러리 안 .acut/휴지통 으로 옮기는 것이라 되돌릴 수 있습니다 — 영구 삭제는 휴지통 화면에서",
      ],
      confirmLabel: "휴지통으로",
    });
    if (!ok) return;
    setSweeping(true);
    try {
      const r = await invoke<Outcome>("trash_apply", { libraryId: null, folderIds });
      const dirs = r.folders_removed ?? 0;
      toast(
        r.failed
          ? `${r.moved.toLocaleString()}장 옮김 · ${r.failed}장 실패 (${r.first_error ?? ""})`
          : `${r.moved.toLocaleString()}장을 휴지통으로 옮겼습니다 (${fmtBytes(r.bytes)})${dirs ? ` · 빈 폴더 ${dirs}개 지움` : ""} — 휴지통에서 되돌릴 수 있습니다`,
        r.failed ? "drop" : "ok",
      );
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setSweeping(false);
    }
    setTick((t) => t + 1);
    onChanged();
  }, [sets, ask, onChanged]);

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
              ? [`주의: ${risky.map((d) => d.library).join(", ")}은 NAS 동기화 폴더입니다 — 휴지통으로 옮기면 NAS에서도 지워집니다`]
              : []),
            "파일은 아직 옮기지 않습니다 — 표시한 뒤 위의 «N장 휴지통으로»를 누르면 옮깁니다",
          ],
          confirmLabel: "제외 표시",
          danger: risky.length > 0,
        });
        if (!ok) return false;
      }
      await invoke<ApplyAll>("cull_folder_set_apply", {
        keepIds: s.ids[k],
        dropIds: s.ids.flatMap((ids, j) => (j === k ? [] : ids)),
      });
      return true;
    },
    [keep, ask],
  );

  /// 여러 묶음을 골라 한 번에 — 묶음마다 체크, 위에서 «전체». 고른 묶음은 저마다의 ●(남길 폴더)로 처리
  const [pickedSets, setPickedSets] = useState<Set<string>>(new Set());
  const togglePicked = useCallback((key: string) => {
    setPickedSets((cur) => {
      const next = new Set(cur);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);
  const applyPicked = useCallback(async () => {
    if (!sets) return;
    const todo = sets.filter((s) => s.pending && pickedSets.has(setKey(s)));
    if (todo.length === 0) {
      toast("고른 묶음이 없습니다 — 줄 앞의 상자에 체크하세요");
      return;
    }
    const keepAt = (s: FolderSet) => Math.min(keep[setKey(s)] ?? 0, s.folders.length - 1);
    // 지워질 쪽에 NAS 폴더가 있는 묶음 — 사람이 봐야 한다
    const risky = todo.filter((s) => s.folders.some((f, j) => j !== keepAt(s) && settled(f)));
    const bytes = todo.reduce((a, s) => a + s.bytes * (s.folders.length - 1), 0);
    const files = todo.reduce((a, s) => a + s.files * (s.folders.length - 1), 0);
    const ok = await ask({
      title: `고른 ${todo.length.toLocaleString()}묶음 — ● 남기고 나머지 ${files.toLocaleString()}장에 제외 표시`,
      lines: [
        `${fmtBytes(bytes)} 빔`,
        ...(risky.length > 0
          ? [`주의: ${risky.length.toLocaleString()}묶음은 지워질 쪽에 NAS 동기화 폴더(내사진·공용)가 있습니다 — 휴지통으로 옮기면 NAS에서도 지워집니다`]
          : []),
        "파일은 아직 옮기지 않습니다 — 표시한 뒤 위의 «N장 휴지통으로»를 누르면 옮깁니다",
      ],
      confirmLabel: "제외 표시",
      danger: risky.length > 0,
    });
    if (!ok) return;
    let failed = 0;
    let firstErr = "";
    for (const s of todo) {
      const k = keepAt(s);
      try {
        await invoke<ApplyAll>("cull_folder_set_apply", {
          keepIds: s.ids[k],
          dropIds: s.ids.flatMap((ids, j) => (j === k ? [] : ids)),
        });
      } catch (e) {
        failed += 1;
        firstErr ||= String(e);
      }
    }
    toast(
      failed
        ? `${(todo.length - failed).toLocaleString()}묶음 표시 · ${failed}묶음 실패 (${firstErr})`
        : `${todo.length.toLocaleString()}묶음 표시했습니다 — 위의 «휴지통으로»로 옮깁니다`,
      failed ? "drop" : "ok",
    );
    setPickedSets(new Set());
    setTick((t) => t + 1);
    onChanged();
  }, [sets, pickedSets, keep, ask, onChanged]);

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
      confirmLabel: "전부 표시",
    });
    if (!ok) return;
    let failed = 0;
    let firstErr = "";
    for (const s of todo) {
      const k = keepAt(s);
      try {
        await invoke<ApplyAll>("cull_folder_set_apply", {
          keepIds: s.ids[k],
          dropIds: s.ids.flatMap((ids, j) => (j === k ? [] : ids)),
        });
      } catch (e) {
        failed += 1;
        firstErr ||= String(e);
      }
    }
    toast(
      failed
        ? `${(todo.length - failed).toLocaleString()}묶음 표시 · ${failed}묶음 실패 (${firstErr}) — 목록을 새로 읽습니다`
        : `${todo.length.toLocaleString()}묶음 표시했습니다 — 위의 «휴지통으로»로 옮깁니다`,
      failed ? "drop" : "ok",
    );
    setTick((t) => t + 1);
    onChanged();
  }, [sets, keep, ask, onChanged]);

  if (sets === null)
    return (
      <div className="h-full flex items-center justify-center gap-2 text-fg-mute">
        <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 폴더를 비교하는 중…
      </div>
    );
  const pending = sets.filter((s) => s.pending);
  const gain = pending.reduce((a, s) => a + s.bytes * (s.folders.length - 1), 0);
  const flagged = sets.reduce((a, s) => a + s.flagged, 0);
  if (sets.length === 0)
    return (
      <div className="h-full flex items-center justify-center text-fg-mute">
        내용이 완전히 같은 폴더가 없습니다 — 부분만 겹치는 것은 «개별 비교»에서
      </div>
    );

  return (
    <div className="h-full flex flex-col relative">
      {viewing && (
        <PairView
          a={viewing.a}
          b={viewing.b}
          aIds={viewing.aIds}
          bIds={viewing.bIds}
          onClose={() => {
            setViewing(null);
            setTick((t) => t + 1);
            onChanged();
          }}
        />
      )}
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 border-b border-line text-[13.5px] bar-fixed">
        <span className="text-fg-dim tabular-nums">
          완전히 같은 폴더(하위 포함) <b className="text-fg">{sets.length.toLocaleString()}묶음</b>
          {sets.length >= 5000 && <span className="text-drop"> (5,000묶음까지만 보입니다 — 처리하면 다음 묶음이 올라옵니다)</span>}
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
        {sweeping && (
          <span className="flex items-center gap-2 text-keep">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 휴지통으로 옮기는 중…
          </span>
        )}
        {flagged > 0 && (
          <>
            <button
              onClick={() => unmark(sets)}
              disabled={sweeping}
              title="여기서 붙인 남김·제외 표시를 전부 미판정으로 되돌립니다 — 파일은 그대로"
              className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[13.5px] disabled:opacity-40"
            >
              표시 지우기
            </button>
            <button
              onClick={sweep}
              disabled={sweeping}
              title="여기 나온 폴더 안에서 제외 표시한 사진을 라이브러리 안 휴지통으로 옮깁니다 — 되돌릴 수 있습니다"
              className="h-7 px-3 rounded-md bg-drop text-drop-fg font-semibold text-[13.5px] disabled:opacity-40"
            >
              {flagged.toLocaleString()}장 휴지통으로
            </button>
          </>
        )}
        {pending.length > 0 && (
          <>
            <label className="flex items-center gap-1.5 text-[13px] cursor-pointer" title="아직 안 한 묶음 전부 고르기/풀기">
              <input
                type="checkbox"
                className="accent-accent w-3.5 h-3.5"
                checked={pending.every((s) => pickedSets.has(setKey(s)))}
                onChange={(e) => setPickedSets(e.target.checked ? new Set(pending.map(setKey)) : new Set())}
              />
              전체
            </label>
            {pickedSets.size > 0 && (
              <button
                onClick={applyPicked}
                disabled={sweeping}
                title="고른 묶음마다 ● 폴더를 남기고 나머지에 제외 표시"
                className="h-7 px-3 rounded-md bg-accent text-accent-fg font-semibold text-[13.5px] disabled:opacity-40"
              >
                고른 {pickedSets.size.toLocaleString()}묶음 표시
              </button>
            )}
            <button
              onClick={applyAllNas}
              disabled={sweeping}
              title="아직 안 한 묶음 전부 — NAS 동기화 폴더(내사진·공용) 쪽을 남기고 나머지 폴더의 사진에 제외 표시"
              className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[13.5px] disabled:opacity-40"
            >
              NAS 쪽 남기고 전부
            </button>
          </>
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
              <div className="flex items-center gap-3 mb-1.5 text-[13px] text-fg-mute tabular-nums">
                {s.pending && (
                  <input
                    type="checkbox"
                    className="accent-accent w-3.5 h-3.5"
                    checked={pickedSets.has(key)}
                    onChange={() => togglePicked(key)}
                    title="이 묶음을 골라 위의 «고른 N묶음 표시»로 한 번에"
                  />
                )}
                <span>
                  {s.folders.length}곳에 같은 폴더 · {s.files.toLocaleString()}장 · 폴더당 {fmtBytes(s.bytes)}
                </span>
                {!s.pending && (
                  <span className="text-ok">
                    처리됨
                    <button
                      onClick={() => unmark([s])}
                      disabled={sweeping}
                      className="ml-2 h-6 px-2 rounded text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
                      title="이 묶음의 표시를 되돌립니다"
                    >
                      취소
                    </button>
                  </span>
                )}
                <div className="flex-1" />
                {s.pending && (
                  <button
                    onClick={async () => {
                      if (await applyOne(s)) {
                        toast("제외 표시했습니다 — 위의 «휴지통으로»로 옮깁니다", "ok");
                        setTick((t) => t + 1);
                        onChanged();
                      }
                    }}
                    title="● 남김 폴더만 남기고 나머지 폴더의 사진에 제외 표시 — 파일은 아직 그대로"
                    className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[13px]"
                  >
                    나머지 제외
                  </button>
                )}
              </div>
              <div className="grid gap-1" style={{ gridTemplateColumns: "auto 1fr auto" }}>
                {s.folders.map((f, j) => (
                  <label
                    key={f.folder_id}
                    className={`contents cursor-pointer ${s.pending ? "" : "pointer-events-none"}`}
                  >
                    <span className="flex items-center justify-center w-16 text-[12px]">
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
                    <span className="truncate text-[14px]" title={`${f.library} / ${f.folder || "/"}`}>
                      <span className={settled(f) ? "text-keep" : "text-fg-mute"}>{f.library}</span>
                      <span className="text-fg-mute"> · </span>
                      <span className="text-fg">{f.folder || "/"}</span>
                    </span>
                    <span className="text-[12px] text-fg-mute pl-3 flex items-center gap-2">
                      {settled(f) ? "NAS 동기화 폴더" : ""}
                      {j !== k && s.ids[j].length > 1 && <span className="text-fg-faint">/…</span>}
                      {j !== k && (
                        <button
                          onClick={(e) => {
                            e.preventDefault();
                            setViewing({ a: s.folders[k], b: f, aIds: s.ids[k], bIds: s.ids[j] });
                          }}
                          className="h-5 px-1.5 rounded text-[12px] text-fg-dim ring-1 ring-line-strong pointer-events-auto"
                          title="남길 폴더(●)와 이 폴더의 사진을 나란히 놓고 직접 봅니다"
                        >
                          보기
                        </button>
                      )}
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

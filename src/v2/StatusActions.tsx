import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { fmtBytes } from "./format";
import { useJob } from "./jobStore";
import { toast } from "./toastStore";
import { Kbd } from "./ui";
import { useCountUp } from "./useCountUp";
import { usePrefs } from "./prefs";
import { useView } from "./viewStore";

/**
 * 상태바 오른쪽 — 벌어지는 일과 되돌리기.
 *
 * 진행 숫자는 여기서만 구독한다. 초당 20번 오는 알림에 이 칸만 다시 그린다.
 */
export default function StatusActions({
  stopJob,
  restoreAll,
  emptyTrash,
  cleanExcluded,
  undoLast,
}: {
  stopJob: () => void;
  restoreAll: () => void;
  emptyTrash: () => void;
  cleanExcluded: () => void;
  undoLast: () => void;
}) {
  const busy = useData((s) => s.busy);
  const toClean = useData((s) => s.toClean);
  const trash = useData((s) => s.trash);
  const batches = useData((s) => s.batches);
  const stats = useData((s) => s.stats);
  const nasNew = useData((s) => s.nasNew);
  const viewTrash = useView((s) => s.viewTrash);
  const noThumb = useView((s) => s.picks.no_thumb);
  const hasJob = useJob((s) => s.job !== null);
  const libId = usePrefs((s) => s.libId);
  const libName = useData((s) => s.libs.find((l) => l.id === libId)?.name ?? null);
  const undoable = batches.find((b) => b.undone_at === null);

  return (
    <>
      {busy && <span className="text-keep">{busy}</span>}
      {hasJob && (
        <>
          <Progress />
          <button
            onClick={stopJob}
            title="지금까지 한 것은 저장됩니다"
            className="h-5 px-2 rounded text-drop ring-1 ring-drop/40 hover:bg-drop/10"
          >
            멈추기
          </button>
        </>
      )}
      {!hasJob && viewTrash && (trash?.files ?? 0) > 0 && (
        <>
          <button
            onClick={restoreAll}
            className="h-5 px-2 rounded text-fg-dim ring-1 ring-line-strong hover:bg-hover"
          >
            전부 되돌리기
          </button>
          <button
            onClick={emptyTrash}
            className="h-5 px-2 rounded text-drop ring-1 ring-drop/40 hover:bg-drop/10"
          >
            영구히 비우기
          </button>
        </>
      )}
      {!hasJob && !viewTrash && (toClean?.files ?? 0) > 0 && (
        <button
          onClick={cleanExcluded}
          title={`${libName ?? "모든 라이브러리"}에서 제외 표시한 ${toClean?.files.toLocaleString()}장(${fmtBytes(toClean?.bytes ?? 0)})을 라이브러리 안 휴지통으로 옮깁니다 — 되돌릴 수 있습니다`}
          className="h-5 px-2 rounded bg-keep text-keep-fg font-semibold"
        >
          {libName ?? "전체"}에서 제외한 {toClean?.files.toLocaleString()}장 휴지통으로
        </button>
      )}
      {!hasJob && nasNew && (
        <button
          onClick={async () => {
            try {
              await invoke("nas_pull_start", { libraryId: nasNew.libraryId });
              useData.getState().setNasNew(null);
            } catch (e) {
              toast(String(e), "drop");
            }
          }}
          title={`NAS 1차 구역에 받은 적 없는 사진 ${nasNew.files.toLocaleString()}장 · ${fmtBytes(nasNew.bytes)}. 누르면 작업대로 내려받습니다.`}
          className="h-5 px-2 rounded bg-accent text-accent-fg font-semibold"
        >
          NAS 새 사진 {nasNew.files.toLocaleString()}장 받기
        </button>
      )}
      {undoable && (
        <button
          onClick={undoLast}
          title={`가장 최근 작업을 물립니다: ${undoable.label ?? ""} · ${undoable.item_count.toLocaleString()}장`}
          className="hover:text-fg"
        >
          ↩ 되돌리기: {undoable.label ?? "최근 작업"} ({undoable.item_count.toLocaleString()}장) <Kbd>⌘Z</Kbd>
        </button>
      )}
      {stats && stats.thumbs_pending > 0 && !hasJob && (
        <button
          onClick={() => useView.getState().patchPicks({ no_thumb: !noThumb })}
          title={
            noThumb
              ? "누르면 다시 전체를 봅니다"
              : "썸네일을 못 만들었거나 아직 안 만든 사진입니다. 누르면 그것만 봅니다. 다시 스캔하면 다시 만들어 봅니다."
          }
          className={
            noThumb
              ? "px-1.5 rounded bg-accent text-accent-fg font-semibold"
              : "text-fg-mute hover:text-fg"
          }
        >
          {noThumb
            ? `썸네일 없음 ${stats.thumbs_pending.toLocaleString()}장만 보는 중 ✕`
            : `썸네일 없음 ${stats.thumbs_pending.toLocaleString()}장`}
        </button>
      )}
    </>
  );
}

/// 진행 표시 — 숫자가 한 칸씩 올라간다.
function Progress() {
  const job = useJob((s) => s.job);
  const n = useCountUp(job?.done ?? 0);
  if (!job) return null;
  return (
    <span className="text-keep tabular-nums whitespace-nowrap">
      {job.label} {n.toLocaleString()}
      {job.total > 0 && ` / ${job.total.toLocaleString()}`}
    </span>
  );
}

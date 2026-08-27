import { useData } from "./dataStore";
import { fmtBytes } from "./format";
import { useJob } from "./jobStore";
import { Kbd } from "./ui";
import { useCountUp } from "./useCountUp";
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
  const batches = useData((s) => s.batches);
  const stats = useData((s) => s.stats);
  const viewTrash = useView((s) => s.viewTrash);
  const hasJob = useJob((s) => s.job !== null);
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
      {!hasJob && viewTrash && (
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
          title={`제외 ${toClean?.files.toLocaleString()}장 · ${fmtBytes(toClean?.bytes ?? 0)} 확보`}
          className="h-5 px-2 rounded bg-keep text-keep-fg font-semibold"
        >
          제외 {toClean?.files.toLocaleString()}장 치우기
        </button>
      )}
      {undoable && (
        <button
          onClick={undoLast}
          title={`되돌리기: ${undoable.label ?? ""}`}
          className="hover:text-fg"
        >
          ↩ 되돌리기 <Kbd>⌘Z</Kbd>
        </button>
      )}
      {stats && stats.thumbs_pending > 0 && (
        <span className="text-keep">
          썸네일 대기 {stats.thumbs_pending.toLocaleString()}
        </span>
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

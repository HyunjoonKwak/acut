import { useEffect } from "react";
import { useShallow } from "zustand/react/shallow";
import Viewer from "./Viewer";
import { useUi } from "./uiStore";

/**
 * 크게 보기의 자리 — 뷰어 상태(viewerAt·viewerFull)는 여기서만 구독한다.
 *
 * App 이 구독하면 화살표 한 번마다 App → 격자 → 타일 전부가 다시 그려진다 (리뷰 H17).
 * 여기서 구독하면 화살표는 이 작은 조각만 다시 그린다.
 */
export default function ViewerHost({
  ids,
  onNearEnd,
  onMark,
  kindOf,
  onRename,
}: {
  ids: number[];
  /** 끝에 다다랐다 — 다음 쪽을 미리 읽게 */
  onNearEnd: () => void;
  onMark: (
    id: number,
    patch: { rating?: number; cullingFlag?: number; favorite?: boolean },
  ) => Promise<void>;
  kindOf?: (id: number) => number | undefined;
  onRename?: (id: number, name: string) => Promise<string>;
}) {
  const { at, full, set } = useUi(
    useShallow((s) => ({ at: s.viewerAt, full: s.viewerFull, set: s.set })),
  );
  const total = ids.length;
  // 뷰어가 끝에 다다르면 다음 쪽을 미리 읽는다
  useEffect(() => {
    if (at !== null && at >= total - 5) onNearEnd();
  }, [at, total, onNearEnd]);
  // 목록이 줄어 뷰어의 순번이 밖으로 나가면 마지막 장으로 당긴다 — 휴지통에
  // 보내거나 다시 읽어 목록이 짧아졌을 때. 비면 닫는다.
  useEffect(() => {
    const cur = useUi.getState().viewerAt;
    if (cur === null || cur < total) return;
    set(
      total === 0
        ? { viewerAt: null, viewerFull: false }
        : { viewerAt: total - 1 },
    );
  }, [total, set]);

  if (at === null || at >= ids.length) return null;
  return (
    <Viewer
      ids={ids}
      index={at}
      onIndex={(i) => set({ viewerAt: i })}
      onClose={() => set({ viewerAt: null, viewerFull: false })}
      onMark={onMark}
      fullScreen={full}
      onToggleFullScreen={() => set({ viewerFull: !full })}
      kindOf={kindOf}
      onRename={onRename}
    />
  );
}

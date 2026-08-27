import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GroupBy } from "./groupItems";
import type { Cursor, FileRow, Mark, Page } from "./types";
import type { Filter } from "./viewStore";
import { PAGE } from "./types";

/**
 * 사진 목록 — keyset 커서로 한 쪽씩.
 *
 * 요청은 한 번에 하나만 돈다(`inflight`). 스크롤이 빠르면 같은 쪽을 두 번
 * 부를 수 있고, 스크롤바를 끌면 초당 수십 번 쏟아진다. 도는 동안 들어온
 * 스크롤바 요청은 `pending`에 덮어써 두고 끝나면 **마지막 것만** 잇는다 —
 * 큐에 쌓으면 손을 뗀 뒤에도 한참 따라온다.
 */
export function usePhotoList(
  filter: Filter,
  group: GroupBy,
  opts: {
    /** 아직 라이브러리가 없으면 읽지 않는다 */
    enabled: boolean;
    /** 조건이 바뀌어 첫 쪽을 새로 읽을 때 같이 부를 것 (통계 갱신) */
    onReload?: () => void;
    /** 스크롤바로 자리를 옮겼을 때 — 스크롤을 맨 위로 되돌린다 */
    onSeek?: () => void;
  },
) {
  const [rows, setRows] = useState<FileRow[]>([]);
  const [cursor, setCursor] = useState<Cursor | null>(null);
  const [done, setDone] = useState(false);
  const [loading, setLoading] = useState(false);
  /// rows[0]이 전체에서 몇 번째인가. 스크롤바 손잡이 위치의 기준이다.
  const [baseIndex, setBaseIndex] = useState(0);

  const inflight = useRef(false);
  const pending = useRef<number | null>(null);
  const drain = useRef<() => void>(() => {});
  // 콜백은 ref로 본다 — 의존성에 넣으면 부르는 쪽이 바뀔 때마다 목록을 다시 읽는다
  const cb = useRef(opts);
  useEffect(() => {
    cb.current = opts;
  });

  /// 어떤 경로로 끝나든 여기서만 잠금을 푼다. 안 그러면 밀린 요청이 사라진다.
  const release = useCallback(() => {
    inflight.current = false;
    drain.current();
  }, []);

  const loadFirst = useCallback(async () => {
    if (inflight.current) return;
    inflight.current = true;
    setLoading(true);
    try {
      const p = await invoke<Page>("files_page", {
        filter,
        cursor: null,
        limit: PAGE,
        group,
      });
      setRows(p.rows);
      setCursor(p.next);
      setDone(!p.next);
      setBaseIndex(0);
    } finally {
      setLoading(false);
      release();
    }
  }, [filter, group, release]);

  const loadMore = useCallback(async () => {
    if (inflight.current || done || !cursor) return;
    inflight.current = true;
    try {
      const p = await invoke<Page>("files_page", {
        filter,
        cursor,
        limit: PAGE,
        group,
      });
      setRows((prev) => [...prev, ...p.rows]);
      setCursor(p.next);
      setDone(!p.next);
    } finally {
      release();
    }
  }, [filter, group, cursor, done, release]);

  /// 스크롤바가 준 전역 순번으로 목록을 다시 읽는다.
  const seekTo = useCallback(
    async (index: number) => {
      pending.current = index;
      if (inflight.current) return; // 도는 쪽이 끝나면서 release()로 이어받는다
      inflight.current = true;
      try {
        while (pending.current !== null) {
          const want = pending.current;
          pending.current = null;
          const c = await invoke<Cursor | null>("files_cursor_at", {
            filter,
            index: want,
          });
          const p = await invoke<Page>("files_page", {
            filter,
            cursor: c,
            limit: PAGE,
          });
          setRows(p.rows);
          setCursor(p.next);
          setDone(!p.next);
          setBaseIndex(want);
          cb.current.onSeek?.();
        }
      } finally {
        inflight.current = false;
      }
    },
    [filter],
  );

  useEffect(() => {
    drain.current = () => {
      if (pending.current !== null) void seekTo(pending.current);
    };
  }, [seekTo]);

  // 조건이 바뀌면 처음부터. loadFirst는 filter·group이 바뀔 때만 새로 만들어진다.
  useEffect(() => {
    if (!opts.enabled) return;
    setRows([]);
    setCursor(null);
    setDone(false);
    loadFirst();
    cb.current.onReload?.();
  }, [opts.enabled, loadFirst]);

  /// 판정을 바꾼다 — 서버에 쓰고 목록도 그 자리에서 고친다. 뷰어에서 바꾸면
  /// 그리드도 같이 바뀌어야 한다.
  const markOne = useCallback(async (id: number, patch: Mark) => {
    await invoke("files_mark", {
      ids: [id],
      rating: patch.rating ?? null,
      cullingFlag: patch.cullingFlag ?? null,
      favorite: patch.favorite ?? null,
    });
    setRows((prev) =>
      prev.map((r) =>
        r.id === id
          ? {
              ...r,
              rating: patch.rating ?? r.rating,
              culling_flag: patch.cullingFlag ?? r.culling_flag,
              favorite: patch.favorite ?? r.favorite,
            }
          : r,
      ),
    );
  }, []);

  return { rows, loading, baseIndex, loadFirst, loadMore, seekTo, markOne };
}

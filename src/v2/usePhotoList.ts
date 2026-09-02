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
  const [error, setError] = useState<string | null>(null);
  /// 첫 쪽이 실제로 도착했나. `loading` 은 처음에 `false` 라 그것만 보면 «아직 아무것도
  /// 안 불렀다»와 «다 불렀다»가 같아 보인다 — 시작 시간을 그 값으로 재던 때는 첫
  /// 그리드가 라이브러리 목록이 온 순간으로 기록됐다 (실측 2026-09-01).
  const [loaded, setLoaded] = useState(false);
  /// rows[0]이 전체에서 몇 번째인가. 스크롤바 손잡이 위치의 기준이다.
  const [baseIndex, setBaseIndex] = useState(0);

  /// 지금 도는 요청의 표. 0이면 없다. 끝난 요청은 자기 표일 때만 자리를 비운다 —
  /// 뒤에 시작한 요청의 자리를 앞 요청이 비우면 두 개가 같이 돈다.
  const inflight = useRef(0);
  const seq = useRef(0);
  /// 조건 세대. 첫 쪽을 새로 읽을 때마다 오른다. 낡은 세대의 응답은 버린다 —
  /// 실측: 앞 폴더를 읽는 중에 다른 폴더를 누르면 새 폴더 요청이 «도는 중»이라
  /// 건너뛰어지고, 앞 폴더의 응답이 새 폴더 제목 아래 그려졌다.
  const gen = useRef(0);
  const pending = useRef<number | null>(null);
  const drain = useRef<() => void>(() => {});
  // 콜백은 ref로 본다 — 의존성에 넣으면 부르는 쪽이 바뀔 때마다 목록을 다시 읽는다
  const cb = useRef(opts);
  useEffect(() => {
    cb.current = opts;
  });

  /// 요청 하나의 자리를 잡는다. 돌려준 함수로만 그 자리를 비운다.
  const take = useCallback(() => {
    const token = ++seq.current;
    inflight.current = token;
    return () => {
      if (inflight.current !== token) return;
      inflight.current = 0;
      drain.current();
    };
  }, []);

  const loadFirst = useCallback(async () => {
    // 도는 요청이 있어도 기다리지 않는다 — 조건이 바뀌었으니 그 응답은 버릴 것이다.
    // 옛 조건의 스크롤 요청도 같이 버린다 — 새 조건 목록에 옛 순번(«6001/40»)이 붙던 길
    const g = ++gen.current;
    pending.current = null;
    const free = take();
    setLoading(true);
    setError(null);
    try {
      const p = await invoke<Page>("files_page", {
        filter,
        cursor: null,
        limit: PAGE,
        group,
      });
      if (g !== gen.current) return;
      setRows(p.rows);
      setCursor(p.next);
      setDone(!p.next);
      setBaseIndex(0);
    } catch (e) {
      if (g === gen.current) setError(String(e));
    } finally {
      if (g === gen.current) {
        setLoading(false);
        setLoaded(true);
      }
      free();
    }
  }, [filter, group, take]);

  const loadMore = useCallback(async () => {
    if (inflight.current || done || !cursor) return;
    const g = gen.current;
    const free = take();
    try {
      const p = await invoke<Page>("files_page", {
        filter,
        cursor,
        limit: PAGE,
        group,
      });
      if (g !== gen.current) return;
      setRows((prev) => [...prev, ...p.rows]);
      setCursor(p.next);
      setDone(!p.next);
      setError(null);
    } catch (e) {
      if (g === gen.current) setError(String(e));
    } finally {
      free();
    }
  }, [filter, group, cursor, done, take]);

  /// 스크롤바가 준 전역 순번으로 목록을 다시 읽는다.
  const seekTo = useCallback(
    async (index: number) => {
      pending.current = index;
      if (inflight.current) return; // 도는 쪽이 끝나면서 drain으로 이어받는다
      const g = gen.current;
      const free = take();
      try {
        while (pending.current !== null) {
          const want = pending.current;
          pending.current = null;
          const c = await invoke<Cursor | null>("files_cursor_at", {
            filter,
            index: want,
          });
          if (g !== gen.current) return;
          // group 을 같이 넘긴다 — 빼면 스크롤바로 옮긴 자리부터 묶기 머리글이 사라진다
          const p = await invoke<Page>("files_page", {
            filter,
            cursor: c,
            limit: PAGE,
            group,
          });
          if (g !== gen.current) return;
          setRows(p.rows);
          setCursor(p.next);
          setDone(!p.next);
          setBaseIndex(want);
          setError(null);
          cb.current.onSeek?.();
        }
      } catch (e) {
        if (g === gen.current) setError(String(e));
      } finally {
        free();
      }
    },
    [filter, group, take],
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
    setLoaded(false);
    setError(null);
    void loadFirst();
    cb.current.onReload?.();
  }, [opts.enabled, loadFirst]);

  /// 한 줄을 그 자리에서 고친다 — 다시 읽지 않는다. 이름·판정이 바뀌었을 때.
  const patchRow = useCallback((id: number, patch: Partial<FileRow>) => {
    setRows((prev) => prev.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  }, []);

  /// 여러 줄을 한 번에 고친다 — 한 장씩 고치면 장수만큼 목록을 다시 훑는다
  const patchRows = useCallback((ids: number[], patch: Partial<FileRow>) => {
    const set = new Set(ids);
    setRows((prev) => prev.map((r) => (set.has(r.id) ? { ...r, ...patch } : r)));
  }, []);

  /// 여러 장의 판정을 한 번에 — IPC 한 번, 트랜잭션 하나, 목록 한 번.
  /// 실측: 5,000장을 한 장씩 보내면 창이 수십 초 멈췄다 (리뷰 H2)
  const markMany = useCallback(
    async (ids: number[], patch: Mark) => {
      if (ids.length === 0) return;
      await invoke("files_mark", {
        ids,
        rating: patch.rating ?? null,
        cullingFlag: patch.cullingFlag ?? null,
        favorite: patch.favorite ?? null,
      });
      patchRows(ids, {
        ...(patch.rating !== undefined ? { rating: patch.rating } : {}),
        ...(patch.cullingFlag !== undefined
          ? { culling_flag: patch.cullingFlag }
          : {}),
        ...(patch.favorite !== undefined ? { favorite: patch.favorite } : {}),
      });
    },
    [patchRows],
  );

  /// 판정을 바꾼다 — 서버에 쓰고 목록도 그 자리에서 고친다. 뷰어에서 바꾸면
  /// 그리드도 같이 바뀌어야 한다.
  const markOne = useCallback(
    async (id: number, patch: Mark) => {
      await invoke("files_mark", {
        ids: [id],
        rating: patch.rating ?? null,
        cullingFlag: patch.cullingFlag ?? null,
        favorite: patch.favorite ?? null,
      });
      patchRow(id, {
        ...(patch.rating !== undefined ? { rating: patch.rating } : {}),
        ...(patch.cullingFlag !== undefined
          ? { culling_flag: patch.cullingFlag }
          : {}),
        ...(patch.favorite !== undefined ? { favorite: patch.favorite } : {}),
      });
    },
    [patchRow],
  );

  return {
    rows,
    loading,
    error,
    loaded,
    baseIndex,
    loadFirst,
    loadMore,
    seekTo,
    markOne,
    markMany,
    patchRow,
    patchRows,
  };
}

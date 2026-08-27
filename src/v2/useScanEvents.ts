import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useData } from "./dataStore";
import { useJob } from "./jobStore";
import { usePrefs } from "./prefs";
import { toast } from "./toastStore";
import { useView } from "./viewStore";
import type { Library } from "./types";

/**
 * 스캔·썸네일의 시작과 진행.
 *
 * 이벤트 리스너는 **한 번만** 건다. 콜백은 ref로 본다 — 의존성에 넣으면
 * 조건이 바뀔 때마다 리스너를 뗐다 붙이는데, listen()이 비동기라 떼기
 * 전에 새것이 붙어 두 벌이 된다.
 */
export function useScanEvents(cb: {
  /** 목록을 처음부터 다시 읽는다 */
  reload: () => void;
  /** 통계·눈금을 다시 읽는다 */
  refreshMeta: () => void;
}) {
  const ref = useRef(cb);
  useEffect(() => {
    ref.current = cb;
  });
  /// 아직 스캔하지 않은 라이브러리들. 하나가 끝나면 다음을 시작한다.
  const queue = useRef<number[]>([]);

  useEffect(() => {
    const data = useData.getState;
    const job = useJob.getState;
    const subs: Promise<() => void>[] = [];
    let alive = true;
    const on = <T>(name: string, f: (payload: T) => void) => {
      subs.push(listen<T>(name, (e) => alive && f(e.payload)));
    };

    on<{ found: number; inserted: number; skipped: number }>(
      "scan-progress",
      (p) => {
        data().setScanMsg("");
        job().progress({
          label: "스캔",
          done: p.inserted + p.skipped,
          total: p.found,
        });
      },
    );
    on("scan-done", () => {
      data().setScanMsg("스캔 완료 — 썸네일 만드는 중");
      job().clear();
      ref.current.reload();
      ref.current.refreshMeta();
    });
    on<{ done: number; total: number }>("thumb-progress", (p) => {
      data().setScanMsg("");
      job().progress({ label: "썸네일", done: p.done, total: p.total });
    });
    on<{ done: number; total: number }>("upgrade-progress", (p) => {
      data().setScanMsg("");
      job().progress({
        label: "화질 올리는 중 — 그냥 쓰셔도 됩니다",
        done: p.done,
        total: p.total,
      });
    });
    on("upgrade-done", () => {
      data().setScanMsg("");
      job().clear();
      ref.current.reload();
      ref.current.refreshMeta();
      data().refreshCache();
    });
    on("thumb-done", () => {
      job().clear();
      ref.current.reload();
      ref.current.refreshMeta();
      data().refreshLibs();
      data().refreshCache();
      const next = queue.current.shift();
      if (next === undefined) {
        data().setScanMsg("");
      } else {
        data().setScanMsg("다음 라이브러리 스캔…");
        invoke("scan_start", { libraryId: next }).catch((e) =>
          data().setScanMsg(String(e)),
        );
      }
    });
    on<string>("scan-error", (m) => data().setScanMsg(`스캔 실패: ${m}`));
    // 폴더 감시 — 파인더로 넣거나 지운 것이 반영됐다
    on<{
      library_id: number;
      inserted: number;
      updated: number;
      removed: number;
    }>("library-changed", (c) => {
      ref.current.reload();
      ref.current.refreshMeta();
      data().refreshLibs();
      data().loadFolders();
      const parts = [
        c.inserted > 0 ? `${c.inserted.toLocaleString()}장 들어옴` : "",
        c.removed > 0 ? `${c.removed.toLocaleString()}장 사라짐` : "",
      ].filter(Boolean);
      if (parts.length) toast(`폴더가 바뀌었습니다 — ${parts.join(" · ")}`);
    });

    return () => {
      alive = false;
      // listen()이 아직 안 끝났어도 끝난 뒤에 뗀다
      subs.forEach((p) => p.then((f) => f()));
    };
  }, []);

  /// 고른 라이브러리를 다시 훑는다. 여럿이면 연결된 것을 하나씩 차례로 —
  /// 예전에는 libs[0]만 훑어서 두 번째 라이브러리는 아무리 눌러도 썸네일이
  /// 생기지 않았다.
  const rescan = useCallback(async (ids: number[]) => {
    const { libs, setScanMsg } = useData.getState();
    const targets = ids.filter((id) => libs.find((l) => l.id === id)?.online);
    if (targets.length === 0) {
      setScanMsg("연결된 디스크가 없습니다");
      return;
    }
    queue.current = targets.slice(1);
    try {
      await invoke("scan_start", { libraryId: targets[0] });
      setScanMsg("스캔 시작…");
    } catch (e) {
      setScanMsg(String(e));
    }
  }, []);

  /// 도는 일을 멈춘다. 진행은 500장마다 저장돼 있어 지금까지 한 것은 남는다.
  const stopJob = useCallback(async () => {
    await invoke("scan_cancel");
    queue.current = [];
    useJob.getState().clear();
    useData
      .getState()
      .setScanMsg("멈췄습니다 — 지금까지 한 것은 저장돼 있습니다");
    ref.current.refreshMeta();
    await useData.getState().refreshLibs();
  }, []);

  const addLibrary = useCallback(async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    const { refreshLibs, setOpen, setScanMsg } = useData.getState();
    try {
      const l = await invoke<Library>("library_add", { path: picked, area: 1 });
      await refreshLibs();
      usePrefs.getState().set("libId", l.id);
      useView.getState().setSel(null);
      setOpen(new Set());
      // rescan()을 쓰지 않는다 — 방금 등록한 것이 아직 libs 상태에 없어
      // «연결된 디스크가 없습니다»로 걸린다. 어차피 방금 고른 폴더다.
      queue.current = [];
      await invoke("scan_start", { libraryId: l.id });
      setScanMsg("스캔 시작…");
    } catch (e) {
      setScanMsg(String(e));
    }
  }, []);

  return { rescan, stopJob, addLibrary };
}

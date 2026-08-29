import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useData } from "./dataStore";
import { useUi } from "./uiStore";
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
        // 아직 한 장도 처리 전이면 폴더를 훑는 중이다 — 찾은 수만 올라간다
        if (p.inserted + p.skipped === 0)
          job().progress({ label: "스캔 중", done: p.found, total: 0 });
        else
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
    // 고르기 — 잡동사니·같은 순간은 순식간, 완전 중복의 전체 해시가 오래 걸린다.
    // 고르기 화면을 닫아도 상태바에 남아 «아직 도는 중»을 알린다.
    on("cull-junk", () =>
      job().progress({ label: "고르기 — 같은 순간 찾는 중", done: 0, total: 0 }),
    );
    on("cull-burst", () =>
      job().progress({ label: "고르기 — 중복 후보 찾는 중", done: 0, total: 0 }),
    );
    on<{
      phase: string;
      hashed: number;
      candidates: number;
      full_done: number;
      full_total: number;
    }>("cull-dedup-progress", (p) =>
      job().progress(
        p.phase === "full"
          ? { label: "고르기 — 전체 해시", done: p.full_done, total: p.full_total }
          : { label: "고르기 — 빠른 해시", done: p.hashed, total: p.candidates },
      ),
    );
    on("cull-dedup", () =>
      job().progress({ label: "고르기 — 비슷한 장면 찾는 중", done: 0, total: 0 }),
    );
    on("cull-done", () => job().clear());
    on("cull-error", () => job().clear());
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

    // AI — 모델 받기와 벡터 만들기. 스캔과 같은 자리에 진행이 뜬다.
    on<{ got: number; total: number }>("ai-download", (p) =>
      job().progress({
        label: "모델 받는 중 (MB)",
        done: Math.round(p.got / 1e6),
        total: Math.round(p.total / 1e6),
      }),
    );
    on<string | null>("ai-download-done", (e) => {
      job().clear();
      if (e) toast(`모델 받기 실패 — ${e}`, "drop");
      else toast("모델을 받았습니다. 이제 벡터를 만들 수 있습니다", "ok");
    });
    on<{ done: number; total: number; failed: number }>("ai-progress", (p) =>
      job().progress({ label: "AI 벡터", done: p.done, total: p.total }),
    );
    on<{ done: number; total: number; failed: number }>("ai-done", (p) => {
      job().clear();
      toast(
        p.failed > 0
          ? `벡터 ${p.done - p.failed}장 · 실패 ${p.failed}장`
          : `벡터 ${p.done.toLocaleString()}장을 만들었습니다`,
        p.failed > 0 ? "drop" : "ok",
      );
    });
    on<{ done: number; total: number; faces: number }>("faces-progress", (p) =>
      job().progress({ label: "얼굴 찾기", done: p.done, total: p.total }),
    );
    on<{ done: number; faces: number; persons: number }>("faces-done", (p) => {
      job().clear();
      toast(
        `${p.done.toLocaleString()}장에서 얼굴 ${p.faces.toLocaleString()}개 — ${p.persons.toLocaleString()}명으로 묶었습니다`,
        "ok",
      );
    });
    on<{ done: number; total: number }>("backup-progress", (p) =>
      job().progress({ label: "백업", done: p.done, total: p.total }),
    );
    on<{
      copied: number;
      updated: number;
      bytes: number;
      errors: number;
      cancelled: boolean;
    }>("backup-done", (r) => {
      job().clear();
      const n = r.copied + r.updated;
      toast(
        r.cancelled
          ? `백업 멈춤 — ${n.toLocaleString()}장까지 복사했습니다`
          : r.errors > 0
            ? `백업 끝 — ${n.toLocaleString()}장 복사, ${r.errors}건 문제 (로그 참고)`
            : `백업 끝 — ${n.toLocaleString()}장 복사했습니다`,
        r.errors > 0 ? "drop" : "ok",
      );
    });
    on<{ done: number; total: number }>("offload-progress", (p) =>
      job().progress({ label: "옮기는 중", done: p.done, total: p.total }),
    );
    on<{ folders: number; files: number; bytes: number }>(
      "offload-done",
      async (o) => {
        job().clear();
        toast(
          `${o.files.toLocaleString()}장(${o.folders}개 폴더)을 옮겼습니다`,
          "ok",
        );
        await useData.getState().refreshLibs();
        ref.current.refreshMeta();
      },
    );
    on<string>("offload-error", (e) => {
      job().clear();
      toast(`옮기기 실패 — ${e}`, "drop");
    });
    // NAS — 내려받기·비우기·XMP
    on<{ done: number; total: number; percent: number }>(
      "nas-pull-progress",
      (p) =>
        job().progress({
          // 큰 영상 하나를 받는 동안엔 장수가 안 움직인다 — 용량 %가 살아 있음을 보인다
          label: `NAS 내려받는 중 ${p.percent}%`,
          done: p.total > 0 ? p.done : p.percent,
          total: p.total > 0 ? p.total : 0,
        }),
    );
    on<{ library_id: number; files: number; cancelled: boolean }>(
      "nas-pull-done",
      (p) => {
        job().clear();
        useData.getState().setNasNew(null);
        toast(
          p.cancelled
            ? `내려받기 멈춤 — ${p.files.toLocaleString()}개까지 받았습니다`
            : `NAS 1차 구역에서 ${p.files.toLocaleString()}개를 받았습니다 — 스캔합니다`,
          "ok",
        );
        // 방금 받은 것이 목록에 보이게 — 그 라이브러리만 스캔
        queue.current = [];
        invoke("scan_start", { libraryId: p.library_id }).catch((e) =>
          toast(String(e), "drop"),
        );
      },
    );
    on<{ moved: number; bytes: number }>("nas-purge-done", (p) =>
      toast(
        `1차 구역에서 ${p.moved.toLocaleString()}개를 #trash로 옮겼습니다`,
        "ok",
      ),
    );
    on<{ done: number; total: number }>("xmp-progress", (p) =>
      job().progress({ label: "XMP 사이드카", done: p.done, total: p.total }),
    );
    on<{ written: number; skipped: number; failed: number }>(
      "xmp-done",
      (x) => {
        job().clear();
        toast(
          `XMP ${x.written.toLocaleString()}개 씀${x.skipped ? ` · ${x.skipped}개 건너뜀` : ""}${x.failed ? ` · ${x.failed}개 실패` : ""}`,
          x.failed ? "drop" : "ok",
        );
      },
    );
    on<{ done: number; total: number }>("video-dates-progress", (p) =>
      job().progress({ label: "영상 촬영일", done: p.done, total: p.total }),
    );
    on<{ checked: number; fixed: number }>("video-dates-done", (r) => {
      job().clear();
      toast(
        `영상 ${r.checked.toLocaleString()}개 확인 — ${r.fixed.toLocaleString()}개의 촬영일을 고쳤습니다`,
        "ok",
      );
      ref.current.reload();
    });
    on<string>("nas-error", (e) => {
      job().clear();
      toast(`NAS — ${e}`, "drop");
    });
    on<string>("ai-error", (e) => {
      job().clear();
      toast(`AI 실패 — ${e}`, "drop");
    });
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
      // «이미 스캔 중입니다» 같은 것 — 메뉴 안에 숨기면 눌러도 반응이 없어 보인다
      toast(String(e), "drop");
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

  /// 폴더를 고른 뒤 영역을 묻는다 — 등록은 registerLibrary가
  const addLibrary = useCallback(async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    useUi.getState().set({ areaPick: picked });
  }, []);

  const registerLibrary = useCallback(async (picked: string, area: number) => {
    const { refreshLibs, setOpen, setScanMsg } = useData.getState();
    try {
      const l = await invoke<Library>("library_add", { path: picked, area });
      await refreshLibs();
      usePrefs.getState().set("libId", l.id);
      useView.getState().setSel(null);
      setOpen(new Set());
      // rescan()을 쓰지 않는다 — 방금 등록한 것이 아직 libs 상태에 없어
      // «연결된 디스크가 없습니다»로 걸린다. 어차피 방금 고른 폴더다.
      queue.current = [];
      await invoke("scan_start", { libraryId: l.id });
      setScanMsg("스캔 시작…");
      toast(`「${l.name}」 등록 — 스캔을 시작합니다`, "ok");
    } catch (e) {
      toast(String(e), "drop");
      setScanMsg(String(e));
    }
  }, []);

  return { rescan, stopJob, addLibrary, registerLibrary };
}

import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "./confirmContext";
import { useData } from "./dataStore";
import { fmtBytes } from "./format";
import { toast } from "./toastStore";
import { usePrefs } from "./prefs";
import type { Library, Outcome } from "./types";
import { useView } from "./viewStore";
import { isUndoableBatchKind } from "./undoLabel";

/**
 * 파일을 실제로 움직이는 일들 — 휴지통, 되돌리기.
 *
 * 되돌릴 수 없는 일은 먼저 묻고, 끝나면 목록·통계·라이브러리를 새로 읽는다.
 */
export function useOps(cb: {
  reload: () => Promise<void> | void;
  refreshMeta: () => void;
}) {
  const ask = useConfirm();
  const { reload, refreshMeta } = cb;

  /// 끝나고 나서 다시 읽을 것들
  const after = useCallback(async () => {
    const d = useData.getState();
    await Promise.all([
      reload(),
      refreshMeta(),
      d.refreshLibs(),
      d.refreshBatches(),
    ]);
  }, [reload, refreshMeta]);

  const runTrashOp = useCallback(
    async (cmd: string, args: Record<string, unknown>, doing: string) => {
      const { setBusy } = useData.getState();
      setBusy(doing);
      let r: Outcome;
      try {
        r = await invoke<Outcome>(cmd, args);
      } catch (e) {
        setBusy("");
        toast(String(e), "drop");
        return false;
      }
      setBusy("");
      if (r.failed > 0)
        toast(
          `${r.moved}장 처리 · ${r.failed}장 실패 — ${r.first_error ?? ""}`,
          "drop",
        );
      else if (r.moved > 0)
        toast(`${r.moved.toLocaleString()}장 처리했습니다`);
      // 할 일이 없었던 것도 말한다 — 조용히 끝나면 «안 되는 것 같아»가 된다
      else toast(r.first_error ?? "처리할 것이 없습니다");

      try {
        await after();
        await useData.getState().refreshTrash(usePrefs.getState().libId);
      } catch (e) {
        // 파일 작업은 이미 끝났다. 뒤의 조회 실패를 작업 실패로 돌려주면
        // 사용자가 같은 파일을 다시 처리하게 되므로 둘을 구분한다.
        toast(`처리는 끝났지만 화면을 새로 읽지 못했습니다 — ${String(e)}`, "drop");
      }
      // 부분 실패도 완료로 돌려주면 호출부가 선택을 전부 지워 재시도할
      // 대상을 잃는다. 모두 처리됐을 때만 선택을 풀게 한다.
      return r.failed === 0;
    },
    [after],
  );

  /// 고른 것들을 휴지통으로. 되돌릴 수 있다.
  const trashFiles = useCallback(
    async (ids: number[]) => {
      const ok = await ask({
        title: `${ids.length.toLocaleString()}장을 휴지통으로 옮깁니다`,
        lines: ["· 되돌릴 수 있습니다"],
        confirmLabel: "옮기기",
      });
      if (!ok) return false;
      return runTrashOp("trash_files", { ids }, "휴지통으로 옮기는 중…");
    },
    [ask, runTrashOp],
  );

  /// 제외로 판정한 것을 휴지통으로. 파일은 라이브러리 안 `.acut/휴지통`으로
  /// 옮겨질 뿐이라 되돌릴 수 있다.
  /// `scope` 를 주면 그 라이브러리(null 이면 전부)의 제외 표시를 — 고르기 머리의 단추가 쓴다.
  /// 안 주면 지금 보는 라이브러리(상태바)
  const cleanExcluded = useCallback(async (scope?: number | null) => {
    const libId = scope === undefined ? usePrefs.getState().libId : scope;
    const toClean = scope === undefined ? useData.getState().toClean : useData.getState().toCleanAll;
    if (!toClean || toClean.files === 0) return;
    const libName = useData.getState().libs.find((l) => l.id === libId)?.name ?? "모든 라이브러리";
    const ok = await ask({
      title: `${libName}에서 제외한 ${toClean.files.toLocaleString()}장을 휴지통으로 옮깁니다`,
      lines: [
        `· ${fmtBytes(toClean.bytes)} — 라이브러리 안 .acut/휴지통 으로 갑니다 (디스크 자리는 휴지통을 비워야 빕니다)`,
        "· 사진이 다 나간 폴더는 디스크에서도 지웁니다",
        "· 언제든 되돌릴 수 있습니다",
      ],
      confirmLabel: "휴지통으로",
    });
    if (!ok) return;
    runTrashOp("trash_apply", { libraryId: libId }, "휴지통으로 옮기는 중…");
  }, [ask, runTrashOp]);

  /// 제외 표시를 전부 되돌린다 — 휴지통으로 보내기 전. 파일은 그대로
  const unmarkExcluded = useCallback(async () => {
    const { toClean } = useData.getState();
    const libId = usePrefs.getState().libId;
    if (!toClean || toClean.files === 0) return;
    const libName = useData.getState().libs.find((l) => l.id === libId)?.name ?? "모든 라이브러리";
    const ok = await ask({
      title: `${libName}의 제외 표시 ${toClean.files.toLocaleString()}장을 되돌립니다`,
      lines: ["· 표시만 지웁니다 — 파일은 그대로, 미판정으로 돌아갑니다", "· 닫혀 있던 완전 중복 무리는 개별 비교에 다시 나옵니다"],
      confirmLabel: "표시 취소",
    });
    if (!ok) return;
    try {
      const n = await invoke<number>("files_unmark_excluded", { libraryId: libId });
      toast(`${n.toLocaleString()}장의 제외 표시를 되돌렸습니다`, "ok");
      await after();
      useData.getState().refreshTrash(libId);
    } catch (e) {
      toast(String(e), "drop");
    }
  }, [ask, after]);

  /// 휴지통에서 고른 것만 제자리로
  const restoreFiles = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return false;
      return runTrashOp("trash_restore", { libraryId: null, ids }, "되돌리는 중…");
    },
    [runTrashOp],
  );

  /// 휴지통에서 고른 것만 영구히 — 되돌릴 수 없다
  const deleteFiles = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return false;
      const ok = await ask({
        title: `고른 ${ids.length.toLocaleString()}장을 영구히 지웁니다`,
        lines: ["· 디스크에서 사라집니다", "· 되돌릴 수 없습니다"],
        confirmLabel: "영구히 지우기",
        danger: true,
      });
      if (!ok) return false;
      return runTrashOp("trash_empty", { libraryId: null, ids }, "지우는 중…");
    },
    [ask, runTrashOp],
  );

  const emptyTrash = useCallback(async () => {
    const { trash } = useData.getState();
    const libId = usePrefs.getState().libId;
    if (!trash || trash.files === 0) return;
    const ok = await ask({
      title: `휴지통의 ${trash.files.toLocaleString()}장을 영구히 지웁니다`,
      lines: [
        `· ${fmtBytes(trash.bytes)}가 디스크에서 사라집니다`,
        "· 되돌릴 수 없습니다",
      ],
      confirmLabel: "영구히 지우기",
      danger: true,
    });
    if (!ok) return;
    runTrashOp("trash_empty", { libraryId: libId, ids: [] }, "지우는 중…");
  }, [ask, runTrashOp]);

  const restoreAll = useCallback(() => {
    const libId = usePrefs.getState().libId;
    runTrashOp("trash_restore", { libraryId: libId, ids: [] }, "되돌리는 중…");
  }, [runTrashOp]);

  /// 가장 최근의 아직 안 물린 작업을 되돌린다 (⌘Z).
  const undoLast = useCallback(async () => {
    const { batches, setBusy } = useData.getState();
    // 상태바와 같은 규칙 — 가장 최근 작업이 정리·이름 바꾸기·가져오기일 때만
    const latest = batches[0];
    const last =
      latest && latest.undone_at === null && isUndoableBatchKind(latest.kind)
        ? latest
        : undefined;
    if (!last) return;
    setBusy("되돌리는 중…");
    try {
      const r = await invoke<Outcome>("batch_undo", { batchId: last.id });
      setBusy(
        r.failed > 0 ? `${r.failed}장 실패 — ${r.first_error ?? ""}` : "",
      );
      if (r.failed === 0)
        toast(
          r.moved > 0
            ? `«${last.label ?? "최근 작업"}» 되돌렸습니다 — ${r.moved.toLocaleString()}장${
                // 성공했어도 알아야 할 것(옆 이름으로 복원 등)은 first_error 로 온다
                r.first_error ? ` · ${r.first_error}` : ""
              }`
            : (r.first_error ?? "되돌릴 것이 없습니다"),
        );
      await after();
    } catch (e) {
      setBusy(String(e));
    }
  }, [after]);

  /// 등록을 지우면 그 라이브러리의 폴더·파일 기록이 CASCADE로 전부 사라진다.
  /// 원본 사진은 그대로지만 스캔은 처음부터 다시 해야 한다. 실제로 ⟳ 바로 옆에
  /// 붙어 있다가 잘못 눌려 6만 장짜리 라이브러리가 통째로 날아간 적이 있다.
  const dropLibrary = useCallback(
    async (l: Library) => {
      const ok = await ask({
        title: `「${l.name}」 등록을 지웁니다`,
        lines: [
          `· 사진 ${l.file_count.toLocaleString()}장의 기록과 판정·평점이 사라집니다`,
          "· 다시 등록하면 처음부터 스캔해야 합니다",
          "· 원본 사진과 썸네일 파일은 지워지지 않습니다",
        ],
        confirmLabel: "등록 지우기",
        danger: true,
      });
      if (!ok) return;
      await invoke("library_remove", { id: l.id });
      const prefs = usePrefs.getState();
      if (prefs.libId === l.id) prefs.set("libId", null);
      useView.getState().setSel(null);
      useData.getState().setOpen(new Set());
      await after();
    },
    [ask, after],
  );

  return {
    runTrashOp,
    trashFiles,
    cleanExcluded,
    unmarkExcluded,
    restoreFiles,
    deleteFiles,
    emptyTrash,
    restoreAll,
    undoLast,
    dropLibrary,
    after,
  };
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Compare from "./Compare";
import ContextMenu from "./ContextMenu";
import Cull from "./Cull";
import Filmstrip from "./Filmstrip";
import Import from "./Import";
import Organize from "./Organize";
import PhotoGrid from "./PhotoGrid";
import SelectionPanel from "./SelectionPanel";
import Shortcuts from "./Shortcuts";
import Sidebar from "./Sidebar";
import StatusActions from "./StatusActions";
import StatusBar from "./StatusBar";
import Toasts from "./Toasts";
import Toolbar from "./Toolbar";
import Viewer from "./Viewer";
import { contextItems } from "./contextItems";
import { useData } from "./dataStore";
import { usePref } from "./prefs";
import { useSelection } from "./selectionStore";
import { thumbUrl, type Mark } from "./types";
import { toast } from "./toastStore";
import { useUi } from "./uiStore";
import { useGridKeys } from "./useGridKeys";
import { useGridLayout } from "./useGridLayout";
import { useOps } from "./useOps";
import { usePhotoList } from "./usePhotoList";
import { useScanEvents } from "./useScanEvents";
import { facetOf, useFilter } from "./viewStore";

/**
 * 화면의 뼈대 — 툴바 / 레일·사이드바 / 그리드 / 선택 패널 / 상태바.
 *
 * 상태는 스토어(prefs·view·selection·ui·data·job)에, 일은 훅(목록·치수·
 * 스캔·키보드·작업)에 있다. 여기서는 그것들을 잇고 위에 뜨는 것들을 놓는다.
 */
export default function App() {
  const [libId] = usePref("libId");
  const [group] = usePref("group");
  const [thumbSize] = usePref("thumbSize");
  const [gridStyle] = usePref("gridStyle");
  const [scaling] = usePref("scaling");
  const [caption] = usePref("caption");
  const [filmstrip] = usePref("filmstrip");
  const [watch] = usePref("watch");

  const filter = useFilter();
  const facetFilter = useMemo(() => facetOf(filter), [filter]);

  const libs = useData((s) => s.libs);
  const buckets = useData((s) => s.buckets);
  const stats = useData((s) => s.stats);
  const refreshMetaRaw = useData((s) => s.refreshMeta);
  const refreshMeta = useCallback(
    () => refreshMetaRaw(filter, libId),
    [refreshMetaRaw, filter, libId],
  );

  // ── 시작 ────────────────────────────────────────────────────────
  // 옛 위치(디스크 안)의 캐시가 있으면 앱 폴더로 옮겨 온다. 한 번만 걸린다.
  useEffect(() => {
    const d = useData.getState();
    (async () => {
      try {
        const [moved] = await invoke<[number, number]>("cache_migrate");
        if (moved > 0)
          d.setBusy(`썸네일 ${moved.toLocaleString()}장을 옮겼습니다`);
      } catch {
        /* 옮길 것이 없으면 그냥 넘어간다 */
      }
      d.refreshLibs();
      d.refreshCache();
      d.refreshTags();
    })();
  }, []);
  // 폴더 트리는 라이브러리 목록이 바뀔 때만. libId에 매달지 않는다 — 폴더를
  // 누르면 그 라이브러리로 옮겨가는데 그때 트리를 다시 읽으면 방금 누른
  // 줄이 눈앞에서 사라진다.
  useEffect(() => {
    useData.getState().loadFolders();
  }, [libs]);
  // 폴더 감시 — 라이브러리 목록이나 설정이 바뀔 때 지금 목록에 맞춘다
  useEffect(() => {
    if (libs.length === 0) return;
    invoke("watch_set", { enabled: watch }).catch(() => {});
  }, [libs, watch]);

  // ── 목록·치수 ────────────────────────────────────────────────────
  // 목록은 스크롤을 되돌릴 함수가 필요하고, 치수는 목록이 필요하다.
  // 순환을 ref 하나로 푼다.
  const resetScroll = useRef<() => void>(() => {});
  const list = usePhotoList(filter, group, {
    enabled: libs.length > 0,
    onReload: refreshMeta,
    onSeek: () => resetScroll.current(),
  });
  const { rows, loadFirst, loadMore, markOne } = list;

  const selected = useSelection((s) => s.selected);
  const picked = useSelection((s) => s.picked);
  const layout = useGridLayout(rows, {
    thumbSize,
    gridStyle,
    caption,
    loadMore,
    selected,
  });
  useEffect(() => {
    resetScroll.current = layout.resetScroll;
  }, [layout.resetScroll]);

  // ── 선택 ────────────────────────────────────────────────────────
  /// 뷰어가 훑고 다닐 목록 — 지금 화면에 올라온 순서 그대로
  const ids = useMemo(() => rows.map((r) => r.id), [rows]);
  /// 뷰어가 상세를 읽기 전에 사진·영상을 가르게 — 정지 프레임이 한 번
  /// 그려졌다 영상으로 바뀌는 깜빡임을 막는다
  const kinds = useMemo(() => new Map(rows.map((r) => [r.id, r.kind])), [rows]);
  const kindOf = useCallback((id: number) => kinds.get(id), [kinds]);
  /// 고른 것들의 배열. Set을 그대로 넘기면 렌더마다 새 배열이라 정리 패널의
  /// 제안 요청이 끝없이 다시 돈다.
  const pickedIds = useMemo(() => [...picked], [picked]);
  /// 나란히 놓을 것 — 고른 것 중 **목록에 놓인 순서대로** 앞의 넷.
  const compareIds = useMemo(
    () =>
      rows
        .filter((r) => picked.has(r.id))
        .slice(0, 4)
        .map((r) => r.id),
    [rows, picked],
  );
  // 목록이 바뀌면 초점이 사라졌을 때만 첫 장으로. 상태바가 늘 한 장을 가리킨다.
  useEffect(() => {
    useSelection.getState().focusWithin(ids);
  }, [ids]);

  const pick = useCallback(
    (id: number, e: React.MouseEvent) =>
      useSelection
        .getState()
        .pick(id, { meta: e.metaKey || e.ctrlKey, shift: e.shiftKey }, ids),
    [ids],
  );
  const markPicked = useCallback(
    (patch: Mark) =>
      useSelection.getState().picked.forEach((id) => markOne(id, patch)),
    [markOne],
  );

  // ── 일 ──────────────────────────────────────────────────────────
  const scan = useScanEvents({ reload: loadFirst, refreshMeta });
  const ops = useOps({ reload: loadFirst, refreshMeta });
  useGridKeys({
    rows,
    cols: layout.cols,
    compareIds,
    markOne,
    undoLast: ops.undoLast,
  });

  // ── 위에 뜨는 것들 ───────────────────────────────────────────────
  const ui = useUi();
  // 뷰어가 끝에 다다르면 다음 쪽을 미리 읽는다
  useEffect(() => {
    if (ui.viewerAt !== null && ui.viewerAt >= rows.length - 5) loadMore();
  }, [ui.viewerAt, rows.length, loadMore]);

  /// 타일 우클릭. 고른 것 밖을 우클릭하면 그것 하나만 대상으로 삼는다 —
  /// 안 그러면 눈에 안 보이는 선택에 대고 일이 벌어진다.
  const openContext = useCallback((id: number, e: React.MouseEvent) => {
    e.preventDefault();
    const s = useSelection.getState();
    const target = s.picked.has(id) ? [...s.picked] : [id];
    if (!s.picked.has(id)) s.pick(id, { meta: false, shift: false }, []);
    useUi
      .getState()
      .set({ ctxIds: target, ctxAt: { x: e.clientX, y: e.clientY } });
  }, []);
  const ctxItems = useMemo(
    () =>
      contextItems(ui.ctxIds, rows, { markOne, trashFiles: ops.trashFiles }),
    [ui.ctxIds, rows, markOne, ops.trashFiles],
  );

  // ── 상태바가 가리키는 한 장 ───────────────────────────────────────
  const focusAt = useMemo(
    () => (selected === null ? -1 : rows.findIndex((r) => r.id === selected)),
    [rows, selected],
  );
  const focusExif = useFocusExif(selected);
  /// 찾기 결과 개수 — 눈금 합이 곧 필터에 걸린 장수라 따로 세지 않는다
  const matched = useMemo(
    () => buckets.reduce((a, b) => a + b.count, 0),
    [buckets],
  );

  const openViewer = useCallback(
    (i: number) => useUi.getState().set({ viewerAt: i }),
    [],
  );

  return (
    <div className="h-screen flex flex-col bg-canvas text-fg text-[13px]">
      <Toolbar
        matched={matched}
        addLibrary={scan.addLibrary}
        rescan={scan.rescan}
      />

      <div className="flex-1 flex min-h-0">
        <Sidebar
          filter={filter}
          facetFilter={facetFilter}
          reload={loadFirst}
          rescan={scan.rescan}
          addLibrary={scan.addLibrary}
          dropLibrary={ops.dropLibrary}
        />

        {/* 콘텐츠 영역 — 뷰어는 이 안만 덮는다. 왼쪽 폴더 목록은 계속 보인다.
            세로로 나눈다: 위는 필름스트립, 아래는 그리드와 타임라인. */}
        <div className="flex-1 flex flex-col min-w-0 relative">
          {filmstrip && (
            <Filmstrip
              files={rows}
              thumbUrl={thumbUrl}
              selectedId={selected}
              onPick={pick}
              onOpen={openViewer}
              onNearEnd={loadMore}
            />
          )}
          <PhotoGrid
            loading={list.loading}
            empty={libs.length === 0}
            layout={layout}
            baseIndex={list.baseIndex}
            buckets={buckets}
            caption={caption}
            gridStyle={gridStyle}
            scaling={scaling}
            group={group}
            onPick={pick}
            onOpen={openViewer}
            onContext={openContext}
            onSeek={list.seekTo}
          />

          {ui.organizing && libId !== null && (
            <Organize
              ids={pickedIds}
              libraryId={libId}
              onDone={async (o) => {
                if (o.failed > 0)
                  toast(
                    `${o.moved}장 옮김 · ${o.failed}장 실패 — ${o.first_error ?? ""}`,
                    "drop",
                  );
                else toast(`${o.moved.toLocaleString()}장 옮겼습니다`, "ok");
                useSelection.getState().clearPicked();
                await ops.after();
              }}
              onClose={() => ui.set({ organizing: false })}
            />
          )}

          {/* 크게 보기 — 기본은 콘텐츠 영역만 덮는다 */}
          {ui.viewerAt !== null && (
            <Viewer
              ids={ids}
              index={ui.viewerAt}
              onIndex={openViewer}
              onClose={() => ui.set({ viewerAt: null, viewerFull: false })}
              onMark={markOne}
              fullScreen={ui.viewerFull}
              onToggleFullScreen={() => ui.set({ viewerFull: !ui.viewerFull })}
              kindOf={kindOf}
            />
          )}
        </div>
      </div>

      <ContextMenu
        at={ui.ctxAt}
        items={ctxItems}
        onClose={() => ui.set({ ctxAt: null })}
      />
      <Toasts />
      {ui.helping && <Shortcuts onClose={() => ui.set({ helping: false })} />}
      {ui.importing && (
        <Import
          libs={libs}
          libId={libId}
          onDone={ops.after}
          onClose={() => ui.set({ importing: false })}
        />
      )}
      {ui.comparing && (
        <Compare
          ids={ui.comparing}
          onMark={markOne}
          onClose={() => ui.set({ comparing: null })}
        />
      )}
      {ui.culling && libs.length > 0 && (
        <Cull
          onClose={() => {
            ui.set({ culling: false });
            loadFirst();
          }}
        />
      )}

      {!ui.culling && (
        <SelectionPanel
          rows={rows}
          compareIds={compareIds}
          markPicked={markPicked}
          onTrash={ops.trashFiles}
        />
      )}

      {/* 상태바 — 지금 보고 있는 사진의 정보 (Lap의 StatusBar 구성) */}
      <StatusBar
        index={focusAt >= 0 ? list.baseIndex + focusAt : -1}
        total={matched || (stats?.files ?? 0)}
        totalBytes={stats?.bytes ?? 0}
        file={focusAt >= 0 ? rows[focusAt] : null}
        exif={focusExif}
      >
        <StatusActions
          stopJob={scan.stopJob}
          restoreAll={ops.restoreAll}
          emptyTrash={ops.emptyTrash}
          cleanExcluded={ops.cleanExcluded}
          undoLast={ops.undoLast}
        />
      </StatusBar>
    </div>
  );
}

/// 상태바에 띄울 지금 사진의 카메라·설정. 상세는 따로 읽는다.
function useFocusExif(selected: number | null) {
  const [exif, setExif] = useState<{
    camModel: string | null;
    lens: string | null;
    settings: string;
  } | null>(null);
  useEffect(() => {
    let live = true;
    if (selected === null) {
      queueMicrotask(() => live && setExif(null));
      return () => {
        live = false;
      };
    }
    invoke<{
      camModel: string | null;
      lens: string | null;
      iso: number | null;
      aperture: number | null;
      shutter: string | null;
      focalMm: number | null;
    }>("file_detail", { id: selected })
      .then((d) => {
        if (!live) return;
        setExif({
          camModel: d.camModel,
          lens: d.lens,
          settings: [
            d.focalMm ? `${d.focalMm}mm` : null,
            d.shutter,
            d.aperture ? `f${d.aperture}` : null,
            d.iso ? `ISO ${d.iso}` : null,
          ]
            .filter(Boolean)
            .join(" "),
        });
      })
      .catch(() => live && setExif(null));
    return () => {
      live = false;
    };
  }, [selected]);
  return exif;
}

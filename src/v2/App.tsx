import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useShallow } from "zustand/react/shallow";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import Compare from "./Compare";
import ContextMenu from "./ContextMenu";
import Cull from "./Cull";
import Filmstrip from "./Filmstrip";
import Import from "./Import";
import AreaPickDialog from "./AreaPickDialog";
import OffloadDialog from "./OffloadDialog";
import HuskDialog from "./HuskDialog";
import Organize from "./Organize";
import CaptureDateDialog from "./CaptureDateDialog";
import TransferDialog from "./TransferDialog";
import FolderOperationDialog from "./FolderOperationDialog";
import FolderNameAuditDialog from "./FolderNameAuditDialog";
import EventDiscoveryDialog from "./EventDiscoveryDialog";
import PhotoGrid from "./PhotoGrid";
import Preview from "./Preview";
import SettingsView from "./SettingsView";
import Similar from "./Similar";
import ScrollBar from "./ScrollBar";
import RenameDialog from "./RenameDialog";
import SelectionPanel from "./SelectionPanel";
import Shortcuts from "./Shortcuts";
import Sidebar from "./Sidebar";
import StatusActions from "./StatusActions";
import StatusBar from "./StatusBar";
import Toasts from "./Toasts";
import Toolbar from "./Toolbar";
import ViewerHost from "./ViewerHost";
import BlockingJob from "./BlockingJob";
import { contextItems } from "./contextItems";
import { useData } from "./dataStore";
import { usePref } from "./prefs";
import { useSelection } from "./selectionStore";
import { thumbUrl, type Mark } from "./types";
import { toast } from "./toastStore";
import { useUi } from "./uiStore";
import { useNasAuto } from "./useNasAuto";
import { applyStartupSel } from "./devSel";
import InfoPanel from "./InfoPanel";
import { mark, startupMarks } from "./startupMarks";
import { useGridKeys } from "./useGridKeys";
import { useGridLayout } from "./useGridLayout";
import { useOps } from "./useOps";
import { usePhotoList } from "./usePhotoList";
import { useScanEvents } from "./useScanEvents";
import { useUpdateAuto } from "./useUpdateAuto";
import { facetOf, useFilter } from "./viewStore";

// Leaflet은 위치 갈래에서만 필요하다. 시작 번들에서 떼어 내 첫 화면을 가볍게 한다.
const MapView = lazy(() => import("./MapView"));

/**
 * 화면의 뼈대 — 툴바 / 레일·사이드바 / 그리드 / 선택 패널 / 상태바.
 *
 * 상태는 스토어(prefs·view·selection·ui·data·job)에, 일은 훅(목록·치수·
 * 스캔·키보드·작업)에 있다. 여기서는 그것들을 잇고 위에 뜨는 것들을 놓는다.
 */
export default function App() {
  const [libId, setLibId] = usePref("libId");
  useNasAuto();
  useUpdateAuto();
  // 재현용 — 시작 주소에 ?sel= 이 있으면 그 폴더를 바로 연다
  useEffect(() => {
    void applyStartupSel();
  }, []);
  const [group] = usePref("group");
  const [thumbSize] = usePref("thumbSize");
  const [gridStyle] = usePref("gridStyle");
  const [caption] = usePref("caption");
  const [filmstrip] = usePref("filmstrip");
  const [watch] = usePref("watch");
  const [source] = usePref("source");
  const [font] = usePref("font");
  const [statusBar] = usePref("statusBar");
  const [infoPanel, setInfoPanel] = usePref("infoPanel");
  const [dblClick] = usePref("dblClick");
  const [stripPos] = usePref("stripPos");
  // 글꼴 — CSS가 data-font를 본다
  useEffect(() => {
    document.documentElement.dataset.font = font;
  }, [font]);

  const filter = useFilter();
  const facetFilter = useMemo(() => facetOf(filter), [filter]);

  const libs = useData((s) => s.libs);
  const buckets = useData((s) => s.buckets);
  const summary = useData((s) => s.summary);
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
        await d.refreshLibs();
        mark("libs");
        d.refreshCache();
        d.refreshTags();
      } catch (e) {
        toast(`라이브러리를 불러오지 못했습니다 — ${String(e)}`, "drop");
      }
    })();
  }, []);
  // 옛 캐시 옮기기는 첫 화면 뒤에 — 외부 SSD를 깨우느라 콜드 스타트에서 1.7초를
  // 먹었다(실측). 옮길 것이 있으면 그때 알린다.
  useEffect(() => {
    const t = window.setTimeout(async () => {
      try {
        const [moved] = await invoke<[number, number]>("cache_migrate");
        if (moved > 0) {
          useData
            .getState()
            .setBusy(`썸네일 ${moved.toLocaleString()}장을 옮겼습니다`);
          useData.getState().refreshCache();
        }
      } catch {
        /* 옮길 것이 없으면 그냥 넘어간다 */
      }
    }, 3000);
    return () => window.clearTimeout(t);
  }, []);
  // 폴더 트리는 라이브러리 목록이 바뀔 때만. libId에 매달지 않는다 — 폴더를
  // 누르면 그 라이브러리로 옮겨가는데 그때 트리를 다시 읽으면 방금 누른
  // 줄이 눈앞에서 사라진다.
  useEffect(() => {
    useData.getState().loadFolders();
  }, [libs]);
  // 파인더에서 끌어다 놓기 — 놓으면 가져오기 상자가 그 경로들로 뜬다.
  // Tauri가 OS 드롭을 가로채 경로를 준다 (브라우저 DataTransfer가 아니다).
  useEffect(() => {
    let un: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const ui = useUi.getState();
        if (e.payload.type === "enter" || e.payload.type === "over") {
          if (!ui.dragging) ui.set({ dragging: true });
        } else if (e.payload.type === "leave") {
          ui.set({ dragging: false });
        } else if (e.payload.type === "drop") {
          ui.set({
            dragging: false,
            dropped: e.payload.paths,
            importing: true,
          });
        }
      })
      .then((f) => (un = f))
      .catch(() => {});
    return () => un?.();
  }, []);

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
  // 살아 있음 — 5초마다. 뒷단이 20초 넘게 못 들으면 화면을 다시 불러온다
  useEffect(() => {
    const beat = () => invoke("heartbeat").catch(() => {});
    beat();
    const t = window.setInterval(beat, 5_000);
    return () => window.clearInterval(t);
  }, []);
  // 첫 그리드가 그려지면 한 번 — 시작 시간을 잰다 (설정 › 정보에 보인다).
  // **`loading` 이 아니라 `loaded` 를 본다.** `loading` 은 처음에 `false` 라 그것만
  // 보면 라이브러리 목록이 온 순간(아직 사진은 한 장도 안 읽은 때)이 «첫 그리드»로
  // 기록됐다 — 그래서 «웹뷰가 느리다»로 읽혔지만 실은 libraries_list 한 호출이었다.
  const reported = useRef(false);
  useEffect(() => {
    if (reported.current || !list.loaded || libs.length === 0) return;
    reported.current = true;
    mark("grid");
    invoke("startup_report", { marks: startupMarks() }).catch(() => {});
  }, [list.loaded, libs.length]);
  const { rows, loadFirst, loadMore, markOne, markMany, patchRow } = list;

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
    (patch: Mark) => {
      // 판정이 바뀌면 상태바의 «제외 N장 치우기»도 바뀌어야 한다 — 그 수는 refreshMeta 가 센다 (리뷰 H2)
      markMany([...useSelection.getState().picked], patch)
        .then(() => refreshMeta())
        .catch((e) => toast(String(e), "drop"));
    },
    [markMany, refreshMeta],
  );

  // ── 일 ──────────────────────────────────────────────────────────
  const scan = useScanEvents({ reload: loadFirst, refreshMeta });
  const ops = useOps({ reload: loadFirst, refreshMeta });
  useGridKeys({
    rows,
    cols: layout.cols,
    compareIds,
    markOne,
    markMany,
    undoLast: ops.undoLast,
  });

  /// 이름을 바꾼다 — 서버가 준 이름(NFC)으로 목록을 고친다. 실패는 던진다.
  const renameFile = useCallback(
    async (id: number, name: string) => {
      const next = await invoke<string>("file_rename", { id, name });
      patchRow(id, { name: next });
      useData.getState().refreshBatches();
      toast(`「${next}」로 바꿨습니다`, "ok");
      return next;
    },
    [patchRow],
  );

  // ── 위에 뜨는 것들 ───────────────────────────────────────────────
  // 쓰는 칸만 얕게 고른다 — 전체를 구독하면 뷰어 화살표·끌기마다 격자 전체가 다시 그려진다 (리뷰 H17)
  const ui = useUi(
    useShallow((s) => ({
      set: s.set,
      offload: s.offload,
      husks: s.husks,
      similarFor: s.similarFor,
      renaming: s.renaming,
      areaPick: s.areaPick,
      textSearch: s.textSearch,
      dragging: s.dragging,
      culling: s.culling,
      ctxIds: s.ctxIds,
      ctxAt: s.ctxAt,
      comparing: s.comparing,
      organizing: s.organizing,
      organizeSelection: s.organizeSelection,
      captureDate: s.captureDate,
      transfer: s.transfer,
      folderOperation: s.folderOperation,
      folderAudit: s.folderAudit,
      eventDiscovery: s.eventDiscovery,
      importing: s.importing,
      helping: s.helping,
      dropped: s.dropped,
    })),
  );

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
  const closeContext = useCallback(
    () => useUi.getState().set({ ctxAt: null }),
    [],
  );
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
  /// 현재 필터 전체의 장수 — 화면에 아직 내려받지 않은 행까지 포함한다.
  const matched = summary?.files ?? 0;

  const openViewer = useCallback(
    (i: number) => useUi.getState().set({ viewerAt: i }),
    [],
  );
  /// 두 번 눌렀을 때 — 설정에 따라 크게 보기 또는 기본 앱으로. 키보드
  /// Space·Enter는 늘 크게 보기다 (useGridKeys).
  const openAt = useCallback(
    (i: number) => {
      const r = rows[i];
      if (dblClick === "app" && r)
        invoke("open_in_default_app", { id: r.id }).catch(() => {});
      else openViewer(i);
    },
    [rows, dblClick, openViewer],
  );

  /// 필름스트립 띠 — 설정에 따라 위 또는 아래에 놓는다
  const STRIP = (
    <Filmstrip
      position={stripPos}
      files={rows}
      thumbUrl={thumbUrl}
      selectedId={selected}
      onPick={pick}
      onOpen={openAt}
      onNearEnd={loadMore}
    />
  );

  return (
    <div className="h-screen flex flex-col bg-canvas text-fg text-[14px]">
      <Toolbar
        matched={matched}
        rows={rows}
        compareIds={compareIds}
        markPicked={markPicked}
        onTrash={ops.trashFiles}
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
          {source === "settings" ? (
            <SettingsView
              onRescanAll={() => scan.rescan(libs.map((l) => l.id))}
            />
          ) : filmstrip ? (
            /* 필름스트립 — 위는 띠, 아래는 고른 한 장 (Lap의 Content.vue) */
            <>
              {stripPos === "top" && STRIP}
              <div className="flex-1 flex min-h-0 min-w-0 relative">
                <Preview
                  file={focusAt >= 0 ? rows[focusAt] : null}
                  onOpen={() => focusAt >= 0 && openAt(focusAt)}
                />
                <ScrollBar
                  buckets={buckets}
                  offset={list.baseIndex + Math.max(0, focusAt)}
                  pageSize={1}
                  onSeek={list.seekTo}
                />
                {infoPanel && (
                  <InfoPanel
                    file={focusAt >= 0 ? rows[focusAt] : null}
                    onClose={() => setInfoPanel(false)}
                  />
                )}
              </div>
              {stripPos === "bottom" && STRIP}
            </>
          ) : (
            <>
              {/* 위치 갈래 — 지도 위, 그리드 아래. 칸을 누르면 그 자리의 사진만 남는다 */}
              {source === "location" && (
                <Suspense
                  fallback={
                    <div className="h-72 shrink-0 grid place-items-center bg-canvas text-xs text-fg-mute">
                      지도 불러오는 중…
                    </div>
                  }
                >
                  <MapView filter={filter} />
                </Suspense>
              )}
              {/* 격자 오른쪽에 정보 패널 — 고른 한 장의 상세 */}
              <div className="flex-1 flex min-h-0 min-w-0">
                <PhotoGrid
                  loading={list.loading}
                  error={list.error}
                  empty={libs.length === 0}
                  layout={layout}
                  baseIndex={list.baseIndex}
                  buckets={buckets}
                  caption={caption}
                  gridStyle={gridStyle}
                  group={group}
                  onPick={pick}
                  onOpen={openAt}
                  onContext={openContext}
                  onSeek={list.seekTo}
                  onRetry={loadFirst}
                />
                {infoPanel && (
                  <InfoPanel
                    file={focusAt >= 0 ? rows[focusAt] : null}
                    onClose={() => setInfoPanel(false)}
                  />
                )}
              </div>
            </>
          )}

          {ui.organizing &&
            (ui.organizeSelection?.libraryId ?? libId) !== null && (
              <Organize
                ids={ui.organizeSelection?.ids ?? pickedIds}
                libraryId={ui.organizeSelection?.libraryId ?? libId!}
                onDone={async (o) => {
                  if (o.failed > 0)
                    toast(
                      `${o.moved + o.copied}장 처리 · ${o.failed}장 실패 — ${o.first_error ?? ""}`,
                      "drop",
                    );
                  else if (o.copied > 0)
                    toast(
                      `${o.copied.toLocaleString()}장 공용에 복사${o.already_published > 0 ? ` · ${o.already_published.toLocaleString()}장 이미 발행됨` : ""}`,
                      "ok",
                    );
                  else if (o.already_published > 0)
                    toast(
                      `${o.already_published.toLocaleString()}장은 이미 공용에 있습니다`,
                      "ok",
                    );
                  else toast(`${o.moved.toLocaleString()}장 옮겼습니다`, "ok");
                  if (o.failed > 0) {
                    // 성공한 것은 목록에서 빠지고 실패한 것만 다시 시도할 수 있게 남긴다.
                    // 예전 백엔드 응답이면 원래 선택을 보존한다.
                    useSelection
                      .getState()
                      .setPicked(
                        o.failed_ids?.length
                          ? o.failed_ids
                          : (ui.organizeSelection?.ids ?? pickedIds),
                      );
                  } else {
                    useSelection.getState().clearPicked();
                  }
                  await ops.after();
                }}
                onClose={() =>
                  ui.set({ organizing: false, organizeSelection: null })
                }
              />
            )}

          {/* 크게 보기 — 기본은 콘텐츠 영역만 덮는다. 뷰어 상태는 ViewerHost 만 구독한다 */}
          <ViewerHost
            ids={ids}
            onNearEnd={loadMore}
            onMark={markOne}
            kindOf={kindOf}
            onRename={renameFile}
          />
        </div>
      </div>

      <ContextMenu at={ui.ctxAt} items={ctxItems} onClose={closeContext} />
      <Toasts />
      <BlockingJob />
      {ui.helping && <Shortcuts onClose={() => ui.set({ helping: false })} />}
      {ui.captureDate !== null && (
        <CaptureDateDialog
          target={ui.captureDate}
          onChanged={async () => {
            await ops.after();
          }}
          onClose={() => ui.set({ captureDate: null })}
        />
      )}
      {ui.transfer !== null && (
        <TransferDialog
          ids={ui.transfer.ids}
          sourceLibraryId={ui.transfer.sourceLibraryId}
          onChanged={async () => {
            useSelection.getState().clearPicked();
            await ops.after();
            await useData.getState().loadFolders();
          }}
          onClose={() => ui.set({ transfer: null })}
        />
      )}
      {ui.folderOperation !== null && (
        <FolderOperationDialog
          target={ui.folderOperation}
          onChanged={async () => {
            await useData.getState().loadFolders();
            await ops.after();
          }}
          onClose={() => ui.set({ folderOperation: null })}
        />
      )}
      {ui.folderAudit !== null && (
        <FolderNameAuditDialog
          libraryId={ui.folderAudit.libraryId}
          libraryName={ui.folderAudit.libraryName}
          onChanged={async () => {
            await useData.getState().loadFolders();
            await ops.after();
          }}
          onClose={() => ui.set({ folderAudit: null })}
        />
      )}
      {ui.eventDiscovery !== null && (
        <EventDiscoveryDialog
          libraryId={ui.eventDiscovery.libraryId}
          libraryName={ui.eventDiscovery.libraryName}
          onChoose={(chosen) => {
            setLibId(ui.eventDiscovery!.libraryId);
            useSelection.getState().setPicked(chosen);
            useSelection.getState().setSelected(chosen[0] ?? null);
            ui.set({
              eventDiscovery: null,
              organizing: true,
              organizeSelection: {
                ids: chosen,
                libraryId: ui.eventDiscovery!.libraryId,
              },
            });
          }}
          onClose={() => ui.set({ eventDiscovery: null })}
        />
      )}
      {ui.textSearch !== null && ui.similarFor === null && (
        <Similar
          query={{ text: ui.textSearch }}
          onPick={(id) => ui.set({ similarFor: id })}
          onMark={markOne}
          onClose={() => ui.set({ textSearch: null })}
        />
      )}
      {ui.similarFor !== null && (
        <Similar
          query={{ id: ui.similarFor }}
          onPick={(id) => ui.set({ similarFor: id })}
          onMark={markOne}
          onClose={() => ui.set({ similarFor: null })}
        />
      )}
      {ui.renaming !== null && (
        <RenameDialog
          name={rows.find((r) => r.id === ui.renaming)?.name ?? ""}
          onSubmit={async (n) => {
            await renameFile(ui.renaming!, n);
          }}
          onClose={() => ui.set({ renaming: null })}
        />
      )}
      {ui.offload !== null && (
        <OffloadDialog
          folderId={ui.offload.folderId}
          name={ui.offload.name}
          libraryId={ui.offload.libraryId}
          onClose={() => ui.set({ offload: null })}
        />
      )}
      {ui.husks !== null && (
        <HuskDialog
          libraryId={ui.husks.libraryId}
          name={ui.husks.name}
          onClose={() => ui.set({ husks: null })}
        />
      )}
      {ui.areaPick !== null && (
        <AreaPickDialog
          path={ui.areaPick}
          onPick={(area) => {
            const p = ui.areaPick!;
            ui.set({ areaPick: null });
            scan.registerLibrary(p, area);
          }}
          onClose={() => ui.set({ areaPick: null })}
        />
      )}
      {ui.importing && (
        <Import
          libs={libs}
          libId={libId}
          initialSources={ui.dropped}
          onDone={ops.after}
          onClose={() => ui.set({ importing: false, dropped: [] })}
        />
      )}
      {ui.dragging && (
        <div className="fixed inset-0 z-[70] pointer-events-none flex items-center justify-center bg-accent/10 ring-4 ring-inset ring-accent">
          <div className="px-5 py-3 rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl text-[15px] text-fg">
            놓으면 라이브러리로 가져옵니다 — 원본은 그대로 둡니다
          </div>
        </div>
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
            refreshMeta();
          }}
          onChanged={refreshMeta}
          cleanExcluded={() => ops.cleanExcluded(null)}
        />
      )}

      {!ui.culling && (
        <SelectionPanel
          rows={rows}
          compareIds={compareIds}
          markPicked={markPicked}
          onTrash={ops.trashFiles}
          onRestore={ops.restoreFiles}
          onDelete={ops.deleteFiles}
        />
      )}

      {/* 상태바 — 지금 보고 있는 사진의 정보 (Lap의 StatusBar 구성) */}
      {statusBar && (
        <StatusBar
          index={focusAt >= 0 ? list.baseIndex + focusAt : -1}
          total={summary?.files ?? 0}
          totalBytes={summary?.bytes ?? 0}
          file={focusAt >= 0 ? rows[focusAt] : null}
          exif={focusExif}
        >
          <StatusActions
            stopJob={scan.stopJob}
            restoreAll={ops.restoreAll}
            emptyTrash={ops.emptyTrash}
            cleanExcluded={ops.cleanExcluded}
            unmarkExcluded={ops.unmarkExcluded}
            undoLast={ops.undoLast}
          />
        </StatusBar>
      )}
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

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import Cull from "./Cull";
import Viewer from "./Viewer";
import ScrollBar from "./ScrollBar";
import Organize from "./Organize";
import Tile from "./Tile";
import SortMenu, { DEFAULT_SORT, type Sort } from "./SortMenu";
import GroupMenu, { type GroupBy } from "./GroupMenu";
import { layout, headerLabel, HEADER_H } from "./gridLayout";
import ViewMenu from "./ViewMenu";
import ContextMenu, {
  type MenuAt,
  type MenuItem as CtxItem,
} from "./ContextMenu";
import Rail from "./Rail";
import { visible } from "./folderTree";
import { sourceTitle, type Source } from "./railItems";
import TagPanel from "./TagPanel";
import SmartPanel from "./SmartPanel";
import SearchPanel from "./SearchPanel";
import SettingsPanel from "./SettingsPanel";
import Breadcrumb from "./Breadcrumb";
import StatusBar from "./StatusBar";
import {
  Btn,
  IconBtn,
  Kbd,
  Label,
  Menu,
  MenuItem,
  MenuSep,
  QuickRow,
  Sep,
} from "./ui";
import FacetList from "./FacetList";
import Calendar from "./Calendar";
import {
  justify,
  ratio,
  type GridStyle,
  type JustifiedRow,
  type Scaling,
} from "./gridStyle";
import { useCountUp } from "./useCountUp";
import FilterButton from "./FilterBar";
import {
  EMPTY as EMPTY_PICKS,
  isEmpty as isEmptyPicks,
  picksFrom,
  type Picks,
} from "./picks";
import { fmtBytes, fmtDate } from "./format";

// ── 타입 (Rust 쪽과 맞춰야 한다) ─────────────────────────────────────
type FileRow = {
  id: number;
  name: string;
  taken_at: number;
  taken_at_source: number;
  kind: number;
  size: number;
  width: number | null;
  height: number | null;
  rating: number;
  culling_flag: number;
  favorite: boolean;
  duration_ms: number | null;
  /** 묶기를 켰을 때의 그룹 값. 서버가 행마다 붙여 준다. */
  group: string | null;
  /** 어느 라이브러리 소속인가. 썸네일 주소를 만들 때 쓴다 */
  library_id: number | null;
  /** 캐시 루트 기준 상대경로. null이면 아직 생성 전 */
  thumb: string | null;
};
type Cursor = { num: number | null; text: string | null; id: number };
type Page = { rows: FileRow[]; next: Cursor | null };
/** 등록한 사진 폴더. 여러 개, 서로 다른 디스크에 있어도 된다 */
type Library = {
  id: number;
  volume_uuid: string;
  volume_name: string;
  rel_path: string;
  name: string;
  area: number;
  /** 지금 그 디스크가 꽂혀 있는가 */
  online: boolean;
  dir: string | null;
  file_count: number;
};
type Stats = {
  files: number;
  bytes: number;
  thumbs_done: number;
  thumbs_pending: number;
};
/** 캐시 용량 — 디스크를 훑어야 해서 자주 부르지 않는다 */
type CacheUsage = { bytes: number; files: number };
type Counted = { files: number; bytes: number };
type Batch = {
  id: number;
  kind: string;
  label: string | null;
  item_count: number;
  created_at: number;
  undone_at: number | null;
};
type Outcome = {
  batch_id: number;
  moved: number;
  failed: number;
  bytes: number;
  first_error: string | null;
};
type Bucket = { year: number; month: number; count: number; top: number };
/** 사이드바 트리 한 줄. 중간 마디는 DB 행이 없어 id가 null이다. */
type FolderRow = {
  id: number | null;
  /** 이 줄이 속한 라이브러리 */
  library_id: number;
  /** 라이브러리 루트 기준 — 접기의 열쇠 */
  path: string;
  /** 볼륨 기준 — 필터로 보낸다 */
  rel_path: string;
  name: string;
  depth: number;
  file_count: number;
  has_children: boolean;
  /** 라이브러리 자신인가 (트리의 뿌리) */
  is_library: boolean;
};

const PAGE = 300;
const GAP = 10;

export default function App() {
  /// 등록된 라이브러리 전부
  const [libs, setLibs] = useState<Library[]>([]);
  /// 지금 보고 있는 라이브러리. null이면 전부 섞어서 본다.
  const [libId, setLibId] = useState<number | null>(null);
  const [rows, setRows] = useState<FileRow[]>([]);
  const [cursor, setCursor] = useState<Cursor | null>(null);
  const [done, setDone] = useState(false);
  const [loading, setLoading] = useState(false);
  const [stats, setStats] = useState<Stats | null>(null);
  const [cache, setCache] = useState<CacheUsage | null>(null);
  const [folders, setFolders] = useState<FolderRow[]>([]);
  /// 고른 폴더. `path`는 트리 표시용(라이브러리 기준), `rel`은 필터용(볼륨 기준).
  /// 둘을 함께 들고 있어야 한다 — 폴더 목록에서 되찾으려 하면 목록이
  /// 필터에 얽혀 무한 루프가 된다.
  /// 사이드바에서 고른 폴더. `libId`는 그 폴더가 든 라이브러리다 —
  /// 두 라이브러리에 같은 rel_path가 있을 수 있어 짝으로 들고 있어야 한다.
  const [sel, setSel] = useState<{
    libId: number;
    path: string;
    rel: string;
  } | null>(null);
  /// 펼쳐 둔 마디들 (라이브러리 기준 경로)
  const [open, setOpen] = useState<Set<string>>(new Set());
  /// 찾기 줄에서 고른 것들
  const [picks, setPicks] = useState<Picks>(EMPTY_PICKS);
  const [sort, setSort] = useState<Sort>(DEFAULT_SORT);
  const [group, setGroup] = useState<GroupBy>("none");
  const [gridStyle, setGridStyle] = useState<GridStyle>("card");
  /// 필름스트립 — 그리드 아래에 고른 사진을 크게 띄운다.
  /// 자리만 잡아 뒀다. Lap의 Content.vue가 그리드와 MediaViewer를 위아래로
  /// 나누는 구조인데, 지금은 크게 보기(뷰어)로 충분해서 뒤로 미뤘다.
  const [filmstrip, setFilmstrip] = useState(false);
  const [scaling, setScaling] = useState<Scaling>("cover");
  /// 휴지통을 보고 있는가
  const [viewTrash, setViewTrash] = useState(false);
  /// 휴지통에 든 것 / 제외 판정만 하고 아직 안 치운 것
  const [trash, setTrash] = useState<Counted | null>(null);
  const [toClean, setToClean] = useState<Counted | null>(null);
  const [busy, setBusy] = useState("");
  const [scanMsg, setScanMsg] = useState<string>("");
  /// 진행 중인 작업의 (한 일, 전체). 숫자는 화면에서 한 칸씩 따라 오른다.
  const [job, setJob] = useState<{
    label: string;
    done: number;
    total: number;
  } | null>(null);
  const [thumbSize, setThumbSize] = useState(180);
  /// 키보드·뷰어가 기준으로 삼는 한 장
  const [selected, setSelected] = useState<number | null>(null);
  /// 여러 장 고르기. 정리는 이 묶음을 옮긴다.
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [organizing, setOrganizing] = useState(false);
  /// 「⋯」를 연 라이브러리. 지우기는 이 안에 숨겨 둔다.
  const [menuFor, setMenuFor] = useState<number | null>(null);
  /// 사이드바가 무엇을 보여줄지, 펴져 있는지, 얼마나 넓은지
  const [source, setSource] = useState<Source>("all");
  const [panelOpen, setPanelOpen] = useState(true);
  const [panelW, setPanelW] = useState(224);
  const dragPanel = useRef(false);
  /// 우클릭 메뉴가 뜬 자리와 그때 잡힌 사진들
  const [ctxAt, setCtxAt] = useState<MenuAt>(null);
  const [ctxIds, setCtxIds] = useState<number[]>([]);
  const [batches, setBatches] = useState<Batch[]>([]);
  const [culling, setCulling] = useState(false);
  /// 뷰어에 띄운 사진의 rows 안 위치. null이면 뷰어가 닫힌 상태
  const [viewerAt, setViewerAt] = useState<number | null>(null);
  const [buckets, setBuckets] = useState<Bucket[]>([]);
  /// rows[0]이 전체에서 몇 번째인가. 스크롤바 손잡이 위치의 기준이다.
  const [baseIndex, setBaseIndex] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(0);
  const [viewW, setViewW] = useState(0);
  /// 뷰어를 창 전체로 — 기본은 사이드바를 남겨 둔다
  const [viewerFull, setViewerFull] = useState(false);

  const scrollRef = useRef<HTMLDivElement>(null);
  // 요청이 겹치지 않게 — 스크롤이 빠르면 같은 페이지를 두 번 부를 수 있다
  const inflight = useRef(false);
  /// 잠겨 있는 동안 들어온 스크롤바 요청. 마지막 것만 남는다.
  const pending = useRef<number | null>(null);
  /// 아직 스캔하지 않은 라이브러리들. 하나가 끝나면 다음을 시작한다.
  const queue = useRef<number[]>([]);
  /// 잠금이 풀릴 때 밀린 요청을 이어받는다 (아래에서 채운다)
  const drain = useRef<() => void>(() => {});
  /// 어떤 경로로 끝나든 여기서만 잠금을 푼다. 안 그러면 밀린 요청이 사라진다.
  const release = useCallback(() => {
    inflight.current = false;
    drain.current();
  }, []);

  const filter = useMemo(
    () => ({
      ...picks,
      sort,
      // 폴더를 고르면 그 폴더가 든 라이브러리로 좁힌다
      library_id: sel ? sel.libId : libId,
      folder_path: viewTrash ? null : (sel?.rel ?? null),
      trashed: viewTrash,
    }),
    [picks, sort, libId, sel, viewTrash],
  );

  /// 스마트 앨범을 편다 — 저장해 둔 조건을 지금 화면에 그대로 건다.
  ///
  /// 저장한 것은 `Filter` 통짜라 라이브러리·폴더·정렬까지 들어 있다. 조건만
  /// 골라 담지 않고 전부 되돌리는 편이 "그때 보던 그대로"에 가깝다.
  const applySmart = useCallback((f: unknown, srt: unknown) => {
    const v = (f ?? {}) as {
      library_id?: number | null;
      folder_path?: string | null;
      trashed?: boolean;
    };
    setPicks(picksFrom(f));
    setLibId(v.library_id ?? null);
    // 폴더는 경로만 되살린다 — 트리에서 고른 것과 같은 모양이면 충분하다.
    setSel(
      v.folder_path && v.library_id != null
        ? { libId: v.library_id, path: v.folder_path, rel: v.folder_path }
        : null,
    );
    setViewTrash(v.trashed ?? false);
    if (srt) setSort(srt as Sort);
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

  const refreshLibs = useCallback(async () => {
    setLibs(await invoke<Library[]>("libraries_list"));
  }, []);

  /// 휴지통과 「치울 것」 개수. 판정을 바꿀 때마다 달라진다.
  const refreshTrash = useCallback(async () => {
    try {
      const [t, p] = await Promise.all([
        invoke<Counted>("trash_summary", { libraryId: libId }),
        invoke<Counted>("trash_pending", { libraryId: libId }),
      ]);
      setTrash(t);
      setToClean(p);
    } catch {
      /* 아직 라이브러리가 없을 수 있다 */
    }
  }, [libId]);

  // 셋을 한꺼번에 던진다. 줄줄이 await하면 셋의 시간이 그대로 더해진다.
  // 둘을 한꺼번에 던진다. 줄줄이 await하면 시간이 그대로 더해진다.
  const refreshMeta = useCallback(async () => {
    try {
      const [st, bk] = await Promise.all([
        invoke<Stats>("library_stats", { libraryId: libId }),
        invoke<Bucket[]>("files_timeline", { filter }),
      ]);
      setStats(st);
      setBuckets(bk);
      refreshTrash();
      refreshBatches();
    } catch {
      /* 아직 등록된 라이브러리가 없을 수 있다 */
    }
  }, [filter, libId]);

  /// 폴더 트리는 **등록된 라이브러리 전부**를 한 번에 읽는다. 라이브러리가
  /// 트리의 뿌리라 고르기 전에도 하위 폴더가 보인다.
  ///
  /// `libId`에 매달지 않는 게 중요하다 — 폴더를 누르면 그 라이브러리로
  /// 옮겨가는데, 그때 트리를 다시 읽으면 방금 누른 줄이 눈앞에서 사라진다.
  useEffect(() => {
    invoke<FolderRow[]>("folders_list", { libraryId: null })
      .then((f) => {
        setFolders(f);
        // 라이브러리 마디는 펴 둔다. 접힌 채로 시작하면 등록한 이름만
        // 덩그러니 있고 폴더는 하나도 안 보인다.
        setOpen((prev) => {
          const next = new Set(prev);
          for (const n of f) if (n.depth === 0) next.add(n.path);
          return next;
        });
      })
      .catch(() => setFolders([]));
  }, [libs]);

  /// 캐시 용량은 디스크의 파일 12만 개를 훑는다. 폴더를 누를 때마다 하면
  /// 앱이 멈춘 것처럼 보인다 — 시작할 때와 썸네일이 끝났을 때만 센다.
  const refreshCache = useCallback(async () => {
    try {
      setCache(await invoke<CacheUsage>("cache_usage", { libraryId: null }));
    } catch {
      /* 디스크가 빠져 있을 수 있다 */
    }
  }, []);

  // 앱 시작 — 등록된 라이브러리를 읽어 온다.
  // 옛 위치(디스크 안)의 캐시가 있으면 앱 폴더로 옮겨 온다. 한 번만 걸린다.
  useEffect(() => {
    (async () => {
      try {
        const [moved] = await invoke<[number, number]>("cache_migrate");
        if (moved > 0)
          setBusy(`썸네일 ${moved.toLocaleString()}장을 옮겼습니다`);
      } catch {
        /* 옮길 것이 없으면 그냥 넘어간다 */
      }
      refreshLibs();
      refreshCache();
    })();
  }, [refreshLibs, refreshCache]);

  // 보는 라이브러리나 폴더가 바뀌면 목록을 새로 읽는다
  useEffect(() => {
    if (libs.length === 0) return;
    setRows([]);
    setCursor(null);
    setDone(false);
    loadFirst();
    refreshMeta();
  }, [
    libs.length,
    libId,
    sel,
    picks,
    sort,
    group,
    viewTrash,
    loadFirst,
    refreshMeta,
  ]);

  // 스캔·썸네일 진행 상황
  useEffect(() => {
    const un: Array<() => void> = [];
    listen<{ found: number; inserted: number; skipped: number }>(
      "scan-progress",
      (e) => {
        const p = e.payload;
        setScanMsg("");
        setJob({ label: "스캔", done: p.inserted + p.skipped, total: p.found });
      },
    ).then((f) => un.push(f));
    listen("scan-done", () => {
      setScanMsg("스캔 완료 — 썸네일 만드는 중");
      setJob(null);
      loadFirst();
      refreshMeta();
    }).then((f) => un.push(f));
    listen<{ done: number; total: number }>("thumb-progress", (e) => {
      setScanMsg("");
      setJob({ label: "썸네일", done: e.payload.done, total: e.payload.total });
    }).then((f) => un.push(f));
    // 2차 — 작게 나온 것을 원본에서 다시 뽑는다. 그 사이에도 앱은 쓸 수 있다.
    listen<{ done: number; total: number }>("upgrade-progress", (e) => {
      setScanMsg("");
      setJob({
        label: "화질 올리는 중 — 그냥 쓰셔도 됩니다",
        done: e.payload.done,
        total: e.payload.total,
      });
    }).then((f) => un.push(f));
    listen("upgrade-done", () => {
      setScanMsg("");
      setJob(null);
      loadFirst();
      refreshMeta();
      refreshCache();
    }).then((f) => un.push(f));
    listen("thumb-done", () => {
      setScanMsg("썸네일 완료 — 화질을 올립니다");
      setJob(null);
      loadFirst();
      refreshMeta();
      refreshLibs();
      refreshCache();
      const next = queue.current.shift();
      if (next === undefined) {
        setScanMsg("");
      } else {
        setScanMsg("다음 라이브러리 스캔…");
        invoke("scan_start", { libraryId: next }).catch((e) =>
          setScanMsg(String(e)),
        );
      }
    }).then((f) => un.push(f));
    listen<string>("scan-error", (e) =>
      setScanMsg(`스캔 실패: ${e.payload}`),
    ).then((f) => un.push(f));
    return () => un.forEach((f) => f());
  }, [loadFirst, refreshMeta, refreshLibs, refreshCache]);

  // ── 가상 스크롤 ──────────────────────────────────────────────────
  const [cols, setCols] = useState(6);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth - GAP;
      setCols(Math.max(1, Math.floor(w / (thumbSize + GAP))));
      setViewH(el.clientHeight);
      setViewW(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [thumbSize]);

  /// 카드는 아래에 날짜가 붙어 한 줄이 더 높다.
  const rowH = thumbSize + (gridStyle === "card" ? 26 : 0) + 4;
  /// 머리글과 사진 줄을 한 목록으로 편다. 묶기를 끄면 사진 줄만 나온다.
  const grid = useMemo(() => layout(rows, (r) => r.group, cols), [rows, cols]);
  /// 양쪽 맞춤일 때 각 사진 줄의 칸 크기. 줄마다 높이가 다르다.
  const justified = useMemo(() => {
    if (gridStyle !== "justified" || viewW <= 0) return null;
    const out = new Map<number, JustifiedRow<FileRow>[]>();
    grid.forEach((row, i) => {
      if (row.kind !== "photos") return;
      out.set(
        i,
        justify(
          row.items,
          (f) => ratio(f.width, f.height),
          viewW,
          thumbSize,
          GAP,
        ),
      );
    });
    return out;
  }, [grid, gridStyle, viewW, thumbSize]);
  const virt = useVirtualizer({
    count: grid.length,
    getScrollElement: () => scrollRef.current,
    // 머리글과 사진 줄은 높이가 다르다
    estimateSize: (i) => {
      if (grid[i]?.kind === "header") return HEADER_H;
      const j = justified?.get(i);
      // 양쪽 맞춤은 한 「줄」이 여러 소줄로 나뉜다
      if (j) return j.reduce((a, r) => a + r.height + GAP, 0);
      return rowH;
    },
    overscan: 4,
  });

  // 끝에 가까워지면 다음 페이지
  useEffect(() => {
    const items = virt.getVirtualItems();
    const last = items[items.length - 1];
    if (last && last.index >= grid.length - 3) loadMore();
  }, [virt.getVirtualItems(), grid.length, loadMore]);

  const addLibrary = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    try {
      const l = await invoke<Library>("library_add", { path: picked, area: 1 });
      await refreshLibs();
      setLibId(l.id);
      setSel(null);
      setOpen(new Set());
      // rescan()을 쓰지 않는다 — 방금 등록한 것이 아직 libs 상태에 없어
      // "연결된 디스크가 없습니다"로 걸린다. 어차피 방금 고른 폴더다.
      queue.current = [];
      await invoke("scan_start", { libraryId: l.id });
      setScanMsg("스캔 시작…");
    } catch (e) {
      setScanMsg(String(e));
    }
  };

  /// 고른 라이브러리를 다시 훑는다. 「전체」를 보고 있으면 연결된 것을
  /// 하나씩 차례로 — 예전에는 libs[0]만 훑어서 두 번째 라이브러리는
  /// 아무리 눌러도 썸네일이 생기지 않았다.
  const rescan = useCallback(
    async (ids: number[]) => {
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
    },
    [libs],
  );

  /// 도는 일을 멈춘다. 스캔·썸네일·화질 올리기가 같은 스위치를 본다.
  /// 진행은 500장마다 저장돼 있어 멈춰도 지금까지 한 것은 남는다.
  const stopJob = useCallback(async () => {
    await invoke("scan_cancel");
    queue.current = [];
    setJob(null);
    setScanMsg("멈췄습니다 — 지금까지 한 것은 저장돼 있습니다");
    await Promise.all([refreshMeta(), refreshLibs()]);
  }, [refreshMeta, refreshLibs]);

  /// 등록을 지우면 그 라이브러리의 폴더·파일 기록이 CASCADE로 전부 사라진다.
  /// 원본 사진은 그대로지만 스캔은 처음부터 다시 해야 한다. 실제로 ⟳ 바로 옆에
  /// 붙어 있다가 잘못 눌려 6만 장짜리 라이브러리가 통째로 날아간 적이 있다.
  const dropLibrary = async (l: Library) => {
    const ok = window.confirm(
      `「${l.name}」 등록을 지웁니다.\n\n` +
        `사진 ${l.file_count.toLocaleString()}장의 기록과 판정·평점이 사라지고, ` +
        `다시 등록하면 처음부터 스캔해야 합니다.\n` +
        `원본 사진과 썸네일 파일은 지워지지 않습니다.`,
    );
    if (!ok) return;
    await invoke("library_remove", { id: l.id });
    if (libId === l.id) setLibId(null);
    setSel(null);
    setOpen(new Set());
    await refreshLibs();
    loadFirst();
    refreshMeta();
  };

  /// 스크롤바가 준 전역 순번으로 목록을 다시 읽는다.
  ///
  /// 손잡이를 끌면 요청이 초당 수십 번 쏟아진다. 하나가 도는 동안 들어온 것은
  /// `pending`에 덮어써 두고, 끝나면 **마지막 것만** 이어서 처리한다. 큐에
  /// 쌓아 두면 손을 뗀 뒤에도 한참 따라온다.
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
          scrollRef.current?.scrollTo({ top: 0 });
          setScrollTop(0);
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

  /// 고른 것들의 배열. `[...picked]`를 그대로 넘기면 렌더마다 새 배열이라
  /// 정리 패널의 제안 요청이 끝없이 다시 돈다.
  const pickedIds = useMemo(() => [...picked], [picked]);

  /// 뷰어가 훑고 다닐 목록 — 지금 화면에 올라온 순서 그대로
  const ids = useMemo(() => rows.map((r) => r.id), [rows]);

  /// 뷰어에서 판정을 바꾸면 그리드도 같이 바뀌어야 한다
  const markOne = useCallback(
    async (
      id: number,
      patch: { rating?: number; cullingFlag?: number; favorite?: boolean },
    ) => {
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
    },
    [],
  );

  // 뷰어가 끝에 다다르면 다음 페이지를 미리 읽는다
  useEffect(() => {
    if (viewerAt !== null && viewerAt >= rows.length - 5) loadMore();
  }, [viewerAt, rows.length, loadMore]);

  /// 접힌 마디의 자식은 그리지 않는다. 셈은 folderTree.ts에 있다.
  const visibleFolders = useMemo(() => visible(folders, open), [folders, open]);

  const toggleOpen = useCallback((path: string) => {
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  /// 타일을 누를 때. ⌘은 하나씩 더하고, ⇧는 기준점부터 여기까지.
  const pick = useCallback(
    (id: number, e: React.MouseEvent) => {
      if (e.metaKey || e.ctrlKey) {
        setPicked((prev) => {
          const next = new Set(prev);
          if (next.has(id)) next.delete(id);
          else next.add(id);
          return next;
        });
        setSelected(id);
        return;
      }
      if (e.shiftKey && selected !== null) {
        const a = rows.findIndex((r) => r.id === selected);
        const b = rows.findIndex((r) => r.id === id);
        if (a >= 0 && b >= 0) {
          const [lo, hi] = a < b ? [a, b] : [b, a];
          setPicked(new Set(rows.slice(lo, hi + 1).map((r) => r.id)));
          setSelected(id);
          return;
        }
      }
      setPicked(new Set([id]));
      setSelected(id);
    },
    [rows, selected],
  );

  const refreshBatches = useCallback(async () => {
    try {
      setBatches(await invoke<Batch[]>("batches_recent", { limit: 20 }));
    } catch {
      /* 아직 아무 작업도 없다 */
    }
  }, []);

  /// 가장 최근의 아직 안 물린 작업을 되돌린다 (⌘Z).
  const undoLast = useCallback(async () => {
    const last = batches.find((b) => b.undone_at === null);
    if (!last) return;
    setBusy("되돌리는 중…");
    try {
      const r = await invoke<Outcome>("batch_undo", { batchId: last.id });
      setBusy(
        r.failed > 0 ? `${r.failed}장 실패 — ${r.first_error ?? ""}` : "",
      );
      await Promise.all([
        loadFirst(),
        refreshMeta(),
        refreshLibs(),
        refreshBatches(),
      ]);
    } catch (e) {
      setBusy(String(e));
    }
  }, [batches, loadFirst, refreshMeta, refreshLibs, refreshBatches]);

  // 그리드 키보드 — 뷰어를 열지 않고도 판정하고 옮겨 다닌다.
  // 뷰어와 같은 배열이라 손이 기억한 대로 눌린다.
  useEffect(() => {
    if (viewerAt !== null || culling || organizing) return;
    const onKey = (e: KeyboardEvent) => {
      // 찾기 입력칸에 쓰는 중이면 가로채지 않는다
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;

      const i =
        selected === null ? -1 : rows.findIndex((r) => r.id === selected);
      const move = (d: number) => {
        const n = i < 0 ? 0 : i + d;
        if (n < 0 || n >= rows.length) return;
        e.preventDefault();
        setSelected(rows[n].id);
        // ⇧를 잡고 움직이면 묶음이 늘어난다
        setPicked((prev) =>
          e.shiftKey ? new Set([...prev, rows[n].id]) : new Set([rows[n].id]),
        );
      };
      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        undoLast();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "a") {
        e.preventDefault();
        setPicked(new Set(rows.map((r) => r.id)));
        return;
      }
      if (e.key === "Escape" && picked.size > 0) {
        e.preventDefault();
        setPicked(new Set());
        return;
      }
      switch (e.key) {
        case " ":
        case "Enter":
          if (i < 0) return;
          e.preventDefault();
          setViewerAt(i);
          return;
        case "ArrowRight":
          return move(1);
        case "ArrowLeft":
          return move(-1);
        case "ArrowDown":
          return move(cols);
        case "ArrowUp":
          return move(-cols);
      }
      if (i < 0) return;
      const r = rows[i];
      if (/^[0-5]$/.test(e.key)) markOne(r.id, { rating: +e.key });
      else if (e.key === "p")
        markOne(r.id, { cullingFlag: r.culling_flag === 1 ? 0 : 1 });
      else if (e.key === "x")
        markOne(r.id, { cullingFlag: r.culling_flag === 2 ? 0 : 2 });
      else if (e.key === "f") markOne(r.id, { favorite: !r.favorite });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    viewerAt,
    culling,
    organizing,
    selected,
    rows,
    cols,
    picked.size,
    markOne,
    undoLast,
  ]);

  /// 되돌릴 수 없는 일은 여기서 확인 문구를 만들고, 프론트가 물어본 뒤에만 부른다.
  const runTrashOp = useCallback(
    async (cmd: string, args: Record<string, unknown>, doing: string) => {
      setBusy(doing);
      try {
        const r = await invoke<Outcome>(cmd, args);
        setBusy(
          r.failed > 0
            ? `${r.moved}장 처리 · ${r.failed}장 실패 — ${r.first_error ?? ""}`
            : "",
        );
        await Promise.all([
          loadFirst(),
          refreshMeta(),
          refreshLibs(),
          refreshTrash(),
        ]);
      } catch (e) {
        setBusy(String(e));
      }
    },
    [loadFirst, refreshMeta, refreshLibs, refreshTrash],
  );

  /// 제외로 판정한 것을 휴지통으로. 파일은 라이브러리 안 `.acut/휴지통`으로
  /// 옮겨질 뿐이라 되돌릴 수 있다.
  const cleanExcluded = useCallback(() => {
    if (!toClean || toClean.files === 0) return;
    if (
      !window.confirm(
        `제외로 판정한 ${toClean.files.toLocaleString()}장(${fmtBytes(toClean.bytes)})을 ` +
          `휴지통으로 옮깁니다.\n\n파일은 라이브러리 안 .acut/휴지통 으로 이동하며 ` +
          `언제든 되돌릴 수 있습니다.`,
      )
    )
      return;
    runTrashOp("trash_apply", { libraryId: libId }, "치우는 중…");
  }, [toClean, libId, runTrashOp]);

  const emptyTrash = useCallback(() => {
    if (!trash || trash.files === 0) return;
    if (
      !window.confirm(
        `휴지통의 ${trash.files.toLocaleString()}장(${fmtBytes(trash.bytes)})을 ` +
          `영구히 지웁니다.\n\n되돌릴 수 없습니다.`,
      )
    )
      return;
    runTrashOp("trash_empty", { libraryId: libId, ids: [] }, "지우는 중…");
  }, [trash, libId, runTrashOp]);

  /// 고른 것 전부에 같은 판정을 준다.
  const markPicked = useCallback(
    (patch: Parameters<typeof markOne>[1]) => {
      picked.forEach((id) => markOne(id, patch));
    },
    [picked, markOne],
  );

  /// 타일 우클릭. 고른 것 밖을 우클릭하면 그것 하나만 대상으로 삼는다 —
  /// 안 그러면 눈에 안 보이는 선택에 대고 일이 벌어진다.
  const openContext = useCallback(
    (id: number, e: React.MouseEvent) => {
      e.preventDefault();
      const target = picked.has(id) ? [...picked] : [id];
      if (!picked.has(id)) {
        setPicked(new Set([id]));
        setSelected(id);
      }
      setCtxIds(target);
      setCtxAt({ x: e.clientX, y: e.clientY });
    },
    [picked],
  );

  const ctxItems: CtxItem[] = useMemo(() => {
    const n = ctxIds.length;
    const many = n > 1 ? ` ${n.toLocaleString()}장` : "";
    const mark = (patch: Parameters<typeof markOne>[1]) => () =>
      ctxIds.forEach((id) => markOne(id, patch));
    return [
      {
        kind: "item",
        label: "크게 보기",
        hint: "Space",
        run: () => {
          const i = rows.findIndex((r) => r.id === ctxIds[0]);
          if (i >= 0) setViewerAt(i);
        },
      },
      { kind: "sep" },
      {
        kind: "item",
        label: `남김으로${many}`,
        hint: "P",
        run: mark({ cullingFlag: 1 }),
      },
      {
        kind: "item",
        label: `제외로${many}`,
        hint: "X",
        run: mark({ cullingFlag: 2 }),
      },
      {
        kind: "item",
        label: "판정 지우기",
        hint: "0",
        run: mark({ cullingFlag: 0 }),
      },
      {
        kind: "item",
        label: "즐겨찾기",
        hint: "F",
        run: mark({ favorite: true }),
      },
      { kind: "sep" },
      {
        kind: "item",
        label: `정리하기${many}`,
        run: () => {
          setPicked(new Set(ctxIds));
          setOrganizing(true);
        },
      },
      {
        kind: "item",
        label: `휴지통으로 보내기${many}`,
        danger: true,
        run: () => {
          if (
            !window.confirm(
              `${n.toLocaleString()}장을 휴지통으로 옮깁니다.\n되돌릴 수 있습니다.`,
            )
          )
            return;
          runTrashOp("trash_files", { ids: ctxIds }, "치우는 중…");
        },
      },
      { kind: "sep" },
      {
        kind: "item",
        label: "Finder에서 보기",
        run: () => {
          invoke("reveal_in_finder", { id: ctxIds[0] }).catch((e) =>
            setBusy(String(e)),
          );
        },
      },
    ];
  }, [ctxIds, rows, markOne, runTrashOp]);

  /// 상태바에 띄울 지금 사진의 카메라·설정. 상세는 따로 읽는다.
  const [focusExif, setFocusExif] = useState<{
    camModel: string | null;
    lens: string | null;
    settings: string;
  } | null>(null);

  useEffect(() => {
    if (selected === null) {
      setFocusExif(null);
      return;
    }
    let live = true;
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
        setFocusExif({
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
      .catch(() => live && setFocusExif(null));
    return () => {
      live = false;
    };
  }, [selected]);

  /// 지금 고른 사진과 그 순번 — 상태바가 쓴다
  const focusAt = useMemo(
    () => (selected === null ? -1 : rows.findIndex((r) => r.id === selected)),
    [rows, selected],
  );

  /// 갈래 목록을 셀 때 쓰는 필터. 그 갈래 자신은 빼야 다른 값도 보인다.
  const facetFilter = useMemo(
    () => ({
      ...filter,
      year: null,
      month: null,
      camera: null,
      min_rating: null,
      tag_id: null,
      place: null,
    }),
    [filter],
  );

  /// 달력이 쓸 눈금 — **날짜 조건을 뺀** 필터로 읽는다.
  /// 그리드용 buckets를 그대로 쓰면 2024년을 고른 순간 목록에 2024년만
  /// 남아 다른 해로 갈 수가 없다.
  const [calBuckets, setCalBuckets] = useState<Bucket[]>([]);
  useEffect(() => {
    if (source !== "calendar") return;
    invoke<Bucket[]>("files_timeline", { filter: facetFilter })
      .then(setCalBuckets)
      .catch(() => setCalBuckets([]));
  }, [source, facetFilter]);

  /// 찾기 결과 개수 — 눈금 합이 곧 필터에 걸린 장수라 따로 세지 않는다
  const matched = useMemo(
    () => buckets.reduce((a, b) => a + b.count, 0),
    [buckets],
  );

  /// 스크롤바가 알아야 하는 두 값 — 지금 맨 위 사진의 전역 순번과 한 화면 장수
  const offset = baseIndex + Math.floor(scrollTop / rowH) * cols;
  const pageSize = Math.max(cols, Math.ceil(viewH / rowH) * cols);

  // 전용 thumb:// 프로토콜 — 캐시 폴더만 서빙한다 (api/thumb_protocol.rs)
  // 캐시가 라이브러리마다 따로 있어 주소 앞에 라이브러리 id가 붙는다
  const thumbUrl = (r: FileRow) =>
    r.thumb && r.library_id !== null
      ? `thumb://localhost/${r.library_id}/${r.thumb.split("/").map(encodeURIComponent).join("/")}`
      : null;

  return (
    <div className="h-screen flex flex-col bg-canvas text-fg text-[13px]">
      {/* 툴바 — 한 줄로 모은다.
          왼쪽은 "지금 어디를 보고 있나", 오른쪽은 "어떻게 볼까".
          진행 상황과 판정 처리는 아래 상태바로 내렸다 — 위에 줄이 셋이나
          쌓여 사진이 밀려나 있었다. */}
      <div className="h-12 shrink-0 flex items-center gap-2 px-3 bg-chrome border-b border-line">
        <Breadcrumb
          libs={libs}
          libId={libId}
          folder={sel?.path ?? null}
          viewTrash={viewTrash}
          matched={matched}
        />

        <div className="flex-1" />

        {libs.length > 0 && (
          <>
            <FilterButton value={picks} onChange={setPicks} />
            <SortMenu value={sort} onChange={setSort} />
            <GroupMenu value={group} onChange={setGroup} />
            <ViewMenu
              style={gridStyle}
              scaling={scaling}
              onStyle={setGridStyle}
              onScaling={setScaling}
              filmstrip={filmstrip}
              onFilmstrip={setFilmstrip}
            />
            <input
              type="range"
              min={100}
              max={320}
              value={thumbSize}
              onChange={(e) => setThumbSize(+e.target.value)}
              title="썸네일 크기"
              className="w-20 accent-accent"
            />
            <Sep />
            <Btn tone="keep" onClick={() => setCulling(true)}>
              고르기
            </Btn>
          </>
        )}
        <Menu
          align="right"
          trigger={() => <IconBtn title="더 보기">⋯</IconBtn>}
          width={190}
        >
          {(close) => (
            <>
              <MenuItem
                onClick={() => {
                  close();
                  addLibrary();
                }}
              >
                라이브러리 추가…
              </MenuItem>
              <MenuItem
                onClick={() => {
                  close();
                  rescan(libId !== null ? [libId] : libs.map((l) => l.id));
                }}
              >
                다시 스캔
              </MenuItem>
              <MenuSep />
              <div className="px-3 py-1.5 text-[11.5px] text-fg-faint tabular-nums">
                {scanMsg || `캐시 ${fmtBytes(cache?.bytes ?? 0)}`}
              </div>
            </>
          )}
        </Menu>
      </div>

      <div className="flex-1 flex min-h-0">
        {/* 레일 — 사이드바가 무엇을 보여줄지 고른다 */}
        <Rail
          value={source}
          open={panelOpen}
          trashCount={trash?.files ?? 0}
          onPick={(s) => {
            // 같은 갈래를 다시 누르면 접힌다 — 사진을 넓게 보고 싶을 때
            if (s === source && panelOpen) {
              setPanelOpen(false);
              return;
            }
            setSource(s);
            setPanelOpen(true);
            setViewTrash(s === "trash");
            if (s !== "trash") setViewTrash(false);
          }}
        />

        {panelOpen && (
          <aside
            className="shrink-0 bg-chrome border-r border-line overflow-y-auto py-2"
            style={{ width: panelW }}
          >
            <Label>{sourceTitle(source)}</Label>
            {source === "all" && (
              <>
                <QuickRow
                  label="모든 사진"
                  count={libs.reduce((a, l) => a + l.file_count, 0)}
                  on={libId === null && sel === null && isEmptyPicks(picks)}
                  onClick={() => {
                    setLibId(null);
                    setSel(null);
                    setPicks(EMPTY_PICKS);
                    setViewTrash(false);
                  }}
                />
                <QuickRow
                  label="♥ 즐겨찾기"
                  on={picks.favorite_only}
                  onClick={() =>
                    setPicks({
                      ...EMPTY_PICKS,
                      favorite_only: !picks.favorite_only,
                    })
                  }
                />
                <QuickRow
                  label="영상"
                  on={picks.kind === 1}
                  onClick={() =>
                    setPicks({
                      ...EMPTY_PICKS,
                      kind: picks.kind === 1 ? null : 1,
                    })
                  }
                />
                <QuickRow
                  label="RAW"
                  on={picks.kind === 2}
                  onClick={() =>
                    setPicks({
                      ...EMPTY_PICKS,
                      kind: picks.kind === 2 ? null : 2,
                    })
                  }
                />
                <QuickRow
                  label="★ 4개 이상"
                  on={picks.min_rating === 4}
                  onClick={() =>
                    setPicks({
                      ...EMPTY_PICKS,
                      min_rating: picks.min_rating === 4 ? null : 4,
                    })
                  }
                />
              </>
            )}
            {source === "album" && (
              <>
                {folders.length === 0 ? (
                  <div className="px-3 py-2 text-[12px] text-fg-mute leading-relaxed">
                    등록된 라이브러리가 없습니다.
                    <br />
                    아래 「라이브러리 추가」를 누르세요.
                  </div>
                ) : (
                  visibleFolders.map((f) => {
                    const root = f.is_library;
                    const on = root
                      ? sel === null && libId === f.library_id
                      : sel?.path === f.path;
                    const lib = root
                      ? libs.find((l) => l.id === f.library_id)
                      : undefined;
                    return (
                      <div
                        key={f.path}
                        className={`group relative flex items-center pr-1 ${
                          on ? "bg-raised" : "hover:bg-chrome"
                        }`}
                        style={{ paddingLeft: 4 + f.depth * 11 }}
                      >
                        {/* 펼침 삼각형 — 자식이 없으면 자리만 차지한다 */}
                        <button
                          onClick={() => f.has_children && toggleOpen(f.path)}
                          className={`w-4 shrink-0 text-[9px] ${
                            f.has_children
                              ? "text-fg-mute hover:text-fg"
                              : "text-transparent"
                          }`}
                        >
                          {open.has(f.path) ? "▼" : "▶"}
                        </button>
                        <button
                          onClick={() => {
                            setLibId(f.library_id);
                            // 라이브러리 마디를 누르면 그 라이브러리 전체
                            setSel(
                              root
                                ? null
                                : {
                                    libId: f.library_id,
                                    path: f.path,
                                    rel: f.rel_path,
                                  },
                            );
                            setViewTrash(false);
                          }}
                          title={f.rel_path || f.name}
                          className={`flex-1 min-w-0 text-left py-1 truncate ${
                            root ? "font-semibold" : ""
                          } ${on ? "text-fg" : "text-fg-dim"} ${
                            lib && !lib.online ? "opacity-50" : ""
                          }`}
                        >
                          {root && (
                            <span
                              className="inline-block w-1.5 h-1.5 rounded-full mr-1.5 align-middle"
                              style={{
                                background: lib?.online
                                  ? "var(--color-accent)"
                                  : "var(--color-fg-faint)",
                              }}
                            />
                          )}
                          {f.name}
                        </button>
                        <span
                          className={`text-fg-mute tabular-nums text-[11px] shrink-0 pl-1.5 ${
                            root ? "group-hover:invisible" : ""
                          }`}
                        >
                          {f.file_count.toLocaleString()}
                        </span>

                        {/* 라이브러리 줄에만 붙는 조작.
                            ⟳ 옆에 지우기를 두지 않는다 — 실제로 잘못 눌려
                            라이브러리가 통째로 날아갔다. */}
                        {root && lib && (
                          <div className="absolute right-1 hidden group-hover:flex bg-raised rounded">
                            <button
                              onClick={() => rescan([lib.id])}
                              disabled={!lib.online}
                              title="이 라이브러리 다시 스캔"
                              className="px-1.5 text-fg-mute hover:text-accent disabled:opacity-30"
                            >
                              ⟳
                            </button>
                            <button
                              onClick={() =>
                                setMenuFor(menuFor === lib.id ? null : lib.id)
                              }
                              title="더 보기"
                              className="px-1.5 text-fg-mute hover:text-fg"
                            >
                              ⋯
                            </button>
                          </div>
                        )}
                        {root && lib && menuFor === lib.id && (
                          <div className="absolute right-1 top-7 z-20 bg-raised rounded-md ring-1 ring-line-strong shadow-lg py-1">
                            <button
                              onClick={() => {
                                setMenuFor(null);
                                dropLibrary(lib);
                              }}
                              className="block w-full text-left px-3 py-1.5 text-[12px] text-drop hover:bg-hover whitespace-nowrap"
                            >
                              목록에서 빼기
                            </button>
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
                <div className="px-2 pt-2">
                  <Btn onClick={addLibrary}>＋ 라이브러리 추가…</Btn>
                </div>
              </>
            )}
            {source === "calendar" && (
              <Calendar
                buckets={calBuckets}
                year={picks.year}
                month={picks.month}
                onPick={(y, m) => setPicks({ ...picks, year: y, month: m })}
              />
            )}
            {source === "camera" && (
              <FacetList
                kind="camera"
                filter={facetFilter}
                selected={picks.camera ?? null}
                onPick={(v) => setPicks({ ...picks, camera: v })}
              />
            )}
            {source === "location" && (
              <FacetList
                kind="place"
                filter={facetFilter}
                selected={picks.place}
                onPick={(v) => setPicks({ ...picks, place: v })}
              />
            )}
            {source === "tag" && (
              <TagPanel
                selected={picks.tag_id}
                onPick={(v) => setPicks({ ...picks, tag_id: v })}
                pickedIds={[...picked]}
                onChanged={loadFirst}
              />
            )}
            {source === "smart" && (
              <SmartPanel
                current={filter}
                currentSort={sort}
                hasFilter={!isEmptyPicks(picks)}
                onApply={applySmart}
              />
            )}
            {source === "search" && (
              <SearchPanel
                value={picks}
                onChange={setPicks}
                facetFilter={facetFilter}
              />
            )}
            {source === "settings" && (
              <SettingsPanel
                thumbBytes={cache?.bytes ?? null}
                onRefresh={refreshCache}
              />
            )}
            {source === "trash" && (
              <div className="px-3 py-2 text-[12px] text-fg-dim">
                버린 사진 {(trash?.files ?? 0).toLocaleString()}장
                <br />
                <span className="text-fg-mute">
                  {fmtBytes(trash?.bytes ?? 0)}
                </span>
              </div>
            )}
          </aside>
        )}

        {/* 폭 조절 — 잡고 끌면 사이드바가 넓어진다 */}
        {panelOpen && (
          <div
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId);
              dragPanel.current = true;
            }}
            onPointerMove={(e) => {
              if (!dragPanel.current) return;
              setPanelW(Math.max(160, Math.min(480, e.clientX - 48)));
            }}
            onPointerUp={() => (dragPanel.current = false)}
            className="w-1 shrink-0 cursor-col-resize hover:bg-accent"
          />
        )}

        {/* 콘텐츠 영역 — 뷰어는 이 안만 덮는다. 왼쪽 폴더 목록은 계속 보인다. */}
        <div className="flex-1 flex min-w-0 relative">
          {/* 그리드 */}
          <main
            ref={scrollRef}
            onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
            className="flex-1 overflow-y-auto p-2.5"
          >
            {libs.length === 0 && (
              <div className="h-full flex items-center justify-center text-fg-mute">
                「라이브러리 추가」로 사진 폴더를 등록하세요
              </div>
            )}
            <div style={{ height: virt.getTotalSize(), position: "relative" }}>
              {virt.getVirtualItems().map((v) => {
                const row = grid[v.index];
                if (!row) return null;
                const box = {
                  position: "absolute" as const,
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${v.start}px)`,
                };
                if (row.kind === "header") {
                  return (
                    <div
                      key={v.key}
                      style={{ ...box, height: HEADER_H }}
                      className="flex items-baseline gap-2 px-0.5"
                    >
                      <span className="text-[13px] font-semibold text-fg">
                        {headerLabel(row.label, group)}
                      </span>
                      <span className="text-[11.5px] text-fg-mute tabular-nums">
                        {row.count.toLocaleString()}장
                      </span>
                      <div className="flex-1 h-px bg-line" />
                    </div>
                  );
                }
                const jrows = justified?.get(v.index);
                if (jrows) {
                  // 양쪽 맞춤 — 한 「줄」 안에 여러 소줄이 들어간다
                  let n = row.start;
                  return (
                    <div key={v.key} style={box}>
                      {jrows.map((jr, ri) => (
                        <div
                          key={ri}
                          className="flex"
                          style={{ gap: GAP, marginBottom: GAP }}
                        >
                          {jr.items.map(({ file, width }) => {
                            const at = n++;
                            return (
                              <Tile
                                key={file.id}
                                file={file}
                                url={thumbUrl(file)}
                                picked={picked.has(file.id)}
                                focused={selected === file.id}
                                onClick={(e: React.MouseEvent) =>
                                  pick(file.id, e)
                                }
                                onDoubleClick={() => setViewerAt(at)}
                                onContextMenu={(e: React.MouseEvent) =>
                                  openContext(file.id, e)
                                }
                                label={fmtDate(file.taken_at)}
                                style="justified"
                                scaling={scaling}
                                aspect={{ width, height: jr.height }}
                              />
                            );
                          })}
                        </div>
                      ))}
                    </div>
                  );
                }
                return (
                  <div
                    key={v.key}
                    style={{
                      ...box,
                      height: rowH,
                      display: "grid",
                      gridTemplateColumns: `repeat(${cols}, minmax(0,1fr))`,
                      gap: GAP,
                    }}
                  >
                    {row.items.map((r, ci) => (
                      <Tile
                        key={r.id}
                        file={r}
                        url={thumbUrl(r)}
                        picked={picked.has(r.id)}
                        focused={selected === r.id}
                        onClick={(e: React.MouseEvent) => pick(r.id, e)}
                        onDoubleClick={() => setViewerAt(row.start + ci)}
                        onContextMenu={(e: React.MouseEvent) =>
                          openContext(r.id, e)
                        }
                        label={fmtDate(r.taken_at)}
                        style={gridStyle}
                        scaling={scaling}
                      />
                    ))}
                  </div>
                );
              })}
            </div>
            {loading && (
              <div className="py-4 text-center text-fg-mute">불러오는 중…</div>
            )}
          </main>

          {/* 필름스트립 자리 — 아직 비어 있다 */}
          {filmstrip && selected !== null && (
            <div className="absolute bottom-0 inset-x-0 h-1/3 bg-canvas border-t border-line flex items-center justify-center">
              <span className="text-[12.5px] text-fg-mute">
                필름스트립은 아직 준비 중입니다 — 두 번 눌러 크게 보세요
              </span>
            </div>
          )}

          {/* 타임라인 스크롤바 */}
          <ScrollBar
            buckets={buckets}
            offset={offset}
            pageSize={pageSize}
            onSeek={seekTo}
          />

          {organizing && libId !== null && (
            <Organize
              ids={pickedIds}
              libraryId={libId}
              onDone={async (o) => {
                setBusy(
                  o.failed > 0
                    ? `${o.moved}장 옮김 · ${o.failed}장 실패 — ${o.first_error ?? ""}`
                    : `${o.moved}장 옮겼습니다`,
                );
                setPicked(new Set());
                await Promise.all([
                  loadFirst(),
                  refreshMeta(),
                  refreshLibs(),
                  refreshBatches(),
                ]);
              }}
              onClose={() => setOrganizing(false)}
            />
          )}

          {/* 크게 보기 — 기본은 콘텐츠 영역만 덮는다 */}
          {viewerAt !== null && (
            <Viewer
              ids={ids}
              index={viewerAt}
              onIndex={setViewerAt}
              onClose={() => {
                setViewerAt(null);
                setViewerFull(false);
              }}
              onMark={markOne}
              fullScreen={viewerFull}
              onToggleFullScreen={() => setViewerFull((f) => !f)}
            />
          )}
        </div>
      </div>

      <ContextMenu at={ctxAt} items={ctxItems} onClose={() => setCtxAt(null)} />

      {culling && libs.length > 0 && (
        <Cull
          onClose={() => {
            setCulling(false);
            loadFirst();
          }}
        />
      )}

      {/* 선택 패널 — 무엇이 골라져 있고 무엇을 할 수 있는지 (Lap의 SelectionPanel) */}
      {picked.size > 0 && !culling && (
        <div className="h-11 shrink-0 flex items-center gap-2 px-3 bg-chrome border-t border-line">
          <span className="text-accent font-semibold tabular-nums text-[13px]">
            {picked.size.toLocaleString()}장 선택
          </span>
          <span className="text-[11.5px] text-fg-mute">
            {fmtBytes(
              rows
                .filter((r) => picked.has(r.id))
                .reduce((a, r) => a + r.size, 0),
            )}
          </span>
          <Sep />
          <PanelBtn onClick={() => markPicked({ cullingFlag: 1 })} hint="P">
            남김
          </PanelBtn>
          <PanelBtn onClick={() => markPicked({ cullingFlag: 2 })} hint="X">
            제외
          </PanelBtn>
          <PanelBtn onClick={() => markPicked({ favorite: true })} hint="F">
            즐겨찾기
          </PanelBtn>
          <div className="flex items-center gap-0.5 px-1">
            {[1, 2, 3, 4, 5].map((n) => (
              <button
                key={n}
                onClick={() => markPicked({ rating: n })}
                title={`별 ${n}개`}
                className="w-5 h-6 text-[13px] text-fg-faint hover:text-keep"
              >
                ★
              </button>
            ))}
          </div>
          <Sep />
          <button
            onClick={() => setOrganizing(true)}
            disabled={libId === null}
            title={
              libId === null
                ? "옮겨 넣을 라이브러리를 왼쪽에서 고르세요"
                : undefined
            }
            className="h-control px-3 rounded-md bg-accent text-accent-fg font-semibold text-[12.5px] disabled:opacity-40"
          >
            정리
          </button>
          <button
            onClick={() => {
              if (
                !window.confirm(
                  `${picked.size.toLocaleString()}장을 휴지통으로 옮깁니다.\n되돌릴 수 있습니다.`,
                )
              )
                return;
              runTrashOp("trash_files", { ids: [...picked] }, "치우는 중…");
              setPicked(new Set());
            }}
            className="h-control px-3 rounded-md text-drop ring-1 ring-drop text-[12.5px]"
          >
            휴지통으로
          </button>
          <div className="flex-1" />
          <button
            onClick={() => setPicked(new Set())}
            className="h-control px-2 rounded-md text-fg-dim text-[12.5px]"
          >
            선택 해제 <span className="text-[10px] font-mono">Esc</span>
          </button>
        </div>
      )}

      {/* 상태바 — 지금 보고 있는 사진의 정보 (Lap의 StatusBar 구성).
          오른쪽에는 벌어지는 일과 되돌리기. */}
      <StatusBar
        index={focusAt >= 0 ? baseIndex + focusAt : -1}
        total={matched || (stats?.files ?? 0)}
        totalBytes={stats?.bytes ?? 0}
        file={focusAt >= 0 ? rows[focusAt] : null}
        exif={focusExif}
      >
        {busy && <span className="text-keep">{busy}</span>}
        {job && (
          <>
            <Progress label={job.label} done={job.done} total={job.total} />
            <button
              onClick={stopJob}
              title="지금까지 한 것은 저장됩니다"
              className="h-5 px-2 rounded text-drop ring-1 ring-drop/40 hover:bg-drop/10"
            >
              멈추기
            </button>
          </>
        )}
        {!job && viewTrash && (
          <>
            <button
              onClick={() =>
                runTrashOp(
                  "trash_restore",
                  { libraryId: libId, ids: [] },
                  "되돌리는 중…",
                )
              }
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
        {!job && !viewTrash && (toClean?.files ?? 0) > 0 && (
          <button
            onClick={cleanExcluded}
            title={`제외 ${toClean?.files.toLocaleString()}장 · ${fmtBytes(toClean?.bytes ?? 0)} 확보`}
            className="h-5 px-2 rounded bg-keep text-keep-fg font-semibold"
          >
            제외 {toClean?.files.toLocaleString()}장 치우기
          </button>
        )}
        {batches.some((b) => b.undone_at === null) && (
          <button
            onClick={undoLast}
            title={`되돌리기: ${batches.find((b) => b.undone_at === null)?.label ?? ""}`}
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
      </StatusBar>
    </div>
  );
}

/// 진행 표시 — 숫자가 한 칸씩 올라간다.
function Progress({
  label,
  done,
  total,
}: {
  label: string;
  done: number;
  total: number;
}) {
  const n = useCountUp(done);
  return (
    <span className="text-keep tabular-nums">
      {label} {n.toLocaleString()}
      {total > 0 && ` / ${total.toLocaleString()}`}
    </span>
  );
}

/// 선택 패널의 작은 버튼
function PanelBtn({
  children,
  hint,
  onClick,
}: {
  children: React.ReactNode;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="h-control px-2.5 rounded-md text-[12.5px] text-fg-dim ring-1 ring-line-strong hover:text-white"
    >
      {children}
      {hint && (
        <span className="ml-1 text-[10px] font-mono text-fg-mute">{hint}</span>
      )}
    </button>
  );
}

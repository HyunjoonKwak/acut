import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type {
  Batch,
  Bucket,
  CacheUsage,
  Counted,
  TrashByLib,
  FolderRow,
  Library,
  Stats,
} from "./types";
import type { Filter } from "./viewStore";

/**
 * 서버에서 읽어 오는 것들 — 라이브러리·통계·폴더·태그·휴지통·작업 묶음.
 *
 * 전부 «다시 읽기» 하나로 갱신된다. 컴포넌트는 값을 구독하고, 훅이 때맞춰
 * refresh를 부른다. 언제 읽는지는 부르는 쪽이 정한다 — 캐시 용량처럼
 * 디스크를 훑는 것은 아무 데서나 부르면 안 된다.
 */
type Store = {
  libs: Library[];
  stats: Stats | null;
  /** 현재 필터에 실제로 걸린 파일 수와 용량. 라이브러리 전체 통계와 분리한다. */
  summary: Counted | null;
  cache: CacheUsage | null;
  /** 휴지통에 든 것 / 제외 판정만 하고 아직 안 치운 것 */
  trash: Counted | null;
  toClean: Counted | null;
  /** 라이브러리마다의 휴지통 — 고른 것만 보면 다른 쪽을 빠뜨린다 */
  trashByLib: TrashByLib[];
  /** 모든 라이브러리에서 제외 표시만 하고 아직 휴지통에 안 보낸 것 — 고르기 머리에 보인다 */
  toCleanAll: Counted | null;
  /** 지금 스위치를 쥔 작업 이름 — 진행 숫자가 없어도 칩에 «~ 중»으로 보인다 */
  holder: string | null;
  setHolder: (h: string | null) => void;
  /** 지명이 바뀐 횟수 — 위치 갈래가 이 값을 보고 다시 센다 (2026-09-01 리뷰) */
  geoRev: number;
  bumpGeo: () => void;
  batches: Batch[];
  buckets: Bucket[];
  folders: FolderRow[];
  /** 펼쳐 둔 마디들 (라이브러리 기준 경로) */
  open: Set<string>;
  /** 태그 id → 이름. 조건 칩이 「3번 태그」가 아니라 「가족」이라 쓰려면 필요하다 */
  tags: Map<number, string>;
  /** 상태바 왼쪽 오른쪽 끝에 잠깐 뜨는 말 */
  /** 마지막으로 살핀 NAS 연결 상태 — 툴바 불 */
  nasStatus: {
    online: boolean;
    hostname: string;
    error: string | null;
    at: number;
  } | null;
  /** NAS 1차 구역에 받은 적 없는 사진 — 상태바 알림 */
  nasNew: { libraryId: number; files: number; bytes: number } | null;
  busy: string;
  scanMsg: string;

  refreshLibs: () => Promise<void>;
  /** 캐시 용량은 디스크의 파일 12만 개를 훑는다. 시작할 때와 썸네일이 끝났을 때만. */
  refreshCache: () => Promise<void>;
  refreshBatches: () => Promise<void>;
  refreshTags: () => Promise<void>;
  /** 휴지통과 「치울 것」 개수. 판정을 바꿀 때마다 달라진다. */
  refreshTrash: (libId: number | null) => Promise<void>;
  /** 전체 통계·현재 필터 요약·타임라인. 한꺼번에 던져 대기 시간을 겹친다. */
  refreshMeta: (filter: Filter, libId: number | null) => Promise<void>;
  /** 폴더 트리 — 등록된 라이브러리 전부를 한 번에. 라이브러리 마디는 펴 둔다. */
  loadFolders: () => Promise<void>;
  toggleOpen: (path: string) => void;
  setOpen: (open: Set<string>) => void;
  setBusy: (s: string) => void;
  setNasStatus: (
    s: {
      online: boolean;
      hostname: string;
      error: string | null;
      at: number;
    } | null,
  ) => void;
  setNasNew: (
    n: { libraryId: number; files: number; bytes: number } | null,
  ) => void;
  setScanMsg: (s: string) => void;
};

let refreshMetaGeneration = 0;

export const useData = create<Store>()((set, get) => ({
  libs: [],
  nasStatus: null,
  nasNew: null,
  stats: null,
  summary: null,
  cache: null,
  trash: null,
  toClean: null,
  trashByLib: [],
  toCleanAll: null,
  holder: null,
  setHolder: (holder) => set({ holder }),
  geoRev: 0,
  bumpGeo: () => set((s) => ({ geoRev: s.geoRev + 1 })),
  batches: [],
  buckets: [],
  folders: [],
  open: new Set(),
  tags: new Map(),
  busy: "",
  scanMsg: "",

  refreshLibs: async () => {
    set({ libs: await invoke<Library[]>("libraries_list") });
  },
  refreshCache: async () => {
    try {
      set({
        cache: await invoke<CacheUsage>("cache_usage", { libraryId: null }),
      });
    } catch {
      /* 디스크가 빠져 있을 수 있다 */
    }
  },
  refreshBatches: async () => {
    try {
      set({ batches: await invoke<Batch[]>("batches_recent", { limit: 20 }) });
    } catch {
      /* 아직 아무 작업도 없다 */
    }
  },
  refreshTags: async () => {
    try {
      const t = await invoke<{ id: number; name: string }[]>("tags_list");
      set({ tags: new Map(t.map((x) => [x.id, x.name])) });
    } catch {
      set({ tags: new Map() });
    }
  },
  refreshTrash: async (libId) => {
    try {
      const [trash, toClean, trashByLib, toCleanAll] = await Promise.all([
        invoke<Counted>("trash_summary", { libraryId: libId }),
        invoke<Counted>("trash_pending", { libraryId: libId }),
        invoke<TrashByLib[]>("trash_by_library"),
        invoke<Counted>("trash_pending", { libraryId: null }),
      ]);
      set({ trash, toClean, trashByLib, toCleanAll });
    } catch {
      /* 아직 라이브러리가 없을 수 있다 */
    }
  },
  refreshMeta: async (filter, libId) => {
    const generation = ++refreshMetaGeneration;
    // 이전 숫자·연표는 새 결과가 올 때까지 그대로 둔다 — 비우면 이동할 때마다
    // 상태바·장수·타임라인이 0 으로 깜빡인다. 늦은 응답은 세대 가드가 버린다.
    try {
      const [stats, summary, buckets] = await Promise.all([
        invoke<Stats>("library_stats", { libraryId: libId }),
        invoke<Counted>("files_summary", { filter }),
        invoke<Bucket[]>("files_timeline", { filter }),
      ]);
      if (generation !== refreshMetaGeneration) return;
      set({ stats, summary, buckets });
      void get().refreshTrash(libId);
      void get().refreshBatches();
    } catch {
      /* 아직 등록된 라이브러리가 없을 수 있다 */
    }
  },
  loadFolders: async () => {
    try {
      const folders = await invoke<FolderRow[]>("folders_list", {
        libraryId: null,
      });
      set((s) => {
        // 라이브러리 마디는 펴 둔다. 접힌 채로 시작하면 등록한 이름만
        // 덩그러니 있고 폴더는 하나도 안 보인다.
        const open = new Set(s.open);
        for (const n of folders) if (n.depth === 0) open.add(n.path);
        return { folders, open };
      });
    } catch {
      set({ folders: [] });
    }
  },
  toggleOpen: (path) =>
    set((s) => {
      const open = new Set(s.open);
      if (open.has(path)) open.delete(path);
      else open.add(path);
      return { open };
    }),
  setOpen: (open) => set({ open }),
  setBusy: (busy) => set({ busy }),
  setNasNew: (n) => set({ nasNew: n }),
  setNasStatus: (s) => set({ nasStatus: s }),
  setScanMsg: (scanMsg) => set({ scanMsg }),
}));

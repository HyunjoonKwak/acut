import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type {
  Batch,
  Bucket,
  CacheUsage,
  Counted,
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
  cache: CacheUsage | null;
  /** 휴지통에 든 것 / 제외 판정만 하고 아직 안 치운 것 */
  trash: Counted | null;
  toClean: Counted | null;
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
  /** 통계와 타임라인 눈금. 둘을 한꺼번에 던진다 — 줄줄이 await하면 시간이 더해진다. */
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

export const useData = create<Store>()((set, get) => ({
  libs: [],
  nasStatus: null,
  nasNew: null,
  stats: null,
  cache: null,
  trash: null,
  toClean: null,
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
      const [trash, toClean] = await Promise.all([
        invoke<Counted>("trash_summary", { libraryId: libId }),
        invoke<Counted>("trash_pending", { libraryId: libId }),
      ]);
      set({ trash, toClean });
    } catch {
      /* 아직 라이브러리가 없을 수 있다 */
    }
  },
  refreshMeta: async (filter, libId) => {
    try {
      const [stats, buckets] = await Promise.all([
        invoke<Stats>("library_stats", { libraryId: libId }),
        invoke<Bucket[]>("files_timeline", { filter }),
      ]);
      set({ stats, buckets });
      get().refreshTrash(libId);
      get().refreshBatches();
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

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import Cull from "./Cull";

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
  /** 캐시 루트 기준 상대경로. null이면 아직 생성 전 */
  thumb: string | null;
};
type Cursor = { taken_at: number; id: number };
type Page = { rows: FileRow[]; next: Cursor | null };
type Library = {
  root: string;
  volume_uuid: string;
  volume_mount: string;
  volume_name: string;
  cache_root: string;
};
type Stats = {
  files: number;
  bytes: number;
  thumbs_done: number;
  thumbs_pending: number;
  cache_bytes: number;
  cache_files: number;
};
type FolderRow = {
  id: number;
  rel_path: string;
  name: string;
  area: number;
  file_count: number;
  depth: number;
};

const PAGE = 300;
const GAP = 10;

const fmtBytes = (n: number) => {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${u[i]}`;
};
const fmtDate = (ts: number) =>
  new Date(ts * 1000).toLocaleDateString("ko-KR", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });

export default function App() {
  const [lib, setLib] = useState<Library | null>(null);
  const [rows, setRows] = useState<FileRow[]>([]);
  const [cursor, setCursor] = useState<Cursor | null>(null);
  const [done, setDone] = useState(false);
  const [loading, setLoading] = useState(false);
  const [stats, setStats] = useState<Stats | null>(null);
  const [folders, setFolders] = useState<FolderRow[]>([]);
  const [folderId, setFolderId] = useState<number | null>(null);
  const [scanMsg, setScanMsg] = useState<string>("");
  const [thumbSize, setThumbSize] = useState(180);
  const [selected, setSelected] = useState<number | null>(null);
  const [culling, setCulling] = useState(false);

  const scrollRef = useRef<HTMLDivElement>(null);
  // 요청이 겹치지 않게 — 스크롤이 빠르면 같은 페이지를 두 번 부를 수 있다
  const inflight = useRef(false);

  const filter = useMemo(() => ({ folder_id: folderId }), [folderId]);

  const loadFirst = useCallback(async () => {
    if (inflight.current) return;
    inflight.current = true;
    setLoading(true);
    try {
      const p = await invoke<Page>("files_page", { filter, cursor: null, limit: PAGE });
      setRows(p.rows);
      setCursor(p.next);
      setDone(!p.next);
    } finally {
      setLoading(false);
      inflight.current = false;
    }
  }, [filter]);

  const loadMore = useCallback(async () => {
    if (inflight.current || done || !cursor) return;
    inflight.current = true;
    try {
      const p = await invoke<Page>("files_page", { filter, cursor, limit: PAGE });
      setRows((prev) => [...prev, ...p.rows]);
      setCursor(p.next);
      setDone(!p.next);
    } finally {
      inflight.current = false;
    }
  }, [filter, cursor, done]);

  const refreshMeta = useCallback(async () => {
    try {
      setStats(await invoke<Stats>("library_stats"));
      setFolders(await invoke<FolderRow[]>("folders_list"));
    } catch {
      /* 라이브러리가 아직 없을 수 있다 */
    }
  }, []);

  // 앱 시작 — 마지막 라이브러리를 되연다
  useEffect(() => {
    (async () => {
      const l = await invoke<Library | null>("library_reopen");
      if (l) setLib(l);
    })();
  }, []);

  // 라이브러리가 바뀌면 목록을 새로 읽는다
  useEffect(() => {
    if (!lib) return;
    setRows([]);
    setCursor(null);
    setDone(false);
    loadFirst();
    refreshMeta();
  }, [lib, folderId, loadFirst, refreshMeta]);

  // 스캔·썸네일 진행 상황
  useEffect(() => {
    const un: Array<() => void> = [];
    listen<{ found: number; inserted: number; skipped: number }>("scan-progress", (e) => {
      const p = e.payload;
      setScanMsg(`스캔 ${p.inserted + p.skipped}/${p.found}`);
    }).then((f) => un.push(f));
    listen("scan-done", () => {
      setScanMsg("스캔 완료 — 썸네일 생성 중");
      loadFirst();
      refreshMeta();
    }).then((f) => un.push(f));
    listen<{ done: number; total: number }>("thumb-progress", (e) => {
      const p = e.payload;
      setScanMsg(`썸네일 ${p.done}/${p.total}`);
    }).then((f) => un.push(f));
    listen("thumb-done", () => {
      setScanMsg("");
      loadFirst();
      refreshMeta();
    }).then((f) => un.push(f));
    return () => un.forEach((f) => f());
  }, [loadFirst, refreshMeta]);

  // ── 가상 스크롤 ──────────────────────────────────────────────────
  const [cols, setCols] = useState(6);
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth - GAP;
      setCols(Math.max(1, Math.floor(w / (thumbSize + GAP))));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [thumbSize]);

  const rowCount = Math.ceil(rows.length / cols);
  const rowH = thumbSize + 26;
  const virt = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowH,
    overscan: 4,
  });

  // 끝에 가까워지면 다음 페이지
  useEffect(() => {
    const items = virt.getVirtualItems();
    const last = items[items.length - 1];
    if (last && last.index >= rowCount - 3) loadMore();
  }, [virt.getVirtualItems(), rowCount, loadMore]);

  const pickFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    const l = await invoke<Library>("library_open", { path: picked });
    setLib(l);
    setFolderId(null);
    await invoke("scan_start", { area: 1 });
    setScanMsg("스캔 시작…");
  };

  // 전용 thumb:// 프로토콜 — 캐시 폴더만 서빙한다 (api/thumb_protocol.rs)
  const thumbUrl = (r: FileRow) =>
    r.thumb ? `thumb://localhost/${r.thumb.split("/").map(encodeURIComponent).join("/")}` : null;

  return (
    <div className="h-screen flex flex-col bg-[#15191A] text-[#EAEFEF] text-[13px]">
      {/* 툴바 */}
      <div className="h-11 shrink-0 flex items-center gap-3 px-3 bg-[#1C2123] border-b border-[#242C2E]">
        <button
          onClick={pickFolder}
          className="h-7 px-3 rounded-md bg-[#49B8B4] text-[#08191a] font-semibold"
        >
          라이브러리 열기
        </button>
        {lib && (
          <>
            <span className="text-[#A3B2B4]">{lib.root}</span>
            <button
              onClick={async () => {
                await invoke("scan_start", { area: 1 });
                setScanMsg("스캔 시작…");
              }}
              className="h-7 px-3 rounded-md text-[#A3B2B4] ring-1 ring-[#333C3F]"
            >
              다시 스캔
            </button>
            <button
              onClick={() => setCulling(true)}
              className="h-7 px-3 rounded-md bg-[#F0B429] text-[#231A00] font-semibold"
            >
              고르기
            </button>
          </>
        )}
        <div className="flex-1" />
        {scanMsg && <span className="text-[#F0B429] tabular-nums">{scanMsg}</span>}
        <input
          type="range"
          min={100}
          max={320}
          value={thumbSize}
          onChange={(e) => setThumbSize(+e.target.value)}
          className="w-28"
        />
      </div>

      <div className="flex-1 flex min-h-0">
        {/* 사이드바 */}
        <aside className="w-56 shrink-0 bg-[#1C2123] border-r border-[#242C2E] overflow-y-auto py-2">
          <button
            onClick={() => setFolderId(null)}
            className={`w-full text-left px-3 py-1.5 ${
              folderId === null ? "bg-[#232A2C] text-white" : "text-[#A3B2B4]"
            }`}
          >
            전체{" "}
            <span className="text-[#6D7B7E] tabular-nums float-right">
              {stats?.files.toLocaleString() ?? "—"}
            </span>
          </button>
          {folders.map((f) => (
            <button
              key={f.id}
              onClick={() => setFolderId(f.id)}
              title={f.rel_path}
              style={{ paddingLeft: 12 + f.depth * 10 }}
              className={`w-full text-left pr-3 py-1 truncate ${
                folderId === f.id ? "bg-[#232A2C] text-white" : "text-[#A3B2B4]"
              }`}
            >
              {f.name}{" "}
              <span className="text-[#5F6C6E] tabular-nums text-[11px]">{f.file_count}</span>
            </button>
          ))}
        </aside>

        {/* 그리드 */}
        <main ref={scrollRef} className="flex-1 overflow-y-auto p-2.5">
          {!lib && (
            <div className="h-full flex items-center justify-center text-[#6D7B7E]">
              라이브러리 폴더를 여세요
            </div>
          )}
          <div style={{ height: virt.getTotalSize(), position: "relative" }}>
            {virt.getVirtualItems().map((v) => {
              const start = v.index * cols;
              const slice = rows.slice(start, start + cols);
              return (
                <div
                  key={v.key}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: rowH,
                    transform: `translateY(${v.start}px)`,
                    display: "grid",
                    gridTemplateColumns: `repeat(${cols}, minmax(0,1fr))`,
                    gap: GAP,
                  }}
                >
                  {slice.map((r) => {
                    const url = thumbUrl(r);
                    return (
                      <button
                        key={r.id}
                        onClick={() => setSelected(r.id)}
                        className="text-left"
                      >
                        <div
                          className="rounded overflow-hidden bg-[#0F1314] relative"
                          style={{
                            aspectRatio: "1/1",
                            boxShadow:
                              selected === r.id ? "0 0 0 2px #6C6CE8" : undefined,
                          }}
                        >
                          {url ? (
                            <img
                              src={url}
                              loading="lazy"
                              decoding="async"
                              className="w-full h-full object-cover"
                            />
                          ) : (
                            <div className="w-full h-full flex items-center justify-center text-[#3A4547] text-[10px]">
                              {r.kind === 1 ? "영상" : "…"}
                            </div>
                          )}
                          {r.kind === 2 && (
                            <span className="absolute top-1 left-1 text-[9px] px-1 rounded bg-black/60 text-[#F0B429]">
                              RAW
                            </span>
                          )}
                        </div>
                        <div className="text-[10.5px] text-[#6D7B7E] mt-1 truncate tabular-nums">
                          {fmtDate(r.taken_at)}
                        </div>
                      </button>
                    );
                  })}
                </div>
              );
            })}
          </div>
          {loading && <div className="py-4 text-center text-[#6D7B7E]">불러오는 중…</div>}
        </main>
      </div>

      {culling && lib && (
        <Cull onClose={() => { setCulling(false); loadFirst(); }} />
      )}

      {/* 상태바 */}
      <div className="h-7 shrink-0 flex items-center gap-4 px-3 bg-[#1C2123] border-t border-[#242C2E] text-[11.5px] text-[#7C8A8D] tabular-nums">
        {stats && (
          <>
            <span>
              {stats.files.toLocaleString()}장 · {fmtBytes(stats.bytes)}
            </span>
            <span>
              썸네일 {stats.thumbs_done.toLocaleString()}
              {stats.thumbs_pending > 0 && (
                <span className="text-[#F0B429]"> · 대기 {stats.thumbs_pending.toLocaleString()}</span>
              )}
            </span>
            <span className="text-[#5F6C6E]">캐시 {fmtBytes(stats.cache_bytes)}</span>
          </>
        )}
        <div className="flex-1" />
        <span>표시 {rows.length.toLocaleString()}</span>
      </div>
    </div>
  );
}

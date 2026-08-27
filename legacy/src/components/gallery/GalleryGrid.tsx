import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import type { ReactNode } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useAppStore } from "@/stores/appStore";
import { toast } from "@/stores/toastStore";
import { formatFileSize, formatDate } from "@/utils/format";
import { thumbSrc } from "@/utils/media";
import { ImageOff, Check, Play, FolderSearch, StopCircle, RefreshCw, ChevronDown, ChevronRight, UploadCloud, MessageSquareText } from "lucide-react";
import type { MediaFile, MediaListResponse } from "@/types";

export function GalleryGrid() {
  const [files, setFiles] = useState<MediaFile[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [previewFile, setPreviewFile] = useState<MediaFile | null>(null);
  const [sortKey, setSortKey] = useState<string>("modified_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [rowDensity, setRowDensity] = useState<"compact" | "normal" | "relaxed">("normal");
  const containerRef = useRef<HTMLDivElement>(null);

  const selectedFolder = useAppStore((s) => s.selectedFolder);
  const viewMode = useAppStore((s) => s.viewMode);
  const selectedMediaIds = useAppStore((s) => s.selectedMediaIds);
  const toggleMediaSelection = useAppStore((s) => s.toggleMediaSelection);
  const isScanning = useAppStore((s) => s.isScanning);
  const scanProgress = useAppStore((s) => s.scanProgress);
  const mediaFilter = useAppStore((s) => s.mediaFilter);
  const groupBy = useAppStore((s) => s.groupBy);
  const refreshCounter = useAppStore((s) => s.refreshCounter);
  const nasUploadedIds = useAppStore((s) => s.nasUploadedIds);
  const comments = useAppStore((s) => s.comments);
  const tagFilter = useAppStore((s) => s.tagFilter);
  const albumFilter = useAppStore((s) => s.albumFilter);
  const searchQueryRaw = useAppStore((s) => s.searchQuery);

  // Debounce the title-bar search so we don't hit SQLite on every keystroke
  const [searchQuery, setSearchQueryDebounced] = useState(searchQueryRaw);
  useEffect(() => {
    const timer = setTimeout(() => setSearchQueryDebounced(searchQueryRaw), 300);
    return () => clearTimeout(timer);
  }, [searchQueryRaw]);
  const scanStartTime = useRef<number | null>(null);

  // Track scan start time
  useEffect(() => {
    if (isScanning && !scanStartTime.current) {
      scanStartTime.current = Date.now();
    }
    if (!isScanning) {
      scanStartTime.current = null;
    }
  }, [isScanning]);

  const estimatedTimeLeft = useMemo(() => {
    if (!scanProgress || !scanStartTime.current || scanProgress.current === 0 || scanProgress.total === 0) {
      return null;
    }
    const elapsed = (Date.now() - scanStartTime.current) / 1000;
    const rate = scanProgress.current / elapsed;
    const remaining = (scanProgress.total - scanProgress.current) / rate;

    if (remaining < 1) return "곧 완료";
    if (remaining < 60) return `약 ${Math.ceil(remaining)}초 남음`;
    if (remaining < 3600) return `약 ${Math.ceil(remaining / 60)}분 남음`;
    return `약 ${Math.floor(remaining / 3600)}시간 ${Math.ceil((remaining % 3600) / 60)}분 남음`;
  }, [scanProgress]);

  const pageSize = groupBy !== "none" ? 2000 : 1000;

  const loadFiles = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<MediaListResponse>("get_media_list", {
        folderPath: selectedFolder,
        mediaType: mediaFilter === "all" ? null : mediaFilter,
        searchQuery: searchQuery.trim() || null,
        tagId: tagFilter?.id ?? null,
        albumId: albumFilter?.id ?? null,
        offset: 0,
        limit: pageSize,
      });
      setFiles(result.files);
      setTotal(result.total);
    } catch (e) {
      toast.error(`미디어 목록을 불러오지 못했습니다: ${e}`);
    }
    setLoading(false);
  }, [selectedFolder, mediaFilter, searchQuery, tagFilter, albumFilter, refreshCounter, pageSize]);

  const loadMore = useCallback(async () => {
    if (files.length >= total || loading) return;
    setLoading(true);
    try {
      const result = await invoke<MediaListResponse>("get_media_list", {
        folderPath: selectedFolder,
        mediaType: mediaFilter === "all" ? null : mediaFilter,
        searchQuery: searchQuery.trim() || null,
        tagId: tagFilter?.id ?? null,
        albumId: albumFilter?.id ?? null,
        offset: files.length,
        limit: pageSize,
      });
      setFiles((prev) => [...prev, ...result.files]);
      // Safety: an empty page means the end regardless of the reported total
      if (result.files.length === 0) {
        setTotal(files.length);
      }
    } catch (e) {
      toast.error(`미디어 목록을 불러오지 못했습니다: ${e}`);
    }
    setLoading(false);
  }, [files.length, total, loading, selectedFolder, mediaFilter, searchQuery, tagFilter, albumFilter, pageSize]);

  // refreshCounter is a loadFiles dependency, so Phase 1 batch refreshes
  // re-run this with current folder/filter/search values (no stale closure)
  useEffect(() => {
    loadFiles();
  }, [loadFiles, isScanning]);

  const setInspectorMedia = useAppStore((s) => s.setInspectorMedia);

  const handleOpenFile = useCallback(
    (file: MediaFile) => {
      setPreviewFile(file);
      setInspectorMedia(file);
    },
    [setInspectorMedia],
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.code === "Space") {
        e.preventDefault();
        if (previewFile) {
          setPreviewFile(null);
        } else {
          // Open first selected file with Quick Look
          const selectedId = Array.from(selectedMediaIds)[0];
          const file = files.find((f) => f.id === selectedId);
          if (file) {
            handleOpenFile(file);
          }
        }
      }
    },
    [previewFile, selectedMediaIds, files, handleOpenFile]
  );

  // Sorted files for list view
  const sortedFiles = useMemo(() => {
    const sorted = [...files];
    sorted.sort((a, b) => {
      let va: string | number = "";
      let vb: string | number = "";
      switch (sortKey) {
        case "file_name": va = a.file_name.toLowerCase(); vb = b.file_name.toLowerCase(); break;
        case "folder": va = a.file_path; vb = b.file_path; break;
        case "date_taken": va = a.date_taken || ""; vb = b.date_taken || ""; break;
        case "modified_at": va = a.modified_at; vb = b.modified_at; break;
        case "file_size": va = a.file_size; vb = b.file_size; break;
        case "resolution": va = (a.width || 0) * (a.height || 0); vb = (b.width || 0) * (b.height || 0); break;
        case "type": va = a.file_name.split(".").pop()?.toLowerCase() || ""; vb = b.file_name.split(".").pop()?.toLowerCase() || ""; break;
        default: va = a.modified_at; vb = b.modified_at;
      }
      if (va < vb) return sortAsc ? -1 : 1;
      if (va > vb) return sortAsc ? 1 : -1;
      return 0;
    });
    return sorted;
  }, [files, sortKey, sortAsc]);

  const handleSort = useCallback((key: string) => {
    if (sortKey === key) {
      setSortAsc(!sortAsc);
    } else {
      setSortKey(key);
      setSortAsc(key === "file_name");
    }
  }, [sortKey, sortAsc]);

  const defaultColWidths = useMemo(() => ({ icon: 24, file_name: 200, folder: 150, date_taken: 100, modified_at: 90, file_size: 70, resolution: 90, type: 60 }), []);
  const [colWidths, setColWidths] = useState({ icon: 24, file_name: 200, folder: 150, date_taken: 100, modified_at: 90, file_size: 70, resolution: 90, type: 60 });

  const handleColResize = useCallback((col: string, startX: number) => {
    const startW = colWidths[col as keyof typeof colWidths];
    const onMove = (e: MouseEvent) => {
      const diff = e.clientX - startX;
      setColWidths((prev) => ({ ...prev, [col]: Math.max(30, startW + diff) }));
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }, [colWidths]);

  // ALL hooks must be above any conditional returns
  const groupedFiles = useMemo(() => {
    if (groupBy === "none") return null;
    const groups: Record<string, typeof files> = {};

    for (const file of files) {
      let key: string;
      if (groupBy === "date") {
        const bestDate = file.date_taken || file.modified_at || "";
        key = bestDate.replace(/:/g, "-").slice(0, 10) || "날짜 없음";
      } else {
        // folder: extract parent directory
        const parts = file.file_path.split("/");
        parts.pop(); // remove filename
        key = parts.join("/") || "/";
      }
      if (!groups[key]) groups[key] = [];
      groups[key].push(file);
    }

    return Object.entries(groups).sort(([a], [b]) => {
      if (groupBy === "date") return b.localeCompare(a); // newest first
      return a.localeCompare(b); // alphabetical for folders
    });
  }, [files, groupBy]);

  const renderFileCard = useCallback((file: MediaFile) => (
    <div
      key={file.id}
      className={`group relative rounded-lg overflow-hidden border cursor-pointer transition-all hover:shadow-md ${
        selectedMediaIds.has(file.id)
          ? "border-accent ring-2 ring-accent/30"
          : "border-border hover:border-accent/50"
      }`}
      onClick={() => handleOpenFile(file)}
    >
      <div className="aspect-square bg-bg-secondary flex items-center justify-center relative">
        <Thumbnail file={file} size={200} />
        {nasUploadedIds.has(file.id) && (
          <div
            className="absolute bottom-1.5 right-1.5 bg-emerald-500/90 rounded-full p-1"
            title="NAS에 업로드됨"
          >
            <UploadCloud size={10} className="text-white" />
          </div>
        )}
        {comments.has(file.id) && (
          <div
            className="absolute bottom-1.5 left-1.5 bg-amber-500/90 rounded-full p-1"
            title={comments.get(file.id)}
          >
            <MessageSquareText size={10} className="text-white" />
          </div>
        )}
      </div>
      <div
        className={`absolute top-2 left-2 w-5 h-5 rounded-full flex items-center justify-center transition-opacity ${
          selectedMediaIds.has(file.id)
            ? "bg-accent opacity-100"
            : "bg-black/40 opacity-0 group-hover:opacity-100"
        }`}
      >
        <Check size={12} className="text-white" />
      </div>
      {file.media_type === "video" && (
        <div className="absolute top-2 right-2 bg-black/60 rounded-full p-1">
          <Play size={10} className="text-white" />
        </div>
      )}
      <div className="p-1.5 bg-bg-primary">
        <p className="text-xs text-text-primary truncate">{file.file_name}</p>
        <p className="text-[10px] text-text-secondary mt-0.5">{formatFileSize(file.file_size)}</p>
      </div>
    </div>
  ), [selectedMediaIds, toggleMediaSelection, handleOpenFile, nasUploadedIds, comments]);

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  const phase1Done = useAppStore((s) => s.phase1Done);
  const phase1Total = useAppStore((s) => s.phase1Total);
  const phase1Running = useAppStore((s) => s.phase1Running);

  // --- All hooks are above this line. Conditional returns below. ---

  if (isScanning && scanProgress) {
    return <ScanProgressView scanProgress={scanProgress} estimatedTimeLeft={estimatedTimeLeft} />;
  }

  const statusBar = (
    <div className="flex items-center gap-3 px-3 py-1.5 border-b border-border bg-bg-secondary/50 text-xs text-text-secondary shrink-0">
      <span className="font-medium text-text-primary">{total.toLocaleString()}개 파일</span>
      {phase1Running && phase1Total > 0 && (
        <>
          <div className="w-2.5 h-2.5 border-2 border-accent border-t-transparent rounded-full animate-spin" />
          <span className="text-accent">
            분석 중 {phase1Done.toLocaleString()} / {phase1Total.toLocaleString()}
            ({Math.round((phase1Done / phase1Total) * 100)}%)
          </span>
          <div className="w-32 h-1.5 bg-bg-primary rounded-full overflow-hidden">
            <div className="h-full bg-accent rounded-full transition-all" style={{ width: `${Math.round((phase1Done / phase1Total) * 100)}%` }} />
          </div>
        </>
      )}
      {!phase1Running && phase1Done > 0 && phase1Done >= phase1Total && phase1Total > 0 && (
        <span className="text-success">✓ 분석 완료</span>
      )}
      <div className="flex-1" />
      <button
        onClick={() => loadFiles()}
        className="flex items-center gap-1 px-2 py-0.5 rounded hover:bg-bg-secondary transition-colors text-text-secondary hover:text-text-primary"
      >
        <RefreshCw size={11} />
        새로고침
      </button>
    </div>
  );

  if (files.length === 0 && !loading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center text-text-secondary">
          <ImageOff size={48} className="mx-auto mb-3 opacity-30" />
          <p className="text-sm">미디어 파일이 없습니다</p>
          <p className="text-xs mt-1">폴더를 추가하여 스캔을 시작하세요</p>
        </div>
      </div>
    );
  }

  const densityPy = rowDensity === "compact" ? "py-0" : rowDensity === "relaxed" ? "py-1.5" : "py-0.5";

  if (viewMode === "list") {
    const ResizableHeader = ({ label, field, align }: { label: string; field: keyof typeof defaultColWidths; align?: string }) => (
      <th
        className="relative px-2 py-1.5 select-none"
        style={{ width: colWidths[field], minWidth: 30 }}
      >
        <span
          className={`flex items-center gap-0.5 cursor-pointer hover:text-text-primary ${align || ""}`}
          onClick={() => handleSort(field)}
        >
          {label}
          {sortKey === field && <span className="text-accent text-[9px]">{sortAsc ? "▲" : "▼"}</span>}
        </span>
        <div
          className="absolute top-0 right-0 w-1.5 h-full cursor-col-resize hover:bg-accent/30"
          onMouseDown={(e) => { e.stopPropagation(); handleColResize(field, e.clientX); }}
        />
      </th>
    );

    return (
      <div className="flex-1 flex flex-col overflow-hidden">
      {statusBar}
      <div className="flex items-center gap-2 px-3 py-1 border-b border-border bg-bg-secondary/30 text-[10px] text-text-secondary">
        <span>간격:</span>
        {(["compact", "normal", "relaxed"] as const).map((d) => (
          <button
            key={d}
            onClick={() => setRowDensity(d)}
            className={`px-1.5 py-0.5 rounded ${rowDensity === d ? "bg-accent text-white" : "hover:bg-bg-secondary"}`}
          >
            {d === "compact" ? "좁게" : d === "normal" ? "보통" : "넓게"}
          </button>
        ))}
        <button
          onClick={() => setColWidths(defaultColWidths)}
          className="px-1.5 py-0.5 rounded hover:bg-bg-secondary ml-auto"
        >
          컬럼 초기화
        </button>
      </div>
      <div ref={containerRef} className="flex-1 overflow-y-auto">
        <table className="text-[11px]" style={{ minWidth: "100%", tableLayout: "fixed" }}>
          <thead className="sticky top-0 bg-bg-secondary border-b border-border z-10">
            <tr className="text-left text-text-secondary">
              <th className="px-2 py-1.5" style={{ width: colWidths.icon }}></th>
              <ResizableHeader label="파일명" field="file_name" />
              <ResizableHeader label="폴더" field="folder" />
              <ResizableHeader label="촬영일" field="date_taken" />
              <ResizableHeader label="수정일" field="modified_at" />
              <ResizableHeader label="크기" field="file_size" align="text-right" />
              <ResizableHeader label="해상도" field="resolution" />
              <ResizableHeader label="유형" field="type" />
            </tr>
          </thead>
          <tbody>
            {sortedFiles.map((file) => {
              const ext = file.file_name.split(".").pop()?.toUpperCase() || "";
              const folder = file.file_path.substring(0, file.file_path.lastIndexOf("/")).split("/").slice(-2).join("/");
              return (
                <tr
                  key={file.id}
                  className={`border-b border-border/30 hover:bg-bg-secondary/50 cursor-pointer transition-colors ${
                    selectedMediaIds.has(file.id) ? "bg-accent/5" : ""
                  }`}
                  onClick={() => handleOpenFile(file)}
                >
                  <td className={`px-2 ${densityPy}`}>
                    {file.media_type === "video" ? (
                      <Play size={12} className="text-purple-400" />
                    ) : ext === "PNG" ? (
                      <ImageOff size={12} className="text-green-400" />
                    ) : ext === "HEIC" ? (
                      <ImageOff size={12} className="text-orange-400" />
                    ) : (
                      <ImageOff size={12} className="text-blue-400" />
                    )}
                  </td>
                  <td className={`px-2 ${densityPy} text-text-primary truncate max-w-[200px]`} title={file.file_name}>
                    <span className="font-medium">{file.file_name.substring(0, file.file_name.lastIndexOf("."))}</span>
                    <span className="text-text-secondary">.{file.file_name.split(".").pop()}</span>
                  </td>
                  <td className={`px-2 ${densityPy} text-text-secondary truncate max-w-[130px]`} title={file.file_path}>
                    {folder}
                  </td>
                  <td className={`px-2 ${densityPy} text-text-secondary`}>
                    {file.date_taken ? formatDate(file.date_taken) : "-"}
                  </td>
                  <td className={`px-2 ${densityPy} text-text-secondary`}>
                    {formatDate(file.modified_at)}
                  </td>
                  <td className={`px-2 ${densityPy} text-text-secondary text-right`}>
                    {formatFileSize(file.file_size)}
                  </td>
                  <td className={`px-2 ${densityPy} text-text-secondary`}>
                    {file.width && file.height ? `${file.width}×${file.height}` : "-"}
                  </td>
                  <td className={`px-2 ${densityPy}`}>
                    <span className={`text-[9px] px-1.5 py-0.5 rounded-full font-medium ${
                      file.media_type === "video"
                        ? "bg-purple-500/10 text-purple-400"
                        : "bg-blue-500/10 text-blue-400"
                    }`}>
                      {ext}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        {files.length < total && (
          <div className="flex justify-center py-4">
            <button onClick={loadMore} disabled={loading}
              className="px-6 py-2 rounded-md text-xs font-medium bg-accent/10 text-accent hover:bg-accent/20 transition-colors disabled:opacity-50">
              {loading ? "로딩 중..." : `더 보기 (${files.length.toLocaleString()} / ${total.toLocaleString()})`}
            </button>
          </div>
        )}

        {previewFile && (
          <PreviewModal file={previewFile} onClose={() => setPreviewFile(null)} />
        )}
      </div>
      </div>
    );
  }

  if (groupBy !== "none" && groupedFiles) {
    return (
      <div className="flex-1 flex flex-col overflow-hidden">
      {statusBar}
      <div ref={containerRef} className="flex-1 overflow-y-auto p-3">
        {groupedFiles.map(([key, groupFiles]) => (
          <DateAccordion key={key} label={groupBy === "folder" ? key.split("/").slice(-2).join("/") : key} fullLabel={key} files={groupFiles} renderFileCard={renderFileCard} />
        ))}
        {files.length < total && (
          <div className="flex justify-center py-4">
            <button
              onClick={loadMore}
              disabled={loading}
              className="px-6 py-2 rounded-md text-xs font-medium bg-accent/10 text-accent hover:bg-accent/20 transition-colors disabled:opacity-50"
            >
              {loading ? "로딩 중..." : `더 보기 (${files.length.toLocaleString()} / ${total.toLocaleString()})`}
            </button>
          </div>
        )}
        {previewFile && <PreviewModal file={previewFile} onClose={() => setPreviewFile(null)} />}
      </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {statusBar}
      <VirtualCardGrid
        files={files}
        total={total}
        loading={loading}
        loadMore={loadMore}
        renderFileCard={renderFileCard}
      />
      {previewFile && (
        <PreviewModal
          file={previewFile}
          onClose={() => setPreviewFile(null)}
        />
      )}
    </div>
  );
}

const GRID_GAP = 8;
const MIN_CARD_WIDTH = 160;
const GRID_PADDING = 12;
const CARD_INFO_HEIGHT = 44;

// Row-based virtualization: only the visible rows exist in the DOM, so the
// gallery stays smooth with tens of thousands of loaded items
function VirtualCardGrid({
  files,
  total,
  loading,
  loadMore,
  renderFileCard,
}: {
  files: MediaFile[];
  total: number;
  loading: boolean;
  loadMore: () => void;
  renderFileCard: (file: MediaFile) => ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const thumbSize = useAppStore((s) => s.thumbSize);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);

  const innerWidth = Math.max(0, width - GRID_PADDING * 2);
  const cols = Math.max(
    1,
    Math.floor((innerWidth + GRID_GAP) / (thumbSize + GRID_GAP))
  );
  const cardWidth =
    innerWidth > 0 ? (innerWidth - (cols - 1) * GRID_GAP) / cols : MIN_CARD_WIDTH;
  const rowHeight = cardWidth + CARD_INFO_HEIGHT + GRID_GAP;
  const rowCount = Math.ceil(files.length / cols);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 4,
  });

  useEffect(() => {
    virtualizer.measure();
  }, [rowHeight, virtualizer]);

  const virtualRows = virtualizer.getVirtualItems();
  const lastVisibleRow = virtualRows.length
    ? virtualRows[virtualRows.length - 1].index
    : -1;

  // Auto-fetch the next page as the user approaches the end
  useEffect(() => {
    if (lastVisibleRow >= rowCount - 3 && files.length < total && !loading) {
      loadMore();
    }
  }, [lastVisibleRow, rowCount, files.length, total, loading, loadMore]);

  return (
    <div ref={scrollRef} className="flex-1 overflow-y-auto p-3">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualRows.map((row) => (
          <div
            key={row.key}
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              right: 0,
              transform: `translateY(${row.start}px)`,
              display: "flex",
              gap: GRID_GAP,
            }}
          >
            {files
              .slice(row.index * cols, row.index * cols + cols)
              .map((file) => (
                <div key={file.id} style={{ width: cardWidth }}>
                  {renderFileCard(file)}
                </div>
              ))}
          </div>
        ))}
      </div>
      {loading && (
        <div className="flex justify-center py-4">
          <div className="w-5 h-5 border-2 border-accent border-t-transparent rounded-full animate-spin" />
        </div>
      )}
      {!loading && files.length >= total && total > 0 && (
        <p className="text-center text-[10px] text-text-secondary/60 py-3">
          {total.toLocaleString()}개 모두 표시됨
        </p>
      )}
    </div>
  );
}

export function Thumbnail({ file }: { file: MediaFile; size?: number }) {
  const [thumbError, setThumbError] = useState(false);
  const [error, setError] = useState(false);

  // Priority: 1) cached thumbnail file, 2) original file via asset protocol, 3) placeholder
  const thumb = thumbSrc(file.thumbnail);
  if (thumb && !thumbError) {
    return (
      <img
        src={thumb}
        alt={file.file_name}
        className="w-full h-full object-cover"
        loading="lazy"
        onError={() => setThumbError(true)}
      />
    );
  }

  if (!error && file.media_type === "image") {
    return (
      <img
        src={convertFileSrc(file.file_path)}
        alt={file.file_name}
        className="w-full h-full object-cover"
        loading="lazy"
        decoding="async"
        onError={() => setError(true)}
      />
    );
  }

  return (
    <div className="w-full h-full flex flex-col items-center justify-center bg-bg-secondary">
      {file.media_type === "video" ? (
        <Play size={20} className="text-text-secondary/30" />
      ) : (
        <ImageOff size={20} className="text-text-secondary/20" />
      )}
    </div>
  );
}

function PreviewModal({
  file,
  onClose,
}: {
  file: MediaFile;
  onClose: () => void;
}) {
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [loadingPreview, setLoadingPreview] = useState(true);
  const folderPath = file.file_path.substring(0, file.file_path.lastIndexOf("/"));
  const bestDate = file.date_taken || file.modified_at || "";

  // Load preview image/video frame via Tauri command
  useEffect(() => {
    setLoadingPreview(true);
    setPreviewSrc(null);
    const cmd = file.media_type === "video" ? "get_preview_video_frame" : "get_preview_image";
    invoke<string>(cmd, { filePath: file.file_path })
      .then((b64) => setPreviewSrc(`data:image/jpeg;base64,${b64}`))
      .catch(() => setPreviewSrc(null))
      .finally(() => setLoadingPreview(false));
  }, [file]);

  return (
    <div className="fixed inset-0 z-50 bg-black/80 flex" onClick={onClose}>
      {/* Preview area */}
      <div className="flex-1 flex items-center justify-center p-6">
        {loadingPreview ? (
          <div className="w-10 h-10 border-2 border-white/30 border-t-white rounded-full animate-spin" />
        ) : previewSrc ? (
          <img
            src={previewSrc}
            alt={file.file_name}
            className="max-w-full max-h-full object-contain rounded-lg shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <div className="text-white/50 text-sm">미리보기를 불러올 수 없습니다</div>
        )}
      </div>

      {/* Info panel */}
      <div
        className="w-72 bg-bg-elevated border-l border-border flex flex-col shrink-0"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-3 border-b border-border">
          <h3 className="text-xs font-semibold text-text-primary truncate pr-2">{file.file_name}</h3>
          <button onClick={onClose} className="text-text-secondary hover:text-text-primary text-xs shrink-0">✕</button>
        </div>

        {/* Actions */}
        <div className="flex gap-2 p-3 border-b border-border">
          <button
            onClick={() => invoke("preview_file", { path: file.file_path })}
            className="flex-1 py-1.5 rounded text-[10px] font-medium bg-accent text-white hover:bg-accent-hover transition-colors"
          >
            Quick Look
          </button>
          <button
            onClick={() => invoke("open_file", { path: file.file_path })}
            className="flex-1 py-1.5 rounded text-[10px] font-medium bg-bg-secondary text-text-primary hover:bg-bg-primary transition-colors border border-border"
          >
            Finder 열기
          </button>
        </div>

        {/* File info */}
        <div className="flex-1 overflow-y-auto p-3 space-y-2.5 text-xs">
          <InfoRow label="유형" value={`${file.media_type === "image" ? "이미지" : "영상"} · ${file.mime_type || ""}`} />
          <InfoRow label="크기" value={formatFileSize(file.file_size)} />
          {file.width && file.height && (
            <InfoRow label="해상도" value={`${file.width} × ${file.height}`} />
          )}
          <InfoRow label="촬영일" value={bestDate ? formatDate(bestDate) : "-"} />
          <InfoRow label="수정일" value={formatDate(file.modified_at)} />
          <InfoRow label="생성일" value={formatDate(file.created_at)} />
          <div>
            <p className="text-text-secondary text-[10px] mb-1">경로</p>
            <p className="text-text-primary text-[10px] break-all leading-relaxed bg-bg-secondary rounded px-2 py-1.5">{file.file_path}</p>
          </div>
          <div>
            <p className="text-text-secondary text-[10px] mb-1">폴더</p>
            <p className="text-text-primary text-[10px] break-all leading-relaxed bg-bg-secondary rounded px-2 py-1.5">{folderPath}</p>
          </div>
        </div>
      </div>
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-text-secondary">{label}</span>
      <span className="text-text-primary font-medium text-right">{value}</span>
    </div>
  );
}

function ScanProgressView({
  scanProgress,
  estimatedTimeLeft,
}: {
  scanProgress: import("@/types").ScanProgress;
  estimatedTimeLeft: string | null;
}) {
  const [displayCurrent, setDisplayCurrent] = useState(0);
  const [dots, setDots] = useState("");
  const targetCurrent = scanProgress.current;
  const total = scanProgress.total;

  // Smoothly interpolate displayed count toward actual count
  useEffect(() => {
    if (targetCurrent <= displayCurrent) return;
    const step = Math.max(1, Math.ceil((targetCurrent - displayCurrent) / 20));
    const timer = setInterval(() => {
      setDisplayCurrent((prev) => {
        const next = prev + step;
        if (next >= targetCurrent) {
          clearInterval(timer);
          return targetCurrent;
        }
        return next;
      });
    }, 50);
    return () => clearInterval(timer);
  }, [targetCurrent]);

  // Animated dots to show activity
  useEffect(() => {
    const timer = setInterval(() => {
      setDots((prev) => (prev.length >= 3 ? "" : prev + "."));
    }, 400);
    return () => clearInterval(timer);
  }, []);

  const percent = total > 0 ? Math.round((displayCurrent / total) * 100) : 0;

  const phaseLabel = scanProgress.phase === "scanning"
    ? "파일 탐색 중"
    : scanProgress.phase === "processing"
      ? "파일 분석 중"
      : "완료!";

  return (
    <div className="flex-1 flex items-center justify-center">
      <div className="text-center w-80">
        <FolderSearch size={48} className="mx-auto mb-4 text-accent animate-pulse" />
        <p className="text-sm font-semibold text-text-primary mb-1">
          {phaseLabel}{scanProgress.phase !== "complete" ? dots : ""}
        </p>
        <p className="text-xs text-text-secondary mb-4 h-4 truncate">
          {scanProgress.current_file || "준비 중..."}
        </p>

        {/* Progress bar */}
        <div className="h-2.5 bg-bg-secondary rounded-full overflow-hidden mb-2 relative">
          <div
            className="h-full bg-accent rounded-full transition-all duration-500 ease-out"
            style={{ width: `${percent}%` }}
          />
          {/* Shimmer effect to show activity */}
          {percent < 100 && (
            <div
              className="absolute inset-0 rounded-full overflow-hidden"
              style={{ width: `${percent}%` }}
            >
              <div className="w-full h-full bg-gradient-to-r from-transparent via-white/20 to-transparent animate-[shimmer_1.5s_infinite]" />
            </div>
          )}
        </div>

        <div className="flex justify-between text-xs text-text-secondary">
          <span>{displayCurrent.toLocaleString()} / {total.toLocaleString()} 파일</span>
          <span>{percent}%</span>
        </div>

        {estimatedTimeLeft && (
          <p className="text-xs text-accent mt-2">{estimatedTimeLeft}</p>
        )}

        <button
          onClick={() => invoke("cancel_scan")}
          className="mt-4 flex items-center justify-center gap-2 mx-auto px-4 py-2 rounded-md text-xs font-medium bg-danger/10 text-danger hover:bg-danger/20 transition-colors"
        >
          <StopCircle size={14} />
          스캔 중단
        </button>
      </div>
    </div>
  );
}

function DateAccordion({
  label,
  fullLabel,
  files,
  renderFileCard,
}: {
  label: string;
  fullLabel?: string;
  files: MediaFile[];
  renderFileCard: (file: MediaFile) => React.ReactNode;
}) {
  const [isOpen, setIsOpen] = useState(true);
  const thumbSize = useAppStore((s) => s.thumbSize);

  return (
    <div className="mb-3">
      <button
        onClick={() => setIsOpen(!isOpen)}
        title={fullLabel || label}
        className="flex items-center gap-2 w-full py-1.5 px-1 sticky top-0 bg-bg-primary/95 backdrop-blur z-10 hover:bg-bg-secondary/50 rounded transition-colors"
      >
        {isOpen ? (
          <ChevronDown size={14} className="text-text-secondary shrink-0" />
        ) : (
          <ChevronRight size={14} className="text-text-secondary shrink-0" />
        )}
        <span className="text-sm font-semibold text-text-primary truncate">{label}</span>
        <span className="text-xs text-text-secondary shrink-0">{files.length}개</span>
        {!isOpen && (
          <span className="text-[10px] text-text-secondary ml-auto">
            {formatFileSize(files.reduce((s, f) => s + f.file_size, 0))}
          </span>
        )}
      </button>
      {isOpen && (
        <div
          className="grid gap-2 mt-1"
          style={{
            gridTemplateColumns: `repeat(auto-fill, minmax(${thumbSize}px, 1fr))`,
          }}
        >
          {files.map(renderFileCard)}
        </div>
      )}
    </div>
  );
}

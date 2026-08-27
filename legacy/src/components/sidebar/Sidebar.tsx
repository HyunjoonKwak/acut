import { useState, useEffect, useRef, useCallback } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useAppStore } from "@/stores/appStore";
import {
  FolderPlus,
  FolderOpen,
  Image,
  Video,
  Calendar,
  Tag,
  ChevronRight,
  ChevronDown,
  RefreshCw,
  Trash2,
  RotateCcw,
  FolderHeart,
  MapPin,
} from "lucide-react";
import { formatFileSize } from "@/utils/format";
import { runPhase1Loop } from "@/utils/phase1";
import { toast } from "@/stores/toastStore";

interface SourceFolder {
  id: string;
  path: string;
  name: string;
  added_at: string;
  last_scanned_at: string | null;
}

export function Sidebar() {
  const { t } = useTranslation();
  const [folders, setFolders] = useState<SourceFolder[]>([]);
  const selectedFolder = useAppStore((s) => s.selectedFolder);
  const setSelectedFolder = useAppStore((s) => s.setSelectedFolder);
  const setIsScanning = useAppStore((s) => s.setIsScanning);
  const setStats = useAppStore((s) => s.setStats);
  const stats = useAppStore((s) => s.stats);
  const isScanning = useAppStore((s) => s.isScanning);
  const currentView = useAppStore((s) => s.currentView);
  const mediaFilter = useAppStore((s) => s.mediaFilter);
  const groupBy = useAppStore((s) => s.groupBy);
  const refreshCounter = useAppStore((s) => s.refreshCounter);

  // Load saved folders on mount and whenever something (e.g. the device
  // panel) registers a new source
  useEffect(() => {
    const loadFolders = async () => {
      try {
        const saved = await invoke<SourceFolder[]>("get_source_folders");
        setFolders(saved);
      } catch {
        // DB not ready yet
      }
    };
    loadFolders();
  }, [refreshCounter]);

  const handleAddFolder = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected) {
        const path = selected as string;
        const name = path.split("/").pop() || path;

        // Save to DB
        const folder = await invoke<SourceFolder>("add_source_folder", {
          path,
          name,
        });

        setFolders((prev) => {
          const exists = prev.find((f) => f.path === path);
          if (exists) return prev;
          return [...prev, folder];
        });

        setSelectedFolder(path);
        setIsScanning(true);

        // Phase 0: fast file list
        await invoke("scan_directory", { path });
        await invoke("update_folder_scan_time", { path });
        const newStats = await invoke("get_media_stats");
        setStats(newStats as any);
        setIsScanning(false);

        // Phase 1: process batches in a loop from frontend
        runPhase1Loop();
      }
    } catch (e) {
      setIsScanning(false);
      toast.error(`폴더 스캔에 실패했습니다: ${e}`);
    }
  };

  const handleRescan = async (path: string) => {
    setIsScanning(true);
    try {
      await invoke("scan_directory", { path });
      await invoke("update_folder_scan_time", { path });
      const newStats = await invoke("get_media_stats");
      setStats(newStats as any);
      runPhase1Loop();
    } catch (e) {
      toast.error(`다시 스캔에 실패했습니다: ${e}`);
    }
    setIsScanning(false);
  };

  const handleResetLibrary = async () => {
    try {
      await invoke("reset_library");
      setFolders([]);
      setSelectedFolder(null);
      setStats(null);
      useAppStore.getState().triggerRefresh();
    } catch (e) {
      toast.error(`라이브러리 초기화에 실패했습니다: ${e}`);
    }
  };

  const handleRemoveFolder = async (path: string) => {
    try {
      await invoke("remove_source_folder", { path });
      setFolders((prev) => prev.filter((f) => f.path !== path));
      if (selectedFolder === path) {
        setSelectedFolder(null);
      }
      const newStats = await invoke("get_media_stats");
      setStats(newStats as any);
      // Force gallery to reload
      useAppStore.getState().triggerRefresh();
    } catch {
      // Handle error
    }
  };

  const [width, setWidth] = useState(224);
  const isResizing = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isResizing.current = true;
    const startX = e.clientX;
    const startWidth = width;

    const handleMouseMove = (e: MouseEvent) => {
      if (!isResizing.current) return;
      const newWidth = Math.min(500, Math.max(160, startWidth + (e.clientX - startX)));
      setWidth(newWidth);
    };

    const handleMouseUp = () => {
      isResizing.current = false;
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };

    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
  }, [width]);

  return (
    <aside className="bg-bg-secondary border-r border-border flex flex-col shrink-0 select-none relative" style={{ width }}>
      {/* Resize handle */}
      <div
        onMouseDown={handleMouseDown}
        className="absolute top-0 right-0 w-1 h-full cursor-col-resize hover:bg-accent/30 active:bg-accent/50 z-20"
      />
      <div className="p-3 border-b border-border">
        <button
          onClick={handleAddFolder}
          disabled={isScanning}
          className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors disabled:opacity-50"
        >
          {isScanning ? (
            <>
              <div className="w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full animate-spin" />
              {t("sidebar.scanning")}
            </>
          ) : (
            <>
              <FolderPlus size={14} />
              {t("sidebar.addFolder")}
            </>
          )}
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2">
        <SidebarSection title={t("sidebar.source")}>
          {folders.map((folder) => (
            <SidebarItem
              key={folder.path}
              icon={FolderOpen}
              label={folder.name}
              isActive={selectedFolder === folder.path}
              onClick={() => setSelectedFolder(folder.path)}
              onContextAction={() => handleRescan(folder.path)}
              contextIcon={RefreshCw}
              onRemove={() => handleRemoveFolder(folder.path)}
            />
          ))}
          {folders.length === 0 && (
            <p className="text-xs text-text-secondary px-2 py-4 text-center">
              {t("sidebar.emptyHint")}
            </p>
          )}
        </SidebarSection>

        <SidebarSection title={t("sidebar.library")}>
          <SidebarItem
            icon={Image}
            label={t("sidebar.allFiles")}
            tooltip="전체 미디어 파일 보기"
            count={stats?.total_count}
            onClick={() => {
              setSelectedFolder(null);
              useAppStore.getState().setMediaFilter("all");
              useAppStore.getState().setGroupBy("none");
              useAppStore.getState().setTagFilter(null);
              useAppStore.getState().setAlbumFilter(null);
              useAppStore.getState().setCurrentView("gallery");
            }}
            isActive={currentView === "gallery" && selectedFolder === null && mediaFilter === "all" && groupBy === "none"}
          />
          <SidebarItem
            icon={Image}
            label={t("sidebar.imagesOnly")}
            tooltip="사진 파일만 필터링"
            count={stats?.image_count}
            onClick={() => {
              setSelectedFolder(null);
              useAppStore.getState().setMediaFilter("image");
              useAppStore.getState().setGroupBy("none");
              useAppStore.getState().setTagFilter(null);
              useAppStore.getState().setAlbumFilter(null);
              useAppStore.getState().setCurrentView("gallery");
            }}
            isActive={currentView === "gallery" && mediaFilter === "image"}
          />
          <SidebarItem
            icon={Video}
            label={t("sidebar.videosOnly")}
            tooltip="영상 파일만 필터링"
            count={stats?.video_count}
            onClick={() => {
              setSelectedFolder(null);
              useAppStore.getState().setMediaFilter("video");
              useAppStore.getState().setGroupBy("none");
              useAppStore.getState().setTagFilter(null);
              useAppStore.getState().setAlbumFilter(null);
              useAppStore.getState().setCurrentView("gallery");
            }}
            isActive={currentView === "gallery" && mediaFilter === "video"}
          />
          <DateGroupList />
          <FolderGroupList />
          <TagFilterList />
          <AlbumFilterList />
          <SidebarItem
            icon={MapPin}
            label={t("sidebar.map")}
            tooltip={t("sidebar.mapTip")}
            onClick={() => useAppStore.getState().setCurrentView("map")}
            isActive={currentView === "map"}
          />
        </SidebarSection>

      </div>

      <div className="p-3 border-t border-border text-xs text-text-secondary">
        {stats && stats.total_count > 0 ? (
          <>
            <div className="flex justify-between">
              <span>{t("sidebar.total")}</span>
              <span>{stats.total_count} {t("sidebar.files")}</span>
            </div>
            <div className="flex justify-between mt-1">
              <span>{t("sidebar.capacity")}</span>
              <span>{formatFileSize(stats.total_size)}</span>
            </div>
            <button
              onClick={handleResetLibrary}
              className="w-full flex items-center justify-center gap-1.5 mt-2 px-2 py-1.5 rounded text-[10px] text-danger hover:bg-danger/10 transition-colors"
            >
              <RotateCcw size={10} />
              {t("sidebar.resetLibrary")}
            </button>
          </>
        ) : (
          <p className="text-center text-[10px] py-1">{t("sidebar.libraryEmpty")}</p>
        )}
        <p className="text-center text-[9px] text-text-secondary/50 mt-2">
          v{__APP_VERSION__}
        </p>
      </div>
    </aside>
  );
}

function SidebarSection({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  const [isOpen, setIsOpen] = useState(true);

  return (
    <div className="mb-3">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-1 px-2 py-1 text-xs font-semibold text-text-secondary uppercase tracking-wider w-full hover:text-text-primary"
      >
        {isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {title}
      </button>
      {isOpen && <div className="mt-1 space-y-0.5">{children}</div>}
    </div>
  );
}

function SidebarItem({
  icon: Icon,
  label,
  tooltip,
  count,
  isActive,
  onClick,
  onContextAction,
  contextIcon: ContextIcon,
  onRemove,
}: {
  icon: React.ComponentType<{ size?: number }>;
  label: string;
  tooltip?: string;
  count?: number;
  isActive?: boolean;
  onClick?: () => void;
  onContextAction?: () => void;
  contextIcon?: React.ComponentType<{ size?: number }>;
  onRemove?: () => void;
}) {
  const [showTip, setShowTip] = useState(false);
  const [tipPos, setTipPos] = useState({ x: 0, y: 0 });
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const btnRef = useRef<HTMLButtonElement>(null);

  const handleEnter = () => {
    if (!tooltip) return;
    timerRef.current = setTimeout(() => {
      if (btnRef.current) {
        const rect = btnRef.current.getBoundingClientRect();
        setTipPos({ x: rect.right + 8, y: rect.top + rect.height / 2 });
        setShowTip(true);
      }
    }, 400);
  };

  const handleLeave = () => {
    if (timerRef.current) clearTimeout(timerRef.current);
    setShowTip(false);
  };

  return (
    <>
      <button
        ref={btnRef}
        onClick={onClick}
        onMouseEnter={handleEnter}
        onMouseLeave={handleLeave}
        className={`w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-xs group transition-all duration-150 ${
          isActive
            ? "bg-accent/15 text-accent font-semibold shadow-sm shadow-accent/10"
            : "text-text-secondary hover:bg-bg-primary hover:text-text-primary active:scale-[0.97] active:bg-accent/10"
        }`}
      >
        <Icon size={14} />
        <span className="flex-1 text-left truncate">{label}</span>
        {count !== undefined && (
          <span className="text-text-secondary">{count}</span>
        )}
        {ContextIcon && onContextAction && (
          <span
            onClick={(e) => {
              e.stopPropagation();
              onContextAction();
            }}
            className="opacity-0 group-hover:opacity-100 hover:text-accent"
          >
            <ContextIcon size={12} />
          </span>
        )}
        {onRemove && (
          <span
            onClick={(e) => {
              e.stopPropagation();
              onRemove();
            }}
            className="opacity-0 group-hover:opacity-100 hover:text-danger"
          >
            <Trash2 size={12} />
          </span>
        )}
      </button>
      {showTip && tooltip && createPortal(
        <div
          className="fixed px-2.5 py-1.5 rounded-md bg-[#1a1a2e] text-white text-[10px] whitespace-nowrap shadow-lg shadow-black/20 pointer-events-none z-[9999] animate-fade-in"
          style={{ left: tipPos.x, top: tipPos.y, transform: "translateY(-50%)" }}
        >
          {tooltip}
          <div className="absolute right-full top-1/2 -translate-y-1/2 border-4 border-transparent border-r-[#1a1a2e]" />
        </div>,
        document.body
      )}
    </>
  );
}

interface GroupInfo {
  label: string;
  count: number;
}

function DateGroupList() {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [dates, setDates] = useState<GroupInfo[]>([]);
  const dateFilter = useAppStore((s) => s.dateFilter);

  const handleToggle = async () => {
    if (!isOpen && dates.length === 0) {
      try {
        const result = await invoke<GroupInfo[]>("get_date_groups");
        setDates(result);
      } catch {}
    }
    setIsOpen(!isOpen);
  };

  const handleSelect = (date: string) => {
    useAppStore.getState().setSelectedFolder(null);
    useAppStore.getState().setMediaFilter("all");
    useAppStore.getState().setGroupBy("none");
    useAppStore.getState().setDateFilter(date);
    useAppStore.getState().setFolderFilter(null);
    useAppStore.getState().setCurrentView("gallery");
    useAppStore.getState().triggerRefresh();
  };

  const handleShowAll = () => {
    useAppStore.getState().setSelectedFolder(null);
    useAppStore.getState().setMediaFilter("all");
    useAppStore.getState().setGroupBy("date");
    useAppStore.getState().setDateFilter(null);
    useAppStore.getState().setFolderFilter(null);
    useAppStore.getState().setCurrentView("gallery");
  };

  return (
    <div>
      <button
        onClick={handleToggle}
        className={`w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-xs transition-colors ${
          isOpen ? "text-accent" : "text-text-secondary hover:bg-bg-primary hover:text-text-primary"
        }`}
      >
        {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <Calendar size={14} />
        <span className="flex-1 text-left">{t("sidebar.byDate")}</span>
        {dates.length > 0 && <span className="text-[10px] text-text-secondary">{dates.length}</span>}
      </button>
      {isOpen && (
        <div className="ml-5 mt-0.5 max-h-48 overflow-y-auto space-y-0.5">
          <button
            onClick={handleShowAll}
            className="w-full text-left px-2 py-1 rounded text-[10px] text-accent hover:bg-accent/10"
          >
            {t("sidebar.showAllDates")}
          </button>
          {dates.map((d) => (
            <button
              key={d.label}
              onClick={() => handleSelect(d.label)}
              className={`w-full flex items-center justify-between px-2 py-1 rounded text-[10px] transition-colors ${
                dateFilter === d.label ? "bg-accent/10 text-accent" : "text-text-secondary hover:bg-bg-primary"
              }`}
            >
              <span className="truncate">{d.label}</span>
              <span>{d.count}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

interface FolderNode {
  name: string;
  fullPath: string;
  count: number;
  children: FolderNode[];
}

function buildFolderTree(folders: GroupInfo[]): FolderNode[] {
  const root: FolderNode = { name: "", fullPath: "", count: 0, children: [] };

  for (const f of folders) {
    const parts = f.label.split("/").filter(Boolean);
    let current = root;

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      let child = current.children.find((c) => c.name === part);
      if (!child) {
        child = {
          name: part,
          fullPath: "/" + parts.slice(0, i + 1).join("/"),
          count: 0,
          children: [],
        };
        current.children.push(child);
      }
      if (i === parts.length - 1) {
        child.count += f.count;
      }
      current = child;
    }
  }

  // Propagate counts upward
  function sumCounts(node: FolderNode): number {
    if (node.children.length === 0) return node.count;
    const childSum = node.children.reduce((s, c) => s + sumCounts(c), 0);
    node.count = node.count + childSum;
    return node.count;
  }
  for (const c of root.children) sumCounts(c);

  // Collapse single-child chains: A -> B -> C => A/B/C
  function collapse(node: FolderNode): FolderNode {
    while (node.children.length === 1 && node.children[0].children.length > 0) {
      const child = node.children[0];
      node.name = node.name ? node.name + "/" + child.name : child.name;
      node.fullPath = child.fullPath;
      node.children = child.children;
    }
    node.children = node.children.map(collapse);
    return node;
  }
  root.children = root.children.map(collapse);

  return root.children;
}

function FolderGroupList() {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [tree, setTree] = useState<FolderNode[]>([]);
  const [rawCount, setRawCount] = useState(0);

  const handleToggle = async () => {
    if (!isOpen && tree.length === 0) {
      try {
        const result = await invoke<GroupInfo[]>("get_folder_groups");
        setRawCount(result.length);
        setTree(buildFolderTree(result));
      } catch {}
    }
    setIsOpen(!isOpen);
  };

  return (
    <div>
      <button
        onClick={handleToggle}
        className={`w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-xs transition-colors ${
          isOpen ? "text-accent" : "text-text-secondary hover:bg-bg-primary hover:text-text-primary"
        }`}
      >
        {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <FolderOpen size={14} />
        <span className="flex-1 text-left">{t("sidebar.byFolder")}</span>
        {rawCount > 0 && <span className="text-[10px] text-text-secondary">{rawCount}</span>}
      </button>
      {isOpen && (
        <div className="ml-3 mt-0.5 max-h-64 overflow-y-auto">
          {tree.map((node) => (
            <FolderTreeNode key={node.fullPath} node={node} depth={0} />
          ))}
        </div>
      )}
    </div>
  );
}

function FolderTreeNode({ node, depth }: { node: FolderNode; depth: number }) {
  const [isOpen, setIsOpen] = useState(depth < 2);
  const folderFilter = useAppStore((s) => s.folderFilter);
  const hasChildren = node.children.length > 0;

  const handleSelect = () => {
    if (hasChildren) {
      setIsOpen(!isOpen);
    }
    useAppStore.getState().setSelectedFolder(node.fullPath);
    useAppStore.getState().setMediaFilter("all");
    useAppStore.getState().setGroupBy("none");
    useAppStore.getState().setDateFilter(null);
    useAppStore.getState().setFolderFilter(node.fullPath);
    useAppStore.getState().setCurrentView("gallery");
    useAppStore.getState().triggerRefresh();
  };

  return (
    <div>
      <button
        onClick={handleSelect}
        title={node.fullPath}
        className={`w-full flex items-center gap-1 py-0.5 rounded text-[10px] transition-colors ${
          folderFilter === node.fullPath ? "bg-accent/10 text-accent" : "text-text-secondary hover:bg-bg-primary hover:text-text-primary"
        }`}
        style={{ paddingLeft: `${depth * 10 + 4}px` }}
      >
        {hasChildren ? (
          isOpen ? <ChevronDown size={10} className="shrink-0" /> : <ChevronRight size={10} className="shrink-0" />
        ) : (
          <span className="w-[10px] shrink-0" />
        )}
        <span className="truncate">{node.name}</span>
        <span className="shrink-0 ml-auto text-text-secondary/60">{node.count}</span>
      </button>
      {hasChildren && isOpen && (
        <div>
          {node.children.map((child) => (
            <FolderTreeNode key={child.fullPath} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Tag / Album filter lists (library sidebar) ──

interface TagInfo {
  id: string;
  name: string;
}

interface AlbumInfo {
  id: string;
  name: string;
  media_count: number;
}

/** 태그 목록 — 클릭하면 갤러리를 해당 태그로 필터링한다. */
function TagFilterList() {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [tags, setTags] = useState<TagInfo[]>([]);
  const tagFilter = useAppStore((s) => s.tagFilter);
  const refreshCounter = useAppStore((s) => s.refreshCounter);

  useEffect(() => {
    if (!isOpen) return;
    invoke<TagInfo[]>("get_tags")
      .then(setTags)
      .catch(() => {});
  }, [isOpen, refreshCounter]);

  const handleSelect = (tag: TagInfo) => {
    const s = useAppStore.getState();
    const isActive = s.tagFilter?.id === tag.id;
    s.setTagFilter(isActive ? null : { id: tag.id, name: tag.name });
    s.setCurrentView("gallery");
  };

  return (
    <div>
      <div className="flex items-center gap-1.5 px-2 py-1.5 rounded-md text-xs text-text-secondary hover:bg-bg-primary transition-colors">
        <button
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-1.5 flex-1 min-w-0 text-left"
        >
          {isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <Tag size={13} />
          <span className="flex-1 truncate">{t("sidebar.tags")}</span>
          {tagFilter && <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />}
        </button>
        <button
          onClick={() => useAppStore.getState().setCurrentView("tags")}
          className="text-[9px] text-text-secondary/70 hover:text-accent shrink-0 transition-colors"
        >
          {t("sidebar.manage")}
        </button>
      </div>
      {isOpen && (
        <div className="ml-6 space-y-0.5">
          {tags.length === 0 && (
            <p className="text-[10px] text-text-secondary/60 px-2 py-1">
              {t("sidebar.noTags")}
            </p>
          )}
          {tags.map((tag) => (
            <button
              key={tag.id}
              onClick={() => handleSelect(tag)}
              className={`w-full text-left px-2 py-1 rounded text-[11px] transition-colors ${
                tagFilter?.id === tag.id
                  ? "bg-accent/10 text-accent font-medium"
                  : "text-text-secondary hover:bg-bg-primary hover:text-text-primary"
              }`}
            >
              # {tag.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** 앨범 목록 — 클릭하면 갤러리를 해당 앨범으로 필터링한다. */
function AlbumFilterList() {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const albumFilter = useAppStore((s) => s.albumFilter);
  const refreshCounter = useAppStore((s) => s.refreshCounter);

  useEffect(() => {
    if (!isOpen) return;
    invoke<AlbumInfo[]>("get_albums")
      .then(setAlbums)
      .catch(() => {});
  }, [isOpen, refreshCounter]);

  const handleSelect = (album: AlbumInfo) => {
    const s = useAppStore.getState();
    const isActive = s.albumFilter?.id === album.id;
    s.setAlbumFilter(isActive ? null : { id: album.id, name: album.name });
    s.setCurrentView("gallery");
  };

  return (
    <div>
      <div className="flex items-center gap-1.5 px-2 py-1.5 rounded-md text-xs text-text-secondary hover:bg-bg-primary transition-colors">
        <button
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-1.5 flex-1 min-w-0 text-left"
        >
          {isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <FolderHeart size={13} />
          <span className="flex-1 truncate">{t("sidebar.albums")}</span>
          {albumFilter && <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />}
        </button>
        <button
          onClick={() => useAppStore.getState().setCurrentView("albums")}
          className="text-[9px] text-text-secondary/70 hover:text-accent shrink-0 transition-colors"
        >
          {t("sidebar.manage")}
        </button>
      </div>
      {isOpen && (
        <div className="ml-6 space-y-0.5">
          {albums.length === 0 && (
            <p className="text-[10px] text-text-secondary/60 px-2 py-1">
              {t("sidebar.noAlbums")}
            </p>
          )}
          {albums.map((album) => (
            <button
              key={album.id}
              onClick={() => handleSelect(album)}
              className={`w-full flex items-center justify-between gap-2 text-left px-2 py-1 rounded text-[11px] transition-colors ${
                albumFilter?.id === album.id
                  ? "bg-accent/10 text-accent font-medium"
                  : "text-text-secondary hover:bg-bg-primary hover:text-text-primary"
              }`}
            >
              <span className="truncate">{album.name}</span>
              <span className="text-[9px] text-text-secondary/60 tabular-nums shrink-0">
                {album.media_count}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

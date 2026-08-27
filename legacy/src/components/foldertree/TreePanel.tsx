import { useState, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { formatFileSize } from "@/utils/format";
import {
  ChevronRight,
  FileImage,
  FileVideo,
  Search,
  X,
  ArrowLeft,
  Home,
  FolderTree,
  FolderPlus,
  RefreshCw,
  Filter,
  Eye,
  EyeOff,
  AlertCircle,
  CheckSquare,
  XSquare,
  Copy,
  FolderInput,
  PencilLine,
  Trash2,
} from "lucide-react";
import { Folder, File as FileIcon } from "lucide-react";
import { TreeNodeRow } from "./TreeNodeRow";
import type { TreeNode, FileTypeFilter, FileOpResult, DirEntry } from "./types";

const FILE_TYPE_LABELS: Record<FileTypeFilter, string> = {
  all: "전체",
  image: "이미지",
  video: "영상",
  audio: "오디오",
  document: "문서",
  archive: "압축파일",
  other: "기타",
};

export function TreePanel() {
  const [rootNode, setRootNode] = useState<TreeNode | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<FileTypeFilter>("all");
  const [showFiles, setShowFiles] = useState(true);
  const [showFilterBar, setShowFilterBar] = useState(false);
  const [breadcrumbs, setBreadcrumbs] = useState<Array<{ name: string; path: string }>>([]);

  // File operations state
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [dropTargetPath, setDropTargetPath] = useState<string | null>(null);
  const [lastResult, setLastResult] = useState<string | null>(null);

  // Right-hand contents pane (dual-pane browser)
  const [browsePath, setBrowsePath] = useState<string | null>(null);
  const [browseEntries, setBrowseEntries] = useState<DirEntry[]>([]);
  const [browseTick, setBrowseTick] = useState(0);

  useEffect(() => {
    if (!browsePath) return;
    let cancelled = false;
    invoke<DirEntry[]>("list_directory", { path: browsePath })
      .then((entries) => {
        if (cancelled) return;
        const sorted = [...entries].sort((a, b) => {
          if (a.name === "..") return -1;
          if (b.name === "..") return 1;
          if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
          return a.name.localeCompare(b.name);
        });
        setBrowseEntries(sorted);
      })
      .catch(() => {
        if (!cancelled) setBrowseEntries([]);
      });
    return () => {
      cancelled = true;
    };
  }, [browsePath, browseTick]);

  const currentPath = breadcrumbs.length > 0 ? breadcrumbs[breadcrumbs.length - 1].path : null;

  const loadTree = useCallback(async (path: string, name?: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<TreeNode>("get_folder_tree", {
        path,
        maxDepth: 5,
      });
      setRootNode(result);
      setSelectedPaths(new Set());
      setLastResult(null);
      // Keep the contents pane where it was if still inside this tree
      setBrowsePath((prev) =>
        prev && (prev === path || prev.startsWith(path + "/")) ? prev : path,
      );
      setBrowseTick((t) => t + 1);

      const folderName = name ?? result.name;
      setBreadcrumbs((prev) => {
        const idx = prev.findIndex((b) => b.path === path);
        if (idx >= 0) return prev.slice(0, idx + 1);
        if (prev.length === 0) return [{ name: folderName, path }];
        return [...prev, { name: folderName, path }];
      });
    } catch {
      setError("폴더를 읽을 수 없습니다. 접근 권한을 확인해 주세요.");
    }
    setLoading(false);
  }, []);

  const handleSelectFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (!selected) return;

    setSearchQuery("");
    setTypeFilter("all");
    setBreadcrumbs([]);
    await loadTree(selected as string);
  };

  const handleNavigateTo = useCallback(
    (path: string, name: string) => {
      loadTree(path, name);
    },
    [loadTree]
  );

  const handleGoBack = useCallback(() => {
    if (breadcrumbs.length <= 1) return;
    const parent = breadcrumbs[breadcrumbs.length - 2];
    loadTree(parent.path, parent.name);
  }, [breadcrumbs, loadTree]);

  const handleRefresh = useCallback(() => {
    if (!currentPath) return;
    const currentName = breadcrumbs[breadcrumbs.length - 1]?.name;
    setBreadcrumbs((prev) => prev.slice(0, -1));
    loadTree(currentPath, currentName);
  }, [currentPath, breadcrumbs, loadTree]);

  // File selection
  const handleToggleSelect = useCallback((path: string, _shiftKey: boolean) => {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const handleSelectAll = useCallback(() => {
    if (!rootNode) return;
    const allFiles = new Set<string>();
    function collect(node: TreeNode) {
      if (!node.is_dir) {
        allFiles.add(node.path);
      }
      for (const child of node.children) collect(child);
    }
    for (const child of rootNode.children) collect(child);
    setSelectedPaths(allFiles);
  }, [rootNode]);

  const handleClearSelection = useCallback(() => {
    setSelectedPaths(new Set());
  }, []);

  // Drag & drop file operations
  const handleDragStart = useCallback(
    (e: React.DragEvent, node: TreeNode) => {
      const paths = selectedPaths.has(node.path)
        ? Array.from(selectedPaths)
        : [node.path];
      e.dataTransfer.setData("application/json", JSON.stringify(paths));
      e.dataTransfer.effectAllowed = "copyMove";
    },
    [selectedPaths]
  );

  const handleDragOverFolder = useCallback((path: string) => {
    setDropTargetPath(path);
  }, []);

  const handleDragEnd = useCallback(() => {
    setDropTargetPath(null);
  }, []);

  const handleDropOnFolder = useCallback(
    async (targetPath: string, e: React.DragEvent) => {
      setDropTargetPath(null);
      const data = e.dataTransfer.getData("application/json");
      if (!data) return;

      try {
        const sources: string[] = JSON.parse(data);
        const isMove = !e.altKey;

        let success = 0;
        let failed = 0;
        if (isMove) {
          // move_paths handles files AND folders, records undo,
          // and keeps library paths in sync
          const result = await invoke<FileOpResult>("move_paths", {
            sources,
            targetDir: targetPath,
          });
          success = result.success;
          failed = result.failed;
        } else {
          // copy_directory copies folder trees (and plain files)
          for (const source of sources) {
            try {
              await invoke("copy_directory", { source, targetDir: targetPath });
              success++;
            } catch {
              failed++;
            }
          }
        }

        setLastResult(
          `${isMove ? "이동" : "복사"} ${success}개 완료${
            failed > 0 ? `, ${failed}개 실패` : ""
          }`
        );
        setSelectedPaths(new Set());

        // Refresh tree
        if (currentPath) {
          const currentName = breadcrumbs[breadcrumbs.length - 1]?.name;
          setBreadcrumbs((prev) => prev.slice(0, -1));
          loadTree(currentPath, currentName);
        }
      } catch {
        setLastResult("작업 실패");
      }
    },
    [currentPath, breadcrumbs, loadTree]
  );

  // Also support dropping on the panel itself (root folder)
  const handlePanelDrop = useCallback(
    (e: React.DragEvent) => {
      if (!currentPath) return;
      e.preventDefault();
      handleDropOnFolder(currentPath, e);
    },
    [currentPath, handleDropOnFolder]
  );

  const handleCreateFolder = useCallback(async () => {
    if (!currentPath) return;
    const name = prompt("새 폴더 이름:");
    if (!name) return;

    try {
      await invoke("create_directory", { path: `${currentPath}/${name}` });
      handleRefresh();
    } catch {
      setError("폴더 생성에 실패했습니다.");
    }
  }, [currentPath, handleRefresh]);

  // ── Folder-level operations (selection-based) ──

  const handleCopyTo = useCallback(async () => {
    if (selectedPaths.size === 0) return;
    const target = await open({
      directory: true,
      multiple: false,
      title: "복사할 위치 선택",
    });
    if (!target) return;

    let success = 0;
    let failed = 0;
    for (const source of selectedPaths) {
      try {
        await invoke("copy_directory", {
          source,
          targetDir: target as string,
        });
        success++;
      } catch {
        failed++;
      }
    }
    setLastResult(
      `복사 ${success}개 완료${failed > 0 ? `, ${failed}개 실패` : ""}`
    );
    setSelectedPaths(new Set());
    handleRefresh();
  }, [selectedPaths, handleRefresh]);

  const handleMoveTo = useCallback(async () => {
    if (selectedPaths.size === 0) return;
    const target = await open({
      directory: true,
      multiple: false,
      title: "이동할 위치 선택",
    });
    if (!target) return;

    try {
      const result = await invoke<FileOpResult>("move_paths", {
        sources: Array.from(selectedPaths),
        targetDir: target as string,
      });
      setLastResult(
        `이동 ${result.success}개 완료${
          result.failed > 0 ? `, ${result.failed}개 실패` : ""
        } · 히스토리에서 되돌릴 수 있습니다`
      );
    } catch (e) {
      setLastResult(`이동 실패: ${e}`);
    }
    setSelectedPaths(new Set());
    handleRefresh();
  }, [selectedPaths, handleRefresh]);

  const handleRenameSelected = useCallback(async () => {
    if (selectedPaths.size !== 1) return;
    const path = Array.from(selectedPaths)[0];
    const current = path.split("/").pop() || "";
    const name = prompt("새 이름:", current);
    if (!name || name === current) return;

    try {
      await invoke("rename_path", { path, newName: name });
      setLastResult(`이름 변경 완료: ${name}`);
    } catch (e) {
      setLastResult(`${e}`);
    }
    setSelectedPaths(new Set());
    handleRefresh();
  }, [selectedPaths, handleRefresh]);

  const handleTrashSelected = useCallback(async () => {
    if (selectedPaths.size === 0) return;
    if (
      !confirm(
        `선택한 ${selectedPaths.size}개 항목을 휴지통으로 이동할까요?\n(폴더는 안의 내용까지 함께 이동됩니다)`
      )
    )
      return;

    try {
      const result = await invoke<FileOpResult>("trash_paths", {
        paths: Array.from(selectedPaths),
      });
      setLastResult(
        `휴지통 이동 ${result.success}개${
          result.failed > 0 ? `, ${result.failed}개 실패` : ""
        }`
      );
    } catch (e) {
      setLastResult(`${e}`);
    }
    setSelectedPaths(new Set());
    handleRefresh();
  }, [selectedPaths, handleRefresh]);

  // Pre-compute matching paths for O(N) filter
  const matchingPaths = useMemo(() => {
    if (!rootNode) return null;
    const lowerQuery = searchQuery.toLowerCase();
    if (!lowerQuery && typeFilter === "all") return null;

    const matching = new Set<string>();

    function mark(node: TreeNode): boolean {
      if (!node.is_dir) {
        const nameMatch = lowerQuery ? node.name.toLowerCase().includes(lowerQuery) : true;
        const typeMatch = typeFilter === "all" || node.file_type === typeFilter;
        if (nameMatch && typeMatch && showFiles) {
          matching.add(node.path);
          return true;
        }
        return false;
      }

      let hasMatch = lowerQuery ? node.name.toLowerCase().includes(lowerQuery) : false;
      for (const child of node.children) {
        if (mark(child)) hasMatch = true;
      }
      if (hasMatch) matching.add(node.path);
      return hasMatch;
    }

    for (const child of rootNode.children) mark(child);
    return matching;
  }, [rootNode, searchQuery, typeFilter, showFiles]);

  const stats = useMemo(() => {
    if (!rootNode) return null;
    return computeFilteredStats(rootNode, searchQuery, typeFilter);
  }, [rootNode, searchQuery, typeFilter]);

  if (!rootNode) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center">
          <FolderTree size={40} className="mx-auto mb-3 text-text-secondary/20" />
          <p className="text-xs text-text-secondary mb-1">폴더 구조를 탐색합니다</p>
          <p className="text-[10px] text-text-secondary/60 mb-3">
            트리 탐색 · 복사 / 이동 / 이름변경 / 휴지통 (⌘클릭으로 폴더 선택)
          </p>
          {error && (
            <div className="flex items-center gap-1.5 justify-center text-xs text-danger mb-3">
              <AlertCircle size={14} />
              {error}
            </div>
          )}
          <button
            onClick={handleSelectFolder}
            disabled={loading}
            className="px-5 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
          >
            {loading ? (
              <span className="flex items-center gap-2">
                <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                로딩 중...
              </span>
            ) : (
              <span className="flex items-center gap-2">
                <FolderTree size={14} />
                폴더 선택
              </span>
            )}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div
      className="flex-1 flex flex-col overflow-hidden"
      onDragOver={(e) => e.preventDefault()}
      onDrop={handlePanelDrop}
    >
      {/* Header with breadcrumbs + actions */}
      <div className="p-2 border-b border-border bg-bg-secondary/50">
        <div className="flex items-center gap-1 mb-1.5">
          <button
            onClick={handleGoBack}
            disabled={breadcrumbs.length <= 1}
            className="p-1 rounded hover:bg-bg-primary disabled:opacity-30 transition-colors"
            title="뒤로"
          >
            <ArrowLeft size={13} />
          </button>
          <button
            onClick={() =>
              breadcrumbs.length > 0 &&
              handleNavigateTo(breadcrumbs[0].path, breadcrumbs[0].name)
            }
            className="p-1 rounded hover:bg-bg-primary transition-colors"
            title="루트로"
          >
            <Home size={13} />
          </button>
          <button
            onClick={handleRefresh}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary transition-colors"
            title="새로고침"
          >
            <RefreshCw size={12} />
          </button>
          <button
            onClick={handleCreateFolder}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary transition-colors"
            title="새 폴더"
          >
            <FolderPlus size={12} />
          </button>

          {/* Folder operations (selection-based) */}
          <div className="w-px h-3.5 bg-border mx-0.5" />
          <button
            onClick={handleCopyTo}
            disabled={selectedPaths.size === 0}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary disabled:opacity-30 transition-colors"
            title="선택 항목 복사 (폴더 구조 포함)"
          >
            <Copy size={12} />
          </button>
          <button
            onClick={handleMoveTo}
            disabled={selectedPaths.size === 0}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary disabled:opacity-30 transition-colors"
            title="선택 항목 이동"
          >
            <FolderInput size={12} />
          </button>
          <button
            onClick={handleRenameSelected}
            disabled={selectedPaths.size !== 1}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary disabled:opacity-30 transition-colors"
            title="이름 변경 (1개 선택 시)"
          >
            <PencilLine size={12} />
          </button>
          <button
            onClick={handleTrashSelected}
            disabled={selectedPaths.size === 0}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary hover:text-danger disabled:opacity-30 transition-colors"
            title="휴지통으로 이동"
          >
            <Trash2 size={12} />
          </button>
          {selectedPaths.size > 0 && (
            <span className="text-[9px] text-accent tabular-nums shrink-0">
              {selectedPaths.size} 선택
            </span>
          )}

          {/* Breadcrumbs */}
          <div className="flex items-center gap-1 text-[11px] text-text-secondary overflow-x-auto flex-1 min-w-0 ml-1">
            {breadcrumbs.map((crumb, idx) => (
              <span key={crumb.path} className="flex items-center gap-1 shrink-0">
                {idx > 0 && <ChevronRight size={10} className="text-text-secondary/40" />}
                <button
                  onClick={() => handleNavigateTo(crumb.path, crumb.name)}
                  className={`hover:text-accent transition-colors truncate max-w-[100px] ${
                    idx === breadcrumbs.length - 1 ? "text-text-primary font-medium" : ""
                  }`}
                  title={crumb.path}
                >
                  {crumb.name}
                </button>
              </span>
            ))}
          </div>

          <button
            onClick={handleSelectFolder}
            className="px-2 py-0.5 rounded text-[10px] bg-accent text-white hover:bg-accent-hover transition-colors shrink-0"
          >
            다른 폴더
          </button>
        </div>

        {/* Error banner */}
        {error && (
          <div className="flex items-center gap-1.5 text-xs text-danger mb-1.5 px-1">
            <AlertCircle size={12} />
            {error}
          </div>
        )}

        {/* Search & filter bar */}
        <div className="flex items-center gap-1.5">
          <div className="flex-1 relative">
            <Search size={11} className="absolute left-2 top-1/2 -translate-y-1/2 text-text-secondary/50" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="검색..."
              className="w-full pl-6 pr-6 py-1 text-[11px] rounded-md bg-bg-primary border border-border focus:border-accent focus:outline-none"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-text-secondary hover:text-text-primary"
              >
                <X size={11} />
              </button>
            )}
          </div>
          <button
            onClick={() => setShowFilterBar(!showFilterBar)}
            className={`p-1 rounded transition-colors ${
              showFilterBar || typeFilter !== "all"
                ? "bg-accent/10 text-accent"
                : "hover:bg-bg-primary text-text-secondary"
            }`}
            title="필터"
          >
            <Filter size={13} />
          </button>
          <button
            onClick={() => setShowFiles(!showFiles)}
            className={`p-1 rounded transition-colors ${
              showFiles ? "text-text-secondary hover:bg-bg-primary" : "bg-accent/10 text-accent"
            }`}
            title={showFiles ? "파일 숨기기" : "파일 보기"}
          >
            {showFiles ? <Eye size={13} /> : <EyeOff size={13} />}
          </button>
          <button
            onClick={handleSelectAll}
            className="p-1 rounded hover:bg-bg-primary text-text-secondary transition-colors"
            title="전체 선택"
          >
            <CheckSquare size={13} />
          </button>
          {selectedPaths.size > 0 && (
            <button
              onClick={handleClearSelection}
              className="p-1 rounded hover:bg-bg-primary text-text-secondary transition-colors"
              title="선택 해제"
            >
              <XSquare size={13} />
            </button>
          )}
        </div>

        {/* Type filter chips */}
        {showFilterBar && (
          <div className="flex items-center gap-1 mt-1.5 flex-wrap">
            {(Object.keys(FILE_TYPE_LABELS) as FileTypeFilter[]).map((type) => (
              <button
                key={type}
                onClick={() => setTypeFilter(type)}
                className={`px-1.5 py-0.5 rounded-full text-[10px] font-medium transition-colors ${
                  typeFilter === type
                    ? "bg-accent text-white"
                    : "bg-bg-primary text-text-secondary hover:bg-bg-secondary"
                }`}
              >
                {FILE_TYPE_LABELS[type]}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Stats bar */}
      {stats && (
        <div className="px-2 py-1 border-b border-border bg-bg-secondary/30 flex items-center gap-3 text-[10px] text-text-secondary">
          <span>
            <strong className="text-text-primary">{stats.folders.toLocaleString()}</strong> 폴더
          </span>
          <span>
            <strong className="text-text-primary">{stats.files.toLocaleString()}</strong> 파일
          </span>
          <span>
            <strong className="text-text-primary">{formatFileSize(stats.totalSize)}</strong>
          </span>
          {stats.images > 0 && (
            <span className="flex items-center gap-0.5">
              <FileImage size={9} className="text-blue-400" />
              {stats.images.toLocaleString()}
            </span>
          )}
          {stats.videos > 0 && (
            <span className="flex items-center gap-0.5">
              <FileVideo size={9} className="text-purple-400" />
              {stats.videos.toLocaleString()}
            </span>
          )}
          {searchQuery && (
            <span className="text-accent">
              &quot;{searchQuery}&quot;
            </span>
          )}
        </div>
      )}

      {/* Dual pane: folder tree (left) + folder contents (right) */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left: tree */}
        <div className="w-[42%] min-w-[240px] max-w-[440px] overflow-y-auto p-1 border-r border-border">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <div className="w-5 h-5 border-2 border-accent border-t-transparent rounded-full animate-spin" />
              <span className="ml-2 text-xs text-text-secondary">분석 중...</span>
            </div>
          ) : (
            rootNode.children.map((child) => (
              <TreeNodeRow
                key={child.path}
                node={child}
                depth={0}
                searchQuery={searchQuery.toLowerCase()}
                typeFilter={typeFilter}
                showFiles={showFiles}
                matchingPaths={matchingPaths}
                selectedPaths={selectedPaths}
                dropTargetPath={dropTargetPath}
                onNavigate={handleNavigateTo}
                onBrowse={setBrowsePath}
                onToggleSelect={handleToggleSelect}
                onDragStart={handleDragStart}
                onDragOverFolder={handleDragOverFolder}
                onDropOnFolder={handleDropOnFolder}
                onDragEnd={handleDragEnd}
              />
            ))
          )}
          {!loading && rootNode.children.length === 0 && (
            <div className="text-center py-8 text-xs text-text-secondary">
              이 폴더는 비어있습니다
            </div>
          )}
        </div>

        {/* Right: contents of the browsed folder */}
        <div className="flex-1 flex flex-col overflow-hidden min-w-0">
          <div className="px-3 py-1.5 border-b border-border/50 flex items-center gap-2 text-[10px] text-text-secondary shrink-0">
            <Folder size={11} className="text-accent shrink-0" />
            <span className="truncate flex-1" title={browsePath ?? ""}>
              {browsePath
                ? currentPath && browsePath.startsWith(currentPath)
                  ? browsePath.slice(currentPath.length).replace(/^\//, "") ||
                    rootNode.name
                  : browsePath
                : ""}
            </span>
            <span className="tabular-nums shrink-0">
              {browseEntries.filter((e) => e.name !== "..").length}개 항목
            </span>
          </div>
          <div
            className="flex-1 overflow-y-auto"
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              if (browsePath) {
                e.preventDefault();
                e.stopPropagation();
                handleDropOnFolder(browsePath, e);
              }
            }}
          >
            {browseEntries.map((entry) => {
              const isParent = entry.name === "..";
              const isSelected = selectedPaths.has(entry.path);
              return (
                <div
                  key={entry.path + entry.name}
                  className={`flex items-center gap-2 px-3 py-1 cursor-pointer text-[11px] transition-colors ${
                    isSelected
                      ? "bg-accent/10 text-accent"
                      : "hover:bg-bg-secondary/60 text-text-primary"
                  }`}
                  onClick={(e) => {
                    if (isParent) return;
                    if (e.metaKey || e.ctrlKey) {
                      handleToggleSelect(entry.path, e.shiftKey);
                    } else {
                      setSelectedPaths(new Set([entry.path]));
                    }
                  }}
                  onDoubleClick={() => {
                    if (entry.is_dir) {
                      setBrowsePath(entry.path);
                    } else {
                      invoke("preview_file", { path: entry.path }).catch(
                        () => {},
                      );
                    }
                  }}
                  draggable={!isParent}
                  onDragStart={(e) => {
                    const paths = selectedPaths.has(entry.path)
                      ? Array.from(selectedPaths)
                      : [entry.path];
                    e.dataTransfer.setData(
                      "application/json",
                      JSON.stringify(paths),
                    );
                    e.dataTransfer.effectAllowed = "copyMove";
                  }}
                  onDragOver={
                    entry.is_dir && !isParent
                      ? (e) => {
                          e.preventDefault();
                          e.stopPropagation();
                        }
                      : undefined
                  }
                  onDrop={
                    entry.is_dir && !isParent
                      ? (e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          handleDropOnFolder(entry.path, e);
                        }
                      : undefined
                  }
                  title={entry.path}
                >
                  {entry.is_dir ? (
                    <Folder
                      size={13}
                      className={`shrink-0 ${isParent ? "text-text-secondary/50" : "text-accent"}`}
                    />
                  ) : (
                    <FileIcon size={13} className="text-text-secondary shrink-0" />
                  )}
                  <span className="flex-1 truncate">{entry.name}</span>
                  {!entry.is_dir && (
                    <span className="text-[9px] text-text-secondary/70 tabular-nums shrink-0">
                      {formatFileSize(entry.size)}
                    </span>
                  )}
                </div>
              );
            })}
            {browseEntries.length === 0 && (
              <div className="text-center py-8 text-xs text-text-secondary">
                이 폴더는 비어있습니다
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Status bar */}
      <div className="flex items-center gap-2 px-2 py-1 border-t border-border text-[10px] text-text-secondary bg-bg-secondary/50">
        {selectedPaths.size > 0 && (
          <span className="text-accent font-medium">
            {selectedPaths.size}개 선택 — 폴더에 드래그하여 이동 (Alt+드롭: 복사)
          </span>
        )}
        {lastResult && (
          <span className="text-success ml-auto">{lastResult}</span>
        )}
        {selectedPaths.size === 0 && !lastResult && (
          <span>파일을 클릭하여 선택, 더블클릭으로 미리보기</span>
        )}
      </div>
    </div>
  );
}

interface FilteredStats {
  folders: number;
  files: number;
  totalSize: number;
  images: number;
  videos: number;
}

function computeFilteredStats(
  node: TreeNode,
  query: string,
  typeFilter: FileTypeFilter
): FilteredStats {
  const lowerQuery = query.toLowerCase();

  function walk(n: TreeNode): FilteredStats {
    if (n.is_dir) {
      return n.children.reduce<FilteredStats>(
        (acc, child) => {
          const childStats = walk(child);
          return {
            folders: acc.folders + childStats.folders,
            files: acc.files + childStats.files,
            totalSize: acc.totalSize + childStats.totalSize,
            images: acc.images + childStats.images,
            videos: acc.videos + childStats.videos,
          };
        },
        { folders: 1, files: 0, totalSize: 0, images: 0, videos: 0 }
      );
    }

    const nameMatch = lowerQuery ? n.name.toLowerCase().includes(lowerQuery) : true;
    const typeMatch = typeFilter === "all" || n.file_type === typeFilter;
    if (nameMatch && typeMatch) {
      return {
        folders: 0,
        files: 1,
        totalSize: n.size,
        images: n.file_type === "image" ? 1 : 0,
        videos: n.file_type === "video" ? 1 : 0,
      };
    }
    return { folders: 0, files: 0, totalSize: 0, images: 0, videos: 0 };
  }

  return node.children.reduce<FilteredStats>(
    (acc, child) => {
      const childStats = walk(child);
      return {
        folders: acc.folders + childStats.folders,
        files: acc.files + childStats.files,
        totalSize: acc.totalSize + childStats.totalSize,
        images: acc.images + childStats.images,
        videos: acc.videos + childStats.videos,
      };
    },
    { folders: 0, files: 0, totalSize: 0, images: 0, videos: 0 }
  );
}

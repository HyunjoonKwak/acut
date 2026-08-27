import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Folder,
  FolderPlus,
  ChevronRight,
  Loader2,
  Check,
  CornerLeftUp,
} from "lucide-react";
import type { NasEntry } from "@/types";

interface Props {
  destRoot: string;
  onSelect: (path: string) => void;
}

export function NasFolderBrowser({ destRoot, onSelect }: Props) {
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [entries, setEntries] = useState<NasEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [creating, setCreating] = useState(false);

  const load = useCallback(async (path: string | null) => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<NasEntry[]>("nas_list_folders", { path });
      setEntries(list);
      setCurrentPath(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load(null);
  }, [load]);

  const goUp = () => {
    if (!currentPath) return;
    const parent = currentPath.split("/").slice(0, -1).join("/");
    // Share roots look like "/photo"; going above them returns to share list
    load(parent.split("/").filter(Boolean).length >= 1 ? parent : null);
  };

  const handleCreateFolder = async () => {
    const name = newFolderName.trim();
    if (!currentPath || !name) return;
    setCreating(true);
    setError(null);
    try {
      const created = await invoke<string>("nas_create_folder", {
        parent: currentPath,
        name,
      });
      setNewFolderName("");
      setNewFolderOpen(false);
      await load(currentPath);
      onSelect(created);
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div className="px-4 pt-3 pb-2 flex items-center justify-between">
        <span className="text-[11px] font-semibold text-text-primary">
          대상 폴더 선택
        </span>
        {currentPath && (
          <button
            onClick={() => setNewFolderOpen((v) => !v)}
            title="새 폴더"
            className="p-1 rounded text-text-secondary hover:text-text-primary hover:bg-bg-primary transition-colors"
          >
            <FolderPlus size={13} />
          </button>
        )}
      </div>

      {/* Breadcrumb */}
      <div className="px-4 pb-2 flex items-center gap-1 flex-wrap text-[10px] text-text-secondary">
        <button
          onClick={() => load(null)}
          className={`hover:text-accent ${!currentPath ? "text-accent font-medium" : ""}`}
        >
          공유 폴더
        </button>
        {currentPath &&
          currentPath
            .split("/")
            .filter(Boolean)
            .map((seg, i, arr) => {
              const path = "/" + arr.slice(0, i + 1).join("/");
              return (
                <span key={path} className="flex items-center gap-1">
                  <ChevronRight size={9} />
                  <button
                    onClick={() => load(path)}
                    className={`hover:text-accent ${i === arr.length - 1 ? "text-text-primary font-medium" : ""}`}
                  >
                    {seg}
                  </button>
                </span>
              );
            })}
      </div>

      {/* Select current folder */}
      {currentPath && (
        <div className="px-4 pb-2 flex items-center gap-1.5">
          <button
            onClick={() => onSelect(currentPath)}
            className={`flex-1 flex items-center justify-center gap-1.5 h-6 rounded-md text-[10px] font-medium border transition-colors ${
              destRoot === currentPath
                ? "bg-accent/10 border-accent text-accent"
                : "border-border text-text-secondary hover:border-accent/50 hover:text-text-primary"
            }`}
          >
            {destRoot === currentPath && <Check size={11} />}이 폴더로 업로드
          </button>
          <button
            onClick={goUp}
            title="상위 폴더"
            className="p-1 rounded text-text-secondary hover:text-text-primary hover:bg-bg-primary transition-colors"
          >
            <CornerLeftUp size={13} />
          </button>
        </div>
      )}

      {/* New folder input */}
      {newFolderOpen && currentPath && (
        <div className="px-4 pb-2 flex items-center gap-1.5">
          <input
            type="text"
            autoFocus
            placeholder="새 폴더 이름"
            value={newFolderName}
            onChange={(e) => setNewFolderName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleCreateFolder()}
            className="flex-1 h-6 px-2 rounded-md text-[10px] bg-bg-primary border border-border text-text-primary focus:outline-none focus:ring-1 focus:ring-accent/50"
          />
          <button
            onClick={handleCreateFolder}
            disabled={creating || !newFolderName.trim()}
            className="h-6 px-2 rounded-md text-[10px] bg-accent text-white disabled:opacity-50"
          >
            {creating ? "..." : "생성"}
          </button>
        </div>
      )}

      {error && (
        <p className="px-4 pb-2 text-[10px] text-danger break-all">{error}</p>
      )}

      {/* Folder list */}
      <div className="flex-1 overflow-y-auto px-2 pb-3">
        {loading ? (
          <div className="flex items-center justify-center py-6 text-text-secondary">
            <Loader2 size={16} className="animate-spin" />
          </div>
        ) : entries.length === 0 ? (
          <p className="px-2 py-4 text-[10px] text-text-secondary/70 text-center">
            하위 폴더가 없습니다
          </p>
        ) : (
          entries.map((entry) => (
            <button
              key={entry.path}
              onClick={() => load(entry.path)}
              onDoubleClick={() => onSelect(entry.path)}
              className={`w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-left transition-colors ${
                destRoot === entry.path
                  ? "bg-accent/10 text-accent"
                  : "text-text-primary hover:bg-bg-primary"
              }`}
            >
              <Folder
                size={13}
                className={
                  destRoot === entry.path ? "text-accent" : "text-warning"
                }
              />
              <span className="text-[11px] truncate flex-1">{entry.name}</span>
              {destRoot === entry.path && <Check size={11} />}
            </button>
          ))
        )}
      </div>
    </div>
  );
}

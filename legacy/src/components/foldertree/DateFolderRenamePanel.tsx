import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  FolderSearch,
  ArrowRight,
  Check,
  AlertTriangle,
  FolderOpen,
  RotateCcw,
  CalendarDays,
  GitMerge,
} from "lucide-react";

interface DateFolderMatch {
  path: string;
  current_name: string;
  suggested_name: string;
  has_conflict: boolean;
  conflict_file_count: number;
}

interface ScanResult {
  matches: DateFolderMatch[];
  total_scanned: number;
}

interface RenameResult {
  success: number;
  failed: number;
  errors: string[];
}

export function DateFolderRenamePanel() {
  const [targetPath, setTargetPath] = useState("");
  const [recursive, setRecursive] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [renameResult, setRenameResult] = useState<RenameResult | null>(null);
  const [mergeConflicts, setMergeConflicts] = useState(true);

  const handleSelectFolder = async () => {
    const result = await open({ directory: true, multiple: false });
    if (result) {
      setTargetPath(result as string);
      setScanResult(null);
      setRenameResult(null);
    }
  };

  const handleScan = async () => {
    if (!targetPath) return;
    setScanning(true);
    setScanResult(null);
    setRenameResult(null);
    try {
      const result = await invoke<ScanResult>("scan_date_folders", {
        path: targetPath,
        recursive,
      });
      setScanResult(result);
      setSelected(new Set(result.matches.map((_, i) => i)));
    } catch {
      // Handle error
    }
    setScanning(false);
  };

  const handleRename = async () => {
    if (!scanResult) return;
    const folders = scanResult.matches.filter((_, i) => selected.has(i));
    if (folders.length === 0) return;

    setRenaming(true);
    setRenameResult(null);
    try {
      const result = await invoke<RenameResult>("rename_date_folders", { folders, merge: mergeConflicts });
      setRenameResult(result);
      if (result.success > 0) {
        await handleScan();
      }
    } catch {
      setRenameResult({ success: 0, failed: 0, errors: ["Rename failed"] });
    }
    setRenaming(false);
  };

  const toggleSelect = (idx: number) => {
    const next = new Set(selected);
    if (next.has(idx)) {
      next.delete(idx);
    } else {
      next.add(idx);
    }
    setSelected(next);
  };

  const toggleAll = () => {
    if (!scanResult) return;
    if (selected.size === scanResult.matches.length) {
      setSelected(new Set());
    } else {
      setSelected(new Set(scanResult.matches.map((_, i) => i)));
    }
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Empty state: centered folder select */}
      {!targetPath && (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center">
            <CalendarDays size={40} className="mx-auto mb-3 text-text-secondary/30" />
            <p className="text-sm text-text-secondary mb-1">날짜 폴더 이름 정리</p>
            <p className="text-[10px] text-text-secondary/70 mb-4">
              <code className="bg-bg-secondary px-1 py-0.5 rounded">20260325</code>
              {" → "}
              <code className="bg-bg-secondary px-1 py-0.5 rounded">2026-03-25</code>
              {" "}형식으로 변환합니다
            </p>
            <button
              onClick={handleSelectFolder}
              className="flex items-center gap-2 mx-auto px-4 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors"
            >
              <FolderOpen size={14} />
              폴더 선택
            </button>
          </div>
        </div>
      )}

      {/* Controls bar (shown after folder selected) */}
      {targetPath && (
        <div className="p-3 border-b border-border space-y-2">
          <div className="flex items-center gap-2">
            <button
              onClick={handleSelectFolder}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-bg-secondary hover:bg-bg-primary text-text-primary transition-colors shrink-0"
            >
              <FolderOpen size={13} />
              폴더 변경
            </button>
            <span className="text-[10px] text-text-secondary truncate flex-1">
              {targetPath}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <label className="flex items-center gap-1.5 text-[11px] text-text-secondary cursor-pointer">
              <input
                type="checkbox"
                checked={recursive}
                onChange={(e) => setRecursive(e.target.checked)}
                className="rounded"
              />
              하위 폴더 포함
            </label>
            <button
              onClick={handleScan}
              disabled={!targetPath || scanning}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
            >
              {scanning ? (
                <>
                  <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                  스캔 중...
                </>
              ) : (
                <>
                  <FolderSearch size={13} />
                  날짜 폴더 스캔
                </>
              )}
            </button>
          </div>
        </div>
      )}

      {/* Results */}
      <div className="flex-1 overflow-y-auto">
        {scanResult && scanResult.matches.length === 0 && (
          <div className="flex-1 flex items-center justify-center p-8">
            <div className="text-center">
              <Check size={32} className="mx-auto mb-2 text-success" />
              <p className="text-sm text-text-primary font-medium">변환 대상 없음</p>
              <p className="text-[10px] text-text-secondary mt-1">
                {scanResult.total_scanned}개 폴더 스캔 완료. 모든 폴더가 올바른 형식입니다.
              </p>
            </div>
          </div>
        )}

        {scanResult && scanResult.matches.length > 0 && (
          <>
            {/* Summary bar */}
            <div className="p-2 border-b border-border bg-bg-secondary/30 space-y-1.5">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3 text-[11px]">
                  <button
                    onClick={toggleAll}
                    className="text-accent hover:underline"
                  >
                    {selected.size === scanResult.matches.length ? "전체 해제" : "전체 선택"}
                  </button>
                  <span className="text-text-secondary">
                    {scanResult.total_scanned}개 스캔 ·{" "}
                    <strong className="text-warning">{scanResult.matches.length}개</strong> 변환 대상 ·{" "}
                    <strong className="text-accent">{selected.size}개</strong> 선택
                    {scanResult.matches.some((m) => m.has_conflict) && (
                      <>
                        {" · "}
                        <strong className="text-orange-400">
                          {scanResult.matches.filter((m) => m.has_conflict).length}개
                        </strong>{" "}
                        충돌
                      </>
                    )}
                  </span>
                </div>
                <button
                  onClick={handleRename}
                  disabled={renaming || selected.size === 0}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
                >
                  {renaming ? (
                    <>
                      <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                      변환 중...
                    </>
                  ) : (
                    <>
                      <RotateCcw size={12} />
                      {selected.size}개 이름 변환
                    </>
                  )}
                </button>
              </div>
              {scanResult.matches.some((m) => m.has_conflict) && (
                <label className="flex items-center gap-1.5 text-[11px] text-text-secondary cursor-pointer">
                  <input
                    type="checkbox"
                    checked={mergeConflicts}
                    onChange={(e) => setMergeConflicts(e.target.checked)}
                    className="rounded"
                  />
                  <GitMerge size={11} className="text-orange-400" />
                  동일 이름 폴더가 있으면 내용을 병합 (체크 해제 시 건너뜀)
                </label>
              )}
            </div>

            {/* Match list */}
            <div className="divide-y divide-border/50">
              {scanResult.matches.map((match, idx) => (
                <label
                  key={match.path}
                  className={`flex items-center gap-3 px-3 py-2 cursor-pointer transition-colors ${
                    selected.has(idx)
                      ? "bg-accent/5"
                      : "hover:bg-bg-secondary/50"
                  }`}
                >
                  <input
                    type="checkbox"
                    checked={selected.has(idx)}
                    onChange={() => toggleSelect(idx)}
                    className="rounded shrink-0"
                  />
                  <div className="flex items-center gap-2 min-w-0 flex-1">
                    <span className="text-xs font-mono text-danger truncate">
                      {match.current_name}
                    </span>
                    <ArrowRight size={12} className="text-text-secondary shrink-0" />
                    <span className="text-xs font-mono text-success truncate">
                      {match.suggested_name}
                    </span>
                    {match.has_conflict && (
                      <span className="flex items-center gap-0.5 text-[9px] text-orange-400 bg-orange-400/10 px-1.5 py-0.5 rounded-full shrink-0">
                        <GitMerge size={9} />
                        병합 ({match.conflict_file_count}개 파일)
                      </span>
                    )}
                  </div>
                  <span className="text-[9px] text-text-secondary/60 truncate max-w-[200px]">
                    {match.path.replace(targetPath, "").replace(/^\//, "")}
                  </span>
                </label>
              ))}
            </div>
          </>
        )}
      </div>

      {/* Rename result toast */}
      {renameResult && (
        <div className={`p-2 border-t text-xs flex items-center gap-2 ${
          renameResult.failed > 0 ? "border-warning bg-warning/5" : "border-success bg-success/5"
        }`}>
          {renameResult.failed > 0 ? (
            <AlertTriangle size={13} className="text-warning shrink-0" />
          ) : (
            <Check size={13} className="text-success shrink-0" />
          )}
          <span>
            {renameResult.success}개 변환 완료
            {renameResult.failed > 0 && `, ${renameResult.failed}개 실패`}
          </span>
          {renameResult.errors.length > 0 && (
            <span className="text-[10px] text-text-secondary truncate">
              ({renameResult.errors[0]})
            </span>
          )}
        </div>
      )}
    </div>
  );
}

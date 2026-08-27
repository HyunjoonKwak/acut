import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatFileSize, formatDate } from "@/utils/format";
import { thumbSrc } from "@/utils/media";
import { MemberPreview } from "@/components/culling/MemberPreview";
import { toast } from "@/stores/toastStore";
import { useDuplicateProgress } from "@/hooks/useTauriEvents";
import {
  Copy,
  Search,
  Check,
  Trash2,
  Shield,
  TrendingDown,
  FileImage,
  Eye,
  FolderOpen,
  AlertTriangle,
  SkipForward,
} from "lucide-react";

interface DupMember {
  media_id: string;
  is_preferred: boolean;
  file_path: string;
  file_name: string;
  file_size: number;
  width: number | null;
  height: number | null;
  date_taken: string | null;
  thumbnail: string | null;
  media_type: string;
}

interface DupGroup {
  id: string;
  match_type: string;
  similarity_score: number | null;
  members: DupMember[];
}

interface DupSummary {
  groups: DupGroup[];
  total_groups: number;
  total_duplicates: number;
  total_savings: number;
  total_files_in_groups: number;
}

interface TrashResult {
  success: number;
  failed: number;
  errors: string[];
}

export function DuplicatesView() {
  const [summary, setSummary] = useState<DupSummary | null>(null);
  const [selectedGroup, setSelectedGroup] = useState<DupGroup | null>(null);
  const [selectedIdx, setSelectedIdx] = useState(0);
  // Member currently shown large in the preview pane
  const [focusIdx, setFocusIdx] = useState(0);
  const [scanning, setScanning] = useState(false);
  const [trashing, setTrashing] = useState(false);
  const [trashResult, setTrashResult] = useState<string | null>(null);
  const dupProgress = useDuplicateProgress();

  const loadGroups = async () => {
    try {
      const result = await invoke<DupSummary>("get_duplicate_groups");
      setSummary(result);
      if (result.groups.length > 0) {
        setSelectedGroup(result.groups[0]);
        setSelectedIdx(0);
      } else {
        setSelectedGroup(null);
        setSelectedIdx(0);
      }
    } catch (e) {
      toast.error(`중복 그룹을 불러오지 못했습니다: ${e}`);
    }
  };

  const handleScan = async () => {
    setScanning(true);
    setSummary(null);
    setSelectedGroup(null);
    setTrashResult(null);
    try {
      await invoke("detect_duplicates");
      await loadGroups();
    } catch (e) {
      toast.error(`중복 탐지에 실패했습니다: ${e}`);
    }
    setScanning(false);
  };

  const handlePreview = async (path: string) => {
    await invoke("preview_file", { path });
  };

  const handleOpenFile = async (path: string) => {
    await invoke("open_file", { path });
  };

  // Change which file to keep in a group
  const handleSetPreferred = async (groupId: string, mediaId: string) => {
    try {
      await invoke("set_preferred_member", { groupId, mediaId });
      const result = await invoke<DupSummary>("get_duplicate_groups");
      setSummary(result);
      const updated = result.groups.find((g) => g.id === groupId);
      if (updated) {
        setSelectedGroup(updated);
      }
    } catch {
      // Handle error
    }
  };

  // Skip this group (keep all, remove from list)
  const handleDismissGroup = async (groupId: string) => {
    try {
      await invoke("dismiss_duplicate_group", { groupId });
      const result = await invoke<DupSummary>("get_duplicate_groups");
      setSummary(result);
      if (result.groups.length > 0) {
        const nextIdx = Math.min(selectedIdx, result.groups.length - 1);
        setSelectedGroup(result.groups[nextIdx]);
        setSelectedIdx(nextIdx);
      } else {
        setSelectedGroup(null);
        setSelectedIdx(0);
      }
    } catch {
      // Handle error
    }
  };

  // Trash non-preferred files in a single group
  const handleTrashGroup = async (group: DupGroup) => {
    const toDelete = group.members.filter((m) => !m.is_preferred);
    if (toDelete.length === 0) return;
    const confirmed = confirm(
      `이 그룹에서 ${toDelete.length}개 파일을 휴지통으로 이동합니다.\n` +
      `(${formatFileSize(toDelete.reduce((s, m) => s + m.file_size, 0))} 절약)\n\n계속하시겠습니까?`
    );
    if (!confirmed) return;

    setTrashing(true);
    try {
      const result = await invoke<TrashResult>("trash_group_duplicates", { groupId: group.id });
      setTrashResult(
        `${result.success}개 파일 휴지통 이동 완료` +
        (result.failed > 0 ? ` (${result.failed}개 실패)` : "")
      );
      await loadGroups();
    } catch (e) {
      setTrashResult("휴지통 이동 실패");
      toast.error(`휴지통 이동에 실패했습니다: ${e}`);
    }
    setTrashing(false);
  };

  // Trash all non-preferred files across all groups
  const handleTrashAll = async () => {
    if (!summary || summary.groups.length === 0) return;
    const confirmed = confirm(
      `${summary.total_duplicates}개 중복 파일을 휴지통으로 이동합니다.\n` +
      `${formatFileSize(summary.total_savings)} 용량이 확보됩니다.\n\n` +
      `각 그룹에서 "유지" 표시된 파일만 남고,\n"삭제 예정" 파일이 휴지통으로 이동됩니다.\n\n계속하시겠습니까?`
    );
    if (!confirmed) return;

    setTrashing(true);
    setTrashResult(null);
    try {
      const result = await invoke<TrashResult>("trash_duplicate_files");
      setTrashResult(
        `${result.success}개 파일 휴지통으로 이동 완료` +
        (result.failed > 0 ? ` (${result.failed}개 실패)` : "")
      );
      await loadGroups();
    } catch (e) {
      setTrashResult("휴지통 이동 실패");
      toast.error(`휴지통 이동에 실패했습니다: ${e}`);
    }
    setTrashing(false);
  };

  // Stats: files marked for deletion
  const deleteStats = useMemo(() => {
    if (!summary) return { count: 0, size: 0 };
    let count = 0;
    let size = 0;
    for (const group of summary.groups) {
      for (const member of group.members) {
        if (!member.is_preferred) {
          count++;
          size += member.file_size;
        }
      }
    }
    return { count, size };
  }, [summary]);

  // Keyboard shortcuts
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!summary || !selectedGroup) return;
      const groups = summary.groups;

      if (e.code === "Space") {
        e.preventDefault();
        const focused = selectedGroup.members[focusIdx];
        if (focused) {
          handlePreview(focused.file_path);
        }
      }
      if (e.code === "ArrowLeft" || e.code === "KeyH") {
        e.preventDefault();
        setFocusIdx((i) => Math.max(0, i - 1));
      }
      if (e.code === "ArrowRight" || e.code === "KeyL") {
        e.preventDefault();
        setFocusIdx((i) =>
          Math.min(selectedGroup.members.length - 1, i + 1),
        );
      }
      if (e.code === "ArrowDown" || e.code === "KeyJ") {
        e.preventDefault();
        const next = Math.min(selectedIdx + 1, groups.length - 1);
        setSelectedIdx(next);
        setSelectedGroup(groups[next]);
      }
      if (e.code === "ArrowUp" || e.code === "KeyK") {
        e.preventDefault();
        const prev = Math.max(selectedIdx - 1, 0);
        setSelectedIdx(prev);
        setSelectedGroup(groups[prev]);
      }
      if (e.code === "KeyS") {
        e.preventDefault();
        handleDismissGroup(selectedGroup.id);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [summary, selectedGroup, selectedIdx, focusIdx]
  );

  // Reset member focus when the group changes
  useEffect(() => {
    setFocusIdx(0);
  }, [selectedGroup?.id]);

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  useEffect(() => {
    loadGroups();
  }, []);

  const groups = summary?.groups ?? [];

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="p-3 border-b border-border flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Copy size={20} className="text-accent" />
          <h2 className="text-sm font-semibold text-text-primary">중복 탐지</h2>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-[10px] text-text-secondary">
            Space: 미리보기 · ↑↓: 그룹 이동
          </span>
          {scanning && dupProgress && dupProgress.phase !== "complete" && dupProgress.total > 0 && (
            <div className="flex items-center gap-2 mr-2">
              <div className="w-32 h-1.5 bg-bg-primary rounded-full overflow-hidden">
                <div
                  className="h-full bg-accent rounded-full transition-all duration-300"
                  style={{ width: `${(dupProgress.current / dupProgress.total) * 100}%` }}
                />
              </div>
              <span className="text-[10px] text-text-secondary whitespace-nowrap">
                {dupProgress.phase} {dupProgress.current.toLocaleString()}/{dupProgress.total.toLocaleString()}
              </span>
            </div>
          )}
          <button
            onClick={handleScan}
            disabled={scanning}
            className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
          >
            {scanning ? (
              <>
                <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                스캔 중...
              </>
            ) : (
              <>
                <Search size={14} />
                중복 스캔
              </>
            )}
          </button>
        </div>
      </div>

      {/* Summary cards + action bar */}
      {summary && summary.total_groups > 0 && (
        <div className="p-3 border-b border-border bg-bg-secondary/50">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-4 flex-1 text-xs">
              <span className="text-text-secondary">
                <strong className="text-text-primary">{summary.total_groups}</strong> 그룹
              </span>
              <span className="text-text-secondary">
                <strong className="text-text-primary">{summary.total_files_in_groups}</strong> 파일
              </span>
              <span className="text-danger">
                <Trash2 size={11} className="inline mr-0.5 -mt-0.5" />
                <strong>{deleteStats.count}</strong>개 삭제 예정
              </span>
              <span className="text-success">
                <TrendingDown size={11} className="inline mr-0.5 -mt-0.5" />
                <strong>{formatFileSize(deleteStats.size)}</strong> 절약
              </span>
            </div>
            <button
              onClick={handleTrashAll}
              disabled={trashing || deleteStats.count === 0}
              className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-danger text-white hover:bg-danger/90 disabled:opacity-50 transition-colors"
            >
              {trashing ? (
                <>
                  <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                  처리 중...
                </>
              ) : (
                <>
                  <Trash2 size={13} />
                  {deleteStats.count}개 휴지통으로 이동
                </>
              )}
            </button>
          </div>
          {trashResult && (
            <p className="text-xs text-success mt-2">{trashResult}</p>
          )}
        </div>
      )}

      {/* Scanning state */}
      {scanning ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center max-w-sm">
            <div className="w-10 h-10 mx-auto mb-4 border-3 border-accent border-t-transparent rounded-full animate-spin" />
            {dupProgress && dupProgress.total > 0 ? (
              <>
                <p className="text-sm font-medium text-text-primary mb-3">
                  {dupProgress.phase}
                </p>
                <div className="w-72 h-2.5 bg-bg-secondary rounded-full overflow-hidden mx-auto mb-2">
                  <div
                    className="h-full bg-accent rounded-full transition-all duration-300"
                    style={{ width: `${(dupProgress.current / dupProgress.total) * 100}%` }}
                  />
                </div>
                <p className="text-xs text-text-secondary mb-1">
                  {dupProgress.current.toLocaleString()} / {dupProgress.total.toLocaleString()}
                  <span className="ml-2 text-accent font-medium">
                    {Math.round((dupProgress.current / dupProgress.total) * 100)}%
                  </span>
                </p>
                {dupProgress.detail && (
                  <p className="text-[10px] text-text-secondary/70 mt-2">
                    {dupProgress.detail}
                  </p>
                )}
              </>
            ) : (
              <>
                <p className="text-sm font-medium text-text-primary mb-1">
                  중복 파일 탐색 준비 중...
                </p>
                <p className="text-xs text-text-secondary">
                  전체 파일 크기 분석 중
                </p>
              </>
            )}
          </div>
        </div>
      ) : groups.length === 0 ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center text-text-secondary">
            <Copy size={48} className="mx-auto mb-3 opacity-30" />
            <p className="text-sm">중복 파일이 없습니다</p>
            <p className="text-xs mt-1">"중복 스캔"을 눌러 중복 파일을 찾으세요</p>
            <p className="text-[10px] mt-1 text-text-secondary/50">100KB 이하 파일은 제외됩니다</p>
          </div>
        </div>
      ) : groups.length > 0 ? (
        <div className="flex-1 flex overflow-hidden">
          {/* Group list */}
          <div className="w-64 border-r border-border overflow-y-auto">
            {groups.map((group, idx) => {
              const toDelete = group.members.filter((m) => !m.is_preferred).length;
              return (
                <button
                  key={group.id}
                  onClick={() => {
                    setSelectedGroup(group);
                    setSelectedIdx(idx);
                  }}
                  className={`w-full p-3 text-left border-b border-border/50 transition-colors ${
                    selectedGroup?.id === group.id
                      ? "bg-accent/5 border-l-2 border-l-accent"
                      : "hover:bg-bg-secondary"
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-medium text-text-primary">
                      그룹 {idx + 1}
                    </span>
                    <span
                      className={`text-[10px] px-1.5 py-0.5 rounded-full ${
                        group.match_type === "exact"
                          ? "bg-danger/10 text-danger"
                          : "bg-warning/10 text-warning"
                      }`}
                    >
                      {group.match_type === "exact" ? "완전 일치" : "유사"}
                    </span>
                  </div>
                  <p className="text-[10px] text-text-secondary mt-1 truncate">
                    {group.members[0]?.file_name}
                  </p>
                  <p className="text-[10px] mt-1">
                    <span className="text-success">유지 1개</span>
                    <span className="text-text-secondary"> · </span>
                    <span className="text-danger">삭제 {toDelete}개</span>
                  </p>
                </button>
              );
            })}
          </div>

          {/* Detail view */}
          {selectedGroup && (() => {
            const focused =
              selectedGroup.members[
                Math.min(focusIdx, selectedGroup.members.length - 1)
              ];
            return (
            <div className="flex-1 flex flex-col overflow-hidden">
              {/* Group header */}
              <div className="px-4 py-2 border-b border-border/50 shrink-0 flex items-center gap-2">
                <AlertTriangle size={14} className="text-warning shrink-0" />
                <span className="text-xs font-medium text-text-primary">
                  {selectedGroup.match_type === "exact"
                    ? `완전 동일 파일 (${selectedGroup.members.length}개)`
                    : `유사 이미지 (${Math.round((selectedGroup.similarity_score ?? 0) * 100)}% 일치)`}
                </span>
                <span className="text-[9px] text-text-secondary/50 ml-auto">
                  ←/→ 사진 이동 · Space 확대 · S 건너뛰기
                </span>
              </div>

              {/* Large preview — pick while actually seeing the photo */}
              {focused && (
                <MemberPreview
                  key={focused.media_id}
                  filePath={focused.file_path}
                  mediaType={focused.media_type}
                  thumbnail={focused.thumbnail}
                  alt={focused.file_name}
                  fit="contain"
                  className="flex-1 min-h-0 bg-black/90"
                  onClick={() => handlePreview(focused.file_path)}
                >
                  {/* Status badge */}
                  <div
                    className={`absolute top-3 left-3 flex items-center gap-1 px-2.5 py-1 rounded-full text-[10px] font-bold backdrop-blur-sm ${
                      focused.is_preferred
                        ? "bg-success/90 text-white"
                        : "bg-danger/80 text-white"
                    }`}
                  >
                    {focused.is_preferred ? (
                      <>
                        <Shield size={10} /> 유지
                      </>
                    ) : (
                      <>
                        <Trash2 size={10} /> 삭제 예정
                      </>
                    )}
                  </div>
                  {/* Keep-this button overlay */}
                  {!focused.is_preferred && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleSetPreferred(selectedGroup.id, focused.media_id);
                      }}
                      className="absolute bottom-3 left-1/2 -translate-x-1/2 flex items-center gap-1.5 px-4 py-1.5 rounded-full text-[11px] font-bold bg-accent text-white hover:bg-accent-hover shadow-lg transition-colors"
                    >
                      <Shield size={11} />
                      이것을 유지
                    </button>
                  )}
                </MemberPreview>
              )}

              {/* Focused file info + actions */}
              {focused && (
                <div className="px-4 py-1.5 border-t border-border/50 shrink-0 flex items-center gap-3">
                  <div className="flex-1 min-w-0">
                    <p className="text-[11px] font-medium text-text-primary truncate">
                      {focused.file_name}
                    </p>
                    <div className="flex gap-3 text-[9.5px] text-text-secondary truncate">
                      <span className="truncate">{focused.file_path}</span>
                    </div>
                  </div>
                  <div className="flex gap-3 text-[10px] text-text-secondary shrink-0 tabular-nums">
                    <span>{formatFileSize(focused.file_size)}</span>
                    {focused.width && focused.height && (
                      <span>
                        {focused.width}x{focused.height}
                      </span>
                    )}
                    {focused.date_taken && (
                      <span>{formatDate(focused.date_taken)}</span>
                    )}
                  </div>
                  <div className="flex gap-1 shrink-0">
                    <button
                      onClick={() => handlePreview(focused.file_path)}
                      className="p-1.5 rounded hover:bg-bg-secondary transition-colors"
                      title="Quick Look (Space)"
                    >
                      <Eye size={13} className="text-text-secondary" />
                    </button>
                    <button
                      onClick={() => handleOpenFile(focused.file_path)}
                      className="p-1.5 rounded hover:bg-bg-secondary transition-colors"
                      title="Finder에서 보기"
                    >
                      <FolderOpen size={13} className="text-text-secondary" />
                    </button>
                  </div>
                </div>
              )}

              {/* Member thumbnail strip */}
              <div className="px-4 py-2 border-t border-border/50 shrink-0 overflow-x-auto">
                <div className="flex gap-2">
                  {selectedGroup.members.map((member, mi) => {
                    const isFocused = mi === Math.min(focusIdx, selectedGroup.members.length - 1);
                    return (
                      <button
                        key={member.media_id}
                        onClick={() => setFocusIdx(mi)}
                        className={`relative w-24 h-[72px] rounded-md overflow-hidden shrink-0 bg-black/80 transition-all ${
                          isFocused
                            ? "ring-2 ring-accent"
                            : member.is_preferred
                              ? "ring-1 ring-success/60 opacity-80 hover:opacity-100"
                              : "ring-1 ring-danger/40 opacity-60 hover:opacity-100"
                        }`}
                        title={member.file_name}
                      >
                        {thumbSrc(member.thumbnail) ? (
                          <img
                            src={thumbSrc(member.thumbnail)!}
                            alt={member.file_name}
                            className="w-full h-full object-cover"
                          />
                        ) : (
                          <div className="w-full h-full flex items-center justify-center">
                            <FileImage size={16} className="text-white/20" />
                          </div>
                        )}
                        <div
                          className={`absolute top-1 left-1 w-4 h-4 rounded-full flex items-center justify-center ${
                            member.is_preferred
                              ? "bg-success text-white"
                              : "bg-danger/80 text-white"
                          }`}
                        >
                          {member.is_preferred ? (
                            <Check size={9} />
                          ) : (
                            <Trash2 size={8} />
                          )}
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* Group actions */}
              <div className="px-4 py-2.5 border-t border-border/50 shrink-0 flex gap-2">
                <button
                  onClick={() => handleTrashGroup(selectedGroup)}
                  disabled={trashing || selectedGroup.members.filter((m) => !m.is_preferred).length === 0}
                  className="flex items-center justify-center gap-1.5 flex-1 py-2 rounded-md text-xs font-medium bg-danger/10 text-danger hover:bg-danger/20 disabled:opacity-50 transition-colors"
                >
                  <Trash2 size={12} />
                  이 그룹 삭제 예정 파일 휴지통으로
                </button>
                <button
                  onClick={() => handleDismissGroup(selectedGroup.id)}
                  className="flex items-center justify-center gap-1.5 flex-1 py-2 rounded-md text-xs font-medium bg-bg-secondary text-text-secondary hover:bg-bg-primary transition-colors"
                >
                  <SkipForward size={12} />
                  건너뛰기 (모두 유지)
                </button>
              </div>
            </div>
            );
          })()}
        </div>
      ) : null}
    </div>
  );
}

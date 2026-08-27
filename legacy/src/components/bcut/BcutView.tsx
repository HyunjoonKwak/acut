import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatFileSize, formatDate } from "@/utils/format";
import { useBcutProgress } from "@/hooks/useTauriEvents";
import { thumbSrc } from "@/utils/media";
import { MemberPreview } from "@/components/culling/MemberPreview";
import { toast } from "@/stores/toastStore";
import {
  Focus,
  Search,
  Check,
  Trash2,
  Crown,
  Eye,
  FolderOpen,
  SkipForward,
  Sparkles,
  X,
  Zap,
  Sun,
  Maximize2,
} from "lucide-react";

interface BcutMember {
  media_id: string;
  file_path: string;
  file_name: string;
  file_size: number;
  width: number | null;
  height: number | null;
  thumbnail: string | null;
  date_taken: string | null;
  quality_score: number;
  sharpness_score: number;
  exposure_score: number;
  is_best: boolean;
}

interface BcutGroup {
  id: string;
  group_reason: string;
  status: string;
  members: BcutMember[];
}

interface BcutSummary {
  total_groups: number;
  total_bcuts: number;
  groups: BcutGroup[];
}

interface TrashResult {
  success: number;
  failed: number;
  errors: string[];
}

export function BcutView() {
  const [summary, setSummary] = useState<BcutSummary | null>(null);
  const [selectedGroup, setSelectedGroup] = useState<BcutGroup | null>(null);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [scanning, setScanning] = useState(false);
  const [trashing, setTrashing] = useState(false);
  const [trashResult, setTrashResult] = useState<string | null>(null);
  const progress = useBcutProgress();
  const [timeGap, setTimeGap] = useState(5);

  const loadGroups = async () => {
    try {
      const result = await invoke<BcutSummary>("get_bcut_groups");
      setSummary(result);
      if (result.groups.length > 0) {
        setSelectedGroup(result.groups[0]);
        setSelectedIdx(0);
      } else {
        setSelectedGroup(null);
        setSelectedIdx(0);
      }
    } catch (e) {
      toast.error(`B컷 그룹을 불러오지 못했습니다: ${e}`);
    }
  };

  const handleDetect = async () => {
    setScanning(true);
    setSummary(null);
    setSelectedGroup(null);
    setTrashResult(null);
    try {
      await invoke("detect_bcuts", { timeGapSeconds: timeGap });
      await loadGroups();
    } catch (e) {
      toast.error(`B컷 탐지에 실패했습니다: ${e}`);
    }
    setScanning(false);
  };

  const handlePreview = async (path: string) => {
    await invoke("preview_file", { path });
  };

  const handleOpenFile = async (path: string) => {
    await invoke("open_file", { path });
  };

  const handleSetBest = async (groupId: string, mediaId: string) => {
    try {
      await invoke("set_bcut_best", { groupId, mediaId });
      const result = await invoke<BcutSummary>("get_bcut_groups");
      setSummary(result);
      const updated = result.groups.find((g) => g.id === groupId);
      if (updated) setSelectedGroup(updated);
    } catch {
      // Handle error
    }
  };

  const handleDismissGroup = async (groupId: string) => {
    try {
      await invoke("dismiss_bcut_group", { groupId });
      const result = await invoke<BcutSummary>("get_bcut_groups");
      setSummary(result);
      if (result.groups.length > 0) {
        const nextIdx = Math.min(selectedIdx, result.groups.length - 1);
        setSelectedGroup(result.groups[nextIdx]);
        setSelectedIdx(nextIdx);
      } else {
        setSelectedGroup(null);
        setSelectedIdx(0);
      }
    } catch (e) {
      toast.error(`그룹 건너뛰기에 실패했습니다: ${e}`);
    }
  };

  const handleTrashAll = async () => {
    if (!summary || summary.total_bcuts === 0) return;
    setTrashing(true);
    setTrashResult(null);
    try {
      const result = await invoke<TrashResult>("trash_bcut_files");
      setTrashResult(
        `${result.success}개 B컷 휴지통 이동 완료` +
          (result.failed > 0 ? ` (${result.failed}개 실패)` : ""),
      );
      await loadGroups();
    } catch (e) {
      setTrashResult("휴지통 이동 실패");
      toast.error(`휴지통 이동에 실패했습니다: ${e}`);
    }
    setTrashing(false);
  };

  const totalSavings = useMemo(() => {
    if (!summary) return 0;
    let size = 0;
    for (const g of summary.groups) {
      for (const m of g.members) {
        if (!m.is_best) size += m.file_size;
      }
    }
    return size;
  }, [summary]);

  // Keyboard
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!summary || !selectedGroup) return;
      const { groups } = summary;

      if (e.key === "ArrowDown" || e.key === "j") {
        e.preventDefault();
        const next = Math.min(selectedIdx + 1, groups.length - 1);
        setSelectedGroup(groups[next]);
        setSelectedIdx(next);
      } else if (e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault();
        const prev = Math.max(selectedIdx - 1, 0);
        setSelectedGroup(groups[prev]);
        setSelectedIdx(prev);
      } else if (e.key === "s") {
        e.preventDefault();
        handleDismissGroup(selectedGroup.id);
      } else if (e.code === "Space") {
        e.preventDefault();
        if (selectedGroup.members.length > 0) {
          handlePreview(selectedGroup.members[0].file_path);
        }
      } else if (e.key >= "1" && e.key <= "9") {
        const idx = parseInt(e.key) - 1;
        if (idx < selectedGroup.members.length) {
          handleSetBest(selectedGroup.id, selectedGroup.members[idx].media_id);
        }
      }
    },
    [summary, selectedGroup, selectedIdx],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  useEffect(() => {
    loadGroups();
  }, []);

  const groups = summary?.groups ?? [];

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-bg-primary relative">
      {/* Header */}
      <div className="px-4 py-2.5 border-b border-border flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-7 h-7 rounded-lg bg-accent/10 flex items-center justify-center">
            <Focus size={15} className="text-accent" />
          </div>
          <div>
            <h2 className="text-[13px] font-semibold text-text-primary leading-tight">
              B컷 자동 탐지
            </h2>
            <p className="text-[9px] text-text-secondary">
              연사 그룹화 + 품질 분석
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-1.5 text-[10px] text-text-secondary">
            간격
            <select
              value={timeGap}
              onChange={(e) => setTimeGap(Number(e.target.value))}
              className="bg-bg-secondary border border-border rounded-md px-2 py-1 text-[10px] focus:outline-none focus:ring-1 focus:ring-accent/50"
            >
              <option value={2}>2초</option>
              <option value={5}>5초</option>
              <option value={10}>10초</option>
              <option value={30}>30초</option>
              <option value={60}>1분</option>
            </select>
          </label>
          <button
            onClick={handleDetect}
            disabled={scanning}
            className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg text-[11px] font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 shadow-sm shadow-accent/20 transition-all"
          >
            {scanning ? (
              <>
                <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                분석 중...
              </>
            ) : (
              <>
                <Search size={12} />
                분석 시작
              </>
            )}
          </button>
        </div>
      </div>

      {/* Progress */}
      {scanning && progress && (
        <div className="px-4 py-2 border-b border-border bg-accent/5 shrink-0">
          <div className="flex items-center justify-between text-[10px] mb-1.5">
            <span className="text-accent font-medium">
              {progress.phase === "grouping"
                ? "시간 기반 그룹화 중..."
                : `품질 분석 중...`}
            </span>
            {progress.total > 0 && (
              <span className="text-text-secondary tabular-nums">
                {progress.current} / {progress.total}
              </span>
            )}
          </div>
          {progress.total > 0 && (
            <div className="w-full h-1 bg-accent/10 rounded-full overflow-hidden">
              <div
                className="h-full bg-accent rounded-full transition-all duration-300"
                style={{
                  width: `${(progress.current / progress.total) * 100}%`,
                }}
              />
            </div>
          )}
        </div>
      )}

      {/* Summary bar */}
      {summary && summary.total_groups > 0 && (
        <div className="px-4 py-2 border-b border-border flex items-center justify-between shrink-0">
          <div className="flex items-center gap-4 text-[11px]">
            <span className="text-text-secondary">
              <strong className="text-text-primary">{summary.total_groups}</strong> 그룹
            </span>
            <span className="text-danger">
              <X size={10} className="inline mr-0.5 -mt-px" />
              <strong>{summary.total_bcuts}</strong> B컷
            </span>
            <span className="text-success">
              <Sparkles size={10} className="inline mr-0.5 -mt-px" />
              {formatFileSize(totalSavings)} 절약 가능
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handleTrashAll}
              disabled={trashing || summary.total_bcuts === 0}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[10px] font-medium bg-danger text-white hover:bg-danger/90 disabled:opacity-50 shadow-sm transition-all"
            >
              {trashing ? (
                <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
              ) : (
                <Trash2 size={11} />
              )}
              전체 B컷 정리
            </button>
          </div>
        </div>
      )}

      {/* Empty state */}
      {!scanning && (!summary || summary.total_groups === 0) && (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center max-w-xs">
            <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-accent/10 flex items-center justify-center">
              <Focus size={28} className="text-accent" />
            </div>
            <p className="text-sm font-semibold text-text-primary mb-1">
              B컷 자동 탐지
            </p>
            <p className="text-[11px] text-text-secondary leading-relaxed mb-5">
              연속 촬영 사진을 시간 기준으로 그룹화하고
              <br />
              선명도·노출을 분석하여 베스트 컷을 추천합니다.
            </p>
            {summary && summary.total_groups === 0 && (
              <div className="inline-flex items-center gap-1.5 text-[11px] text-success bg-success/10 px-3 py-1.5 rounded-full">
                <Check size={12} />
                B컷이 발견되지 않았습니다
              </div>
            )}
          </div>
        </div>
      )}

      {/* Main content */}
      {summary && groups.length > 0 && (
        <div className="flex-1 flex overflow-hidden">
          {/* Group list */}
          <div className="w-60 border-r border-border overflow-y-auto bg-bg-secondary/30">
            {groups.map((group, idx) => {
              const best = group.members.find((m) => m.is_best);
              const bcutCount = group.members.filter(
                (m) => !m.is_best,
              ).length;
              const isActive = selectedGroup?.id === group.id;
              return (
                <button
                  key={group.id}
                  onClick={() => {
                    setSelectedGroup(group);
                    setSelectedIdx(idx);
                  }}
                  className={`w-full flex gap-2.5 p-2.5 text-left border-b border-border/30 transition-all ${
                    isActive
                      ? "bg-accent/8 border-l-2 border-l-accent"
                      : "hover:bg-bg-secondary/80 border-l-2 border-l-transparent"
                  }`}
                >
                  {/* Thumbnail preview */}
                  <div className="w-11 h-11 rounded-md overflow-hidden bg-bg-secondary shrink-0">
                    {thumbSrc(best?.thumbnail) ? (
                      <img
                        src={thumbSrc(best?.thumbnail)!}
                        alt=""
                        className="w-full h-full object-cover"
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center">
                        <Focus size={14} className="text-text-secondary/20" />
                      </div>
                    )}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center justify-between">
                      <span
                        className={`text-[11px] font-medium ${isActive ? "text-accent" : "text-text-primary"}`}
                      >
                        그룹 {idx + 1}
                      </span>
                      <span className="text-[9px] text-text-secondary">
                        {group.members.length}장
                      </span>
                    </div>
                    <p className="text-[9px] text-text-secondary truncate mt-0.5">
                      {best?.file_name ?? group.members[0]?.file_name}
                    </p>
                    <div className="flex items-center gap-1.5 mt-1">
                      <span className="inline-flex items-center gap-0.5 text-[8px] text-success bg-success/10 px-1.5 py-px rounded-full">
                        <Crown size={7} />
                        {Math.round(best?.quality_score ?? 0)}
                      </span>
                      <span className="text-[8px] text-danger/70">
                        -{bcutCount}
                      </span>
                    </div>
                  </div>
                </button>
              );
            })}
          </div>

          {/* Detail view */}
          {selectedGroup && (
            <div className="flex-1 flex flex-col overflow-hidden">
              {/* Group header */}
              <div className="px-4 py-2 flex items-center justify-between border-b border-border/50 shrink-0">
                <div className="flex items-center gap-2 text-[11px]">
                  <span className="text-text-primary font-medium">
                    그룹 {selectedIdx + 1}
                  </span>
                  <span className="text-text-secondary">
                    {selectedGroup.members.length}장 비교
                  </span>
                  <span className="text-[9px] text-text-secondary/50">
                    1-9 선택 · S 건너뛰기 · Space 미리보기
                  </span>
                </div>
                <button
                  onClick={() => handleDismissGroup(selectedGroup.id)}
                  className="flex items-center gap-1 px-2.5 py-1 rounded-md text-[10px] font-medium text-text-secondary hover:bg-bg-secondary transition-colors"
                >
                  <SkipForward size={11} />
                  건너뛰기
                </button>
              </div>

              {/* Card grid */}
              <div className="flex-1 p-4 overflow-y-auto">
                <div className="grid grid-cols-2 xl:grid-cols-3 gap-3">
                  {selectedGroup.members.map((member, mi) => {
                    const isBest = member.is_best;
                    return (
                      <div
                        key={member.media_id}
                        className={`group rounded-xl overflow-hidden transition-all duration-200 ${
                          isBest
                            ? "ring-2 ring-success shadow-lg shadow-success/10"
                            : "ring-1 ring-border hover:ring-accent/30 hover:shadow-md"
                        }`}
                      >
                        {/* Image area — high-res preview, click to Quick Look */}
                        <div className="relative aspect-[4/3] bg-[#0a0a0a] overflow-hidden">
                          <MemberPreview
                            filePath={member.file_path}
                            thumbnail={member.thumbnail}
                            alt={member.file_name}
                            fit="cover"
                            dimmed={!isBest}
                            className="absolute inset-0"
                            onClick={() => handlePreview(member.file_path)}
                          />

                          {/* Number badge */}
                          <div
                            className={`absolute top-2 right-2 w-6 h-6 rounded-full flex items-center justify-center text-[10px] font-bold backdrop-blur-md ${
                              isBest
                                ? "bg-success text-white"
                                : "bg-black/40 text-white/70"
                            }`}
                          >
                            {mi + 1}
                          </div>

                          {/* Best crown */}
                          {isBest && (
                            <div className="absolute top-2 left-2 flex items-center gap-1 bg-success/90 text-white px-2 py-1 rounded-md text-[9px] font-bold backdrop-blur-sm">
                              <Crown size={10} />
                              BEST
                            </div>
                          )}

                          {/* B-cut label */}
                          {!isBest && (
                            <div className="absolute top-2 left-2 flex items-center gap-1 bg-black/50 text-white/60 px-2 py-1 rounded-md text-[9px] font-medium backdrop-blur-sm">
                              <X size={9} />
                              B컷
                            </div>
                          )}

                          {/* Score overlay */}
                          <div className="absolute bottom-0 inset-x-0 bg-gradient-to-t from-black/70 to-transparent pt-6 pb-2 px-2.5">
                            <div className="flex items-end justify-between">
                              <span
                                className={`text-xl font-bold tabular-nums ${
                                  isBest ? "text-success" : "text-white/50"
                                }`}
                              >
                                {Math.round(member.quality_score)}
                              </span>
                              {/* Action buttons */}
                              <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    handlePreview(member.file_path);
                                  }}
                                  className="p-1.5 rounded-md bg-white/10 hover:bg-white/20 transition-colors"
                                  title="미리보기"
                                >
                                  <Eye size={12} className="text-white" />
                                </button>
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    handleOpenFile(member.file_path);
                                  }}
                                  className="p-1.5 rounded-md bg-white/10 hover:bg-white/20 transition-colors"
                                  title="Finder"
                                >
                                  <FolderOpen
                                    size={12}
                                    className="text-white"
                                  />
                                </button>
                              </div>
                            </div>
                          </div>
                        </div>

                        {/* Info panel */}
                        <div className="p-2.5 bg-bg-primary">
                          <p className="text-[10px] text-text-primary font-medium truncate">
                            {member.file_name}
                          </p>

                          {/* Score bars */}
                          <div className="mt-2 space-y-1.5">
                            <ScoreBar
                              icon={<Zap size={8} />}
                              label="선명도"
                              value={member.sharpness_score}
                              color="blue"
                            />
                            <ScoreBar
                              icon={<Sun size={8} />}
                              label="노출"
                              value={member.exposure_score}
                              color="amber"
                            />
                            <ScoreBar
                              icon={<Maximize2 size={8} />}
                              label="종합"
                              value={member.quality_score}
                              color={isBest ? "green" : "gray"}
                            />
                          </div>

                          {/* Meta */}
                          <div className="flex gap-2 mt-2 text-[8px] text-text-secondary/60">
                            <span>{formatFileSize(member.file_size)}</span>
                            {member.width && member.height && (
                              <span>
                                {member.width}x{member.height}
                              </span>
                            )}
                            {member.date_taken && (
                              <span>{formatDate(member.date_taken)}</span>
                            )}
                          </div>

                          {/* Action */}
                          {isBest ? (
                            <div className="mt-2 flex items-center justify-center gap-1 py-1.5 rounded-lg text-[10px] font-bold text-success bg-success/8">
                              <Crown size={10} />
                              베스트 컷
                            </div>
                          ) : (
                            <button
                              onClick={() =>
                                handleSetBest(
                                  selectedGroup.id,
                                  member.media_id,
                                )
                              }
                              className="mt-2 w-full py-1.5 rounded-lg text-[10px] font-medium bg-bg-secondary text-text-secondary hover:bg-accent hover:text-white transition-all"
                            >
                              베스트로 선택
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>
      )}
      {/* Trashing overlay */}
      {trashing && (
        <div className="absolute inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center">
          <div className="text-center animate-slide-in">
            <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-danger/20 flex items-center justify-center">
              <Trash2 size={28} className="text-danger animate-pulse" />
            </div>
            <p className="text-sm font-semibold text-white mb-1">
              B컷 정리 중...
            </p>
            <p className="text-[11px] text-white/60">
              {summary?.total_bcuts ?? 0}개 파일을 휴지통으로 이동하고 있습니다
            </p>
            <div className="mt-4 w-40 mx-auto h-1 bg-white/10 rounded-full overflow-hidden">
              <div className="h-full bg-danger rounded-full animate-[shimmer_1.5s_infinite]" style={{ width: "60%" }} />
            </div>
          </div>
        </div>
      )}

      {/* Trash complete overlay */}
      {trashResult && !trashing && (
        <div
          className="absolute inset-0 z-50 bg-black/50 backdrop-blur-sm flex items-center justify-center cursor-pointer"
          onClick={() => setTrashResult(null)}
        >
          <div className="text-center animate-mark-pulse">
            <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-success/20 flex items-center justify-center">
              <Check size={32} className="text-success" />
            </div>
            <p className="text-sm font-semibold text-white mb-1">
              정리 완료
            </p>
            <p className="text-[11px] text-white/60">{trashResult}</p>
            <p className="text-[9px] text-white/30 mt-3">클릭하여 닫기</p>
          </div>
        </div>
      )}
    </div>
  );
}

function ScoreBar({
  icon,
  label,
  value,
  color,
}: {
  icon: React.ReactNode;
  label: string;
  value: number;
  color: "blue" | "amber" | "green" | "gray";
}) {
  const barColor = {
    blue: "bg-blue-500",
    amber: "bg-amber-500",
    green: "bg-success",
    gray: "bg-text-secondary/30",
  }[color];

  return (
    <div className="flex items-center gap-1.5">
      <span className="text-text-secondary/40 shrink-0">{icon}</span>
      <span className="text-[8px] text-text-secondary w-6 shrink-0">
        {label}
      </span>
      <div className="flex-1 h-1 bg-bg-secondary rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-all duration-500 ${barColor}`}
          style={{ width: `${Math.min(value, 100)}%` }}
        />
      </div>
      <span className="text-[8px] text-text-secondary/60 w-4 text-right tabular-nums shrink-0">
        {Math.round(value)}
      </span>
    </div>
  );
}

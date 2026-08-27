import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { formatFileSize, formatDate } from "@/utils/format";
import { thumbSrc } from "@/utils/media";
import { toast } from "@/stores/toastStore";
import type { MediaFile, MediaListResponse } from "@/types";
import {
  ScanSearch,
  FolderOpen,
  Trash2,
  Check,
  ChevronLeft,
  ChevronRight,
  Eye,
  X,
  Keyboard,
  AlertTriangle,
  Zap,
  Crown,
  Play,
} from "lucide-react";

type MarkState = "keep" | "discard" | "unmarked";

interface ReviewFile extends MediaFile {
  mark: MarkState;
  bcutGroupId?: string;
  isBest?: boolean;
}

interface QualityScore {
  sharpness: number;
  exposure: number;
  total: number;
}

interface BcutGroup {
  id: string;
  members: { media_id: string; is_best: boolean; quality_score: number }[];
}

export function ReviewView() {
  const { t } = useTranslation();
  const [folderPath, setFolderPath] = useState("");
  const [files, setFiles] = useState<ReviewFile[]>([]);
  const [currentIdx, setCurrentIdx] = useState(0);
  const [loading, setLoading] = useState(false);
  const [trashing, setTrashing] = useState(false);
  const [trashResult, setTrashResult] = useState<string | null>(null);
  const [previewCache, setPreviewCache] = useState<Map<string, string>>(
    new Map(),
  );
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [markAnim, setMarkAnim] = useState<MarkState | null>(null);
  const [qualityScores, setQualityScores] = useState<Map<string, QualityScore>>(
    new Map(),
  );
  const [autoAnalysis, setAutoAnalysis] = useState(false);
  const [analyzing, setAnalyzing] = useState(false);
  const stripRef = useRef<HTMLDivElement>(null);

  const current = files[currentIdx] ?? null;

  const stats = useMemo(() => {
    let discard = 0;
    let keep = 0;
    let discardSize = 0;
    for (const f of files) {
      if (f.mark === "discard") {
        discard++;
        discardSize += f.file_size;
      }
      if (f.mark === "keep") keep++;
    }
    return { discard, keep, discardSize, total: files.length };
  }, [files]);

  const progress = useMemo(() => {
    if (files.length === 0) return 0;
    const reviewed = files.filter((f) => f.mark !== "unmarked").length;
    return (reviewed / files.length) * 100;
  }, [files]);

  const handleSelectFolder = async () => {
    const result = await open({ directory: true, multiple: false });
    if (result) {
      setFolderPath(result as string);
      setTrashResult(null);
    }
  };

  const loadFiles = useCallback(async () => {
    if (!folderPath) return;
    setLoading(true);
    try {
      // Photos AND videos — video previews show the first frame,
      // Quick Look (Space on the eye button) plays them
      const result = await invoke<MediaListResponse>("get_media_list", {
        folderPath,
        mediaType: null,
        offset: 0,
        limit: 5000,
      });
      setFiles(
        result.files.map((f) => ({ ...f, mark: "unmarked" as const })),
      );
      setCurrentIdx(0);
      setPreviewCache(new Map());

      // Compute quality scores (images only — not meaningful for videos)
      const mediaIds = result.files
        .filter((f) => f.media_type === "image")
        .map((f) => f.id);
      if (mediaIds.length > 0) {
        try {
          const scores = await invoke<Record<string, { sharpness: number; exposure: number; total: number }>>(
            "compute_quality_scores",
            { mediaIds },
          );
          const scoreMap = new Map<string, QualityScore>();
          for (const [id, score] of Object.entries(scores)) {
            scoreMap.set(id, score);
          }
          setQualityScores(scoreMap);
        } catch {
          // Quality scores not available
        }
      }
    } catch {
      // Handle error
    }
    setLoading(false);
  }, [folderPath]);

  useEffect(() => {
    if (folderPath) loadFiles();
  }, [folderPath, loadFiles]);

  // Auto analysis: detect bcuts and auto-mark low quality
  const runAutoAnalysis = useCallback(async () => {
    if (files.length === 0) return;
    setAnalyzing(true);
    try {
      // Run B-cut detection
      await invoke("detect_bcuts", { timeGapSeconds: 5 });
      const bcutResult = await invoke<{
        groups: BcutGroup[];
      }>("get_bcut_groups");

      // Build a map: media_id -> { groupId, isBest }
      const bcutMap = new Map<string, { groupId: string; isBest: boolean; quality: number }>();
      for (const group of bcutResult.groups) {
        for (const member of group.members) {
          bcutMap.set(member.media_id, {
            groupId: group.id,
            isBest: member.is_best,
            quality: member.quality_score,
          });
        }
      }

      // Auto-mark: B-cut (not best) → discard, best → keep
      setFiles((prev) =>
        prev.map((f) => {
          const info = bcutMap.get(f.id);
          if (info) {
            return {
              ...f,
              bcutGroupId: info.groupId,
              isBest: info.isBest,
              mark: info.isBest ? "keep" : "discard",
            };
          }
          // Files with quality < 40 also auto-mark as discard
          const q = qualityScores.get(f.id);
          if (q && q.total < 40) {
            return { ...f, mark: "discard" };
          }
          return f;
        }),
      );
    } catch {
      // Detection failed
    }
    setAnalyzing(false);
  }, [files.length, qualityScores]);

  // When autoAnalysis toggle is turned on, run analysis
  useEffect(() => {
    if (autoAnalysis && files.length > 0 && !analyzing) {
      runAutoAnalysis();
    }
  }, [autoAnalysis]);

  // Preload previews
  const loadPreview = useCallback(
    async (idx: number) => {
      if (idx < 0 || idx >= files.length) return;
      const file = files[idx];
      if (previewCache.has(file.id)) return;
      try {
        const cmd =
          file.media_type === "video"
            ? "get_preview_video_frame"
            : "get_preview_image";
        const preview = await invoke<string>(cmd, {
          filePath: file.file_path,
        });
        setPreviewCache((prev) => {
          const next = new Map(prev);
          next.set(file.id, preview);
          return next;
        });
      } catch {
        // Handle error
      }
    },
    [files, previewCache],
  );

  useEffect(() => {
    loadPreview(currentIdx);
    loadPreview(currentIdx + 1);
    loadPreview(currentIdx + 2);
    loadPreview(currentIdx - 1);
  }, [currentIdx, loadPreview]);

  // Scroll strip
  useEffect(() => {
    if (stripRef.current) {
      const el = stripRef.current.children[currentIdx] as HTMLElement;
      if (el) {
        el.scrollIntoView({
          behavior: "smooth",
          block: "nearest",
          inline: "center",
        });
      }
    }
  }, [currentIdx]);

  // Mark animation
  const flashMark = useCallback((mark: MarkState) => {
    setMarkAnim(mark);
    setTimeout(() => setMarkAnim(null), 400);
  }, []);

  const toggleMark = useCallback(
    (mark: MarkState) => {
      setFiles((prev) => {
        const next = [...prev];
        const c = next[currentIdx];
        const newMark = c.mark === mark ? "unmarked" : mark;
        next[currentIdx] = { ...c, mark: newMark };
        if (newMark !== "unmarked") flashMark(newMark);
        return next;
      });
    },
    [currentIdx, flashMark],
  );

  const goNext = useCallback(() => {
    setCurrentIdx((i) => Math.min(i + 1, files.length - 1));
  }, [files.length]);

  const goPrev = useCallback(() => {
    setCurrentIdx((i) => Math.max(i - 1, 0));
  }, []);

  const markDiscardAndNext = useCallback(() => {
    setFiles((prev) => {
      const next = [...prev];
      const c = next[currentIdx];
      const newMark = c.mark === "discard" ? "unmarked" : "discard";
      next[currentIdx] = { ...c, mark: newMark };
      if (newMark === "discard") flashMark("discard");
      return next;
    });
    if (currentIdx < files.length - 1) {
      setTimeout(() => goNext(), 150);
    }
  }, [currentIdx, files.length, goNext, flashMark]);

  const markKeepAndNext = useCallback(() => {
    setFiles((prev) => {
      const next = [...prev];
      const c = next[currentIdx];
      const newMark = c.mark === "keep" ? "unmarked" : "keep";
      next[currentIdx] = { ...c, mark: newMark };
      if (newMark === "keep") flashMark("keep");
      return next;
    });
    if (currentIdx < files.length - 1) {
      setTimeout(() => goNext(), 150);
    }
  }, [currentIdx, files.length, goNext, flashMark]);

  // Keyboard
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (files.length === 0) return;
      switch (e.key) {
        case "ArrowRight":
        case "l":
          e.preventDefault();
          goNext();
          break;
        case "ArrowLeft":
        case "h":
          e.preventDefault();
          goPrev();
          break;
        case " ":
          e.preventDefault();
          markDiscardAndNext();
          break;
        case "k":
          e.preventDefault();
          markKeepAndNext();
          break;
        case "Escape":
          e.preventDefault();
          toggleMark("unmarked");
          break;
        case "?":
          e.preventDefault();
          setShowShortcuts((s) => !s);
          break;
      }
    },
    [files.length, goNext, goPrev, markDiscardAndNext, markKeepAndNext, toggleMark],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  const handlePreview = async () => {
    if (current) await invoke("preview_file", { path: current.file_path });
  };

  const handleTrash = async () => {
    const toTrash = files.filter((f) => f.mark === "discard");
    if (toTrash.length === 0) return;
    setTrashing(true);
    setTrashResult(null);
    const paths = toTrash.map((f) => f.file_path);
    try {
      const result = await invoke<{
        success: number;
        failed: number;
        errors: string[];
      }>("trash_review_files", { filePaths: paths });
      setTrashResult(
        `${result.success}개 파일 휴지통 이동 완료` +
          (result.failed > 0 ? ` (${result.failed}개 실패)` : ""),
      );
      await loadFiles();
    } catch (e) {
      setTrashResult("휴지통 이동 실패");
      toast.error(`휴지통 이동에 실패했습니다: ${e}`);
    }
    setTrashing(false);
  };

  const preview = current ? previewCache.get(current.id) : undefined;
  const currentQuality = current ? qualityScores.get(current.id) : undefined;

  // --- Empty state ---
  if (!folderPath) {
    return (
      <div className="flex-1 flex items-center justify-center bg-bg-primary">
        <div className="text-center max-w-xs">
          <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-accent/10 flex items-center justify-center">
            <ScanSearch size={28} className="text-accent" />
          </div>
          <p className="text-sm font-semibold text-text-primary mb-1">
            사진 리뷰어
          </p>
          <p className="text-[11px] text-text-secondary leading-relaxed mb-5">
            사진을 빠르게 넘기면서 B컷을 마킹하세요.
            <br />
            Space로 B컷, K로 유지, 화살표로 이동합니다.
          </p>
          <button
            onClick={handleSelectFolder}
            className="inline-flex items-center gap-2 px-5 py-2.5 rounded-lg text-xs font-medium bg-accent text-white hover:bg-accent-hover shadow-sm shadow-accent/20 transition-all hover:shadow-md hover:shadow-accent/30"
          >
            <FolderOpen size={15} />
            폴더 선택하여 시작
          </button>
        </div>
      </div>
    );
  }

  // --- Main view ---
  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-bg-primary">
      {/* Progress bar (thin, top edge) */}
      <div className="h-0.5 bg-bg-secondary shrink-0">
        <div
          className="h-full bg-accent transition-all duration-300"
          style={{ width: `${progress}%` }}
        />
      </div>

      {/* Compact top bar */}
      <div className="px-3 py-1.5 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2">
          <button
            onClick={handleSelectFolder}
            className="p-1 rounded hover:bg-bg-secondary transition-colors"
            title="폴더 변경"
          >
            <FolderOpen size={14} className="text-text-secondary" />
          </button>
          <span className="text-[10px] text-text-secondary/60 truncate max-w-[200px]">
            {folderPath.split("/").pop()}
          </span>
          <div className="w-px h-3 bg-border" />
          <span className="text-[11px] text-text-primary tabular-nums font-medium">
            {currentIdx + 1}
            <span className="text-text-secondary/50 font-normal">
              {" "}
              / {stats.total}
            </span>
          </span>
        </div>

        <div className="flex items-center gap-3">
          {/* Stats pills */}
          <div className="flex items-center gap-1.5">
            {stats.keep > 0 && (
              <span className="flex items-center gap-1 text-[10px] text-success bg-success/10 px-2 py-0.5 rounded-full font-medium">
                <Check size={9} />
                {stats.keep}
              </span>
            )}
            {stats.discard > 0 && (
              <span className="flex items-center gap-1 text-[10px] text-danger bg-danger/10 px-2 py-0.5 rounded-full font-medium">
                <X size={9} />
                {stats.discard}
                <span className="text-danger/50">
                  ({formatFileSize(stats.discardSize)})
                </span>
              </span>
            )}
          </div>

          {/* Trash button */}
          {stats.discard > 0 && (
            <button
              onClick={handleTrash}
              disabled={trashing}
              className="flex items-center gap-1.5 px-3 py-1 rounded-md text-[10px] font-medium bg-danger text-white hover:bg-danger/90 disabled:opacity-50 shadow-sm transition-all"
            >
              {trashing ? (
                <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
              ) : (
                <Trash2 size={11} />
              )}
              정리하기
            </button>
          )}

          {/* Auto analysis toggle */}
          <button
            onClick={() => setAutoAnalysis((s) => !s)}
            disabled={analyzing || files.length === 0}
            className={`flex items-center gap-1 px-2 py-1 rounded-md text-[10px] font-medium transition-all ${
              autoAnalysis
                ? "bg-accent text-white shadow-sm shadow-accent/20"
                : "text-text-secondary hover:bg-bg-secondary"
            }`}
            title="B컷 자동 분석: 연속 촬영 사진 그룹핑 + 품질 기반 자동 마킹"
          >
            {analyzing ? (
              <div className="w-3 h-3 border-2 border-current border-t-transparent rounded-full animate-spin" />
            ) : (
              <Zap size={11} />
            )}
            자동 분석
          </button>

          {/* Shortcuts help */}
          <button
            onClick={() => setShowShortcuts((s) => !s)}
            className="p-1 rounded hover:bg-bg-secondary transition-colors"
            title="단축키 (?)"
          >
            <Keyboard size={13} className="text-text-secondary/50" />
          </button>
        </div>
      </div>

      {trashResult && (
        <div className="px-3 py-1 text-[10px] text-success bg-success/5 shrink-0">
          <Check size={10} className="inline mr-1" />
          {trashResult}
        </div>
      )}

      {loading ? (
        <div className="flex-1 flex items-center justify-center">
          <div className="w-5 h-5 border-2 border-accent border-t-transparent rounded-full animate-spin" />
        </div>
      ) : files.length === 0 ? (
        <div className="flex-1 flex items-center justify-center">
          <p className="text-sm text-text-secondary">이미지가 없습니다</p>
        </div>
      ) : (
        <>
          {/* Main preview — dark immersive background */}
          <div className={`flex-1 relative overflow-hidden bg-[#0a0a0a] flex items-center justify-center ${
            currentQuality && currentQuality.total < 40
              ? "ring-2 ring-inset ring-danger/60 shadow-inner shadow-danger/20"
              : currentQuality && currentQuality.total > 70
                ? "ring-1 ring-inset ring-success/30"
                : ""
          }`}>
            {/* Nav arrows */}
            <button
              onClick={goPrev}
              disabled={currentIdx === 0}
              className="absolute left-4 z-10 w-10 h-10 rounded-full bg-white/10 hover:bg-white/20 disabled:opacity-10 text-white flex items-center justify-center backdrop-blur-sm transition-all"
            >
              <ChevronLeft size={20} />
            </button>
            <button
              onClick={goNext}
              disabled={currentIdx === files.length - 1}
              className="absolute right-4 z-10 w-10 h-10 rounded-full bg-white/10 hover:bg-white/20 disabled:opacity-10 text-white flex items-center justify-center backdrop-blur-sm transition-all"
            >
              <ChevronRight size={20} />
            </button>

            {/* Image */}
            {preview ? (
              <img
                key={current?.id}
                src={`data:image/jpeg;base64,${preview}`}
                alt={current?.file_name}
                className={`max-h-full max-w-full object-contain transition-all duration-200 ${
                  current?.mark === "discard"
                    ? "opacity-30 scale-[0.98]"
                    : "opacity-100"
                }`}
              />
            ) : thumbSrc(current?.thumbnail) ? (
              <img
                src={thumbSrc(current?.thumbnail)!}
                alt={current?.file_name}
                className="max-h-full max-w-full object-contain blur-md opacity-50"
              />
            ) : (
              <div className="w-8 h-8 border-2 border-white/20 border-t-white/60 rounded-full animate-spin" />
            )}

            {/* Mark overlay animation */}
            {markAnim === "discard" && (
              <div className="absolute inset-0 flex items-center justify-center pointer-events-none z-20">
                <div className="animate-mark-pulse bg-danger/90 text-white w-20 h-20 rounded-full flex items-center justify-center shadow-2xl shadow-danger/40">
                  <X size={36} strokeWidth={3} />
                </div>
              </div>
            )}
            {markAnim === "keep" && (
              <div className="absolute inset-0 flex items-center justify-center pointer-events-none z-20">
                <div className="animate-mark-pulse bg-success/90 text-white w-20 h-20 rounded-full flex items-center justify-center shadow-2xl shadow-success/40">
                  <Check size={36} strokeWidth={3} />
                </div>
              </div>
            )}

            {/* Persistent mark badge (top-left) */}
            {current?.mark !== "unmarked" && !markAnim && (
              <div
                className={`absolute top-4 left-4 flex items-center gap-1.5 px-3 py-1.5 rounded-full text-[11px] font-bold text-white backdrop-blur-sm ${
                  current?.mark === "discard"
                    ? "bg-danger/70"
                    : "bg-success/70"
                }`}
              >
                {current?.mark === "discard" ? (
                  <>
                    <X size={12} /> B컷
                  </>
                ) : current?.isBest ? (
                  <>
                    <Crown size={12} /> BEST
                  </>
                ) : (
                  <>
                    <Check size={12} /> 유지
                  </>
                )}
              </div>
            )}

            {/* Quick Look */}
            <button
              onClick={handlePreview}
              className="absolute top-4 right-4 p-2 rounded-full bg-white/10 hover:bg-white/20 text-white/70 hover:text-white backdrop-blur-sm transition-all"
              title="Quick Look"
            >
              <Eye size={15} />
            </button>

            {/* Video indicator — Quick Look plays it */}
            {current?.media_type === "video" && (
              <div className="absolute top-4 left-1/2 -translate-x-1/2 flex items-center gap-1.5 px-3 py-1.5 rounded-full text-[10px] font-bold text-white/90 bg-white/15 backdrop-blur-sm">
                <Play size={11} />
                {t("review.videoHint", "영상 — Quick Look으로 재생")}
              </div>
            )}

            {/* Low quality warning icon */}
            {currentQuality && currentQuality.total < 40 && (
              <div className="absolute top-4 left-1/2 -translate-x-1/2 flex items-center gap-1.5 px-3 py-1.5 rounded-full text-[10px] font-bold text-danger bg-danger/20 backdrop-blur-sm">
                <AlertTriangle size={12} />
                {t("review.lowQuality", "낮은 품질")}
              </div>
            )}

            {/* File info overlay (bottom) */}
            <div className="absolute bottom-0 inset-x-0 bg-gradient-to-t from-black/60 via-black/20 to-transparent px-5 pb-3 pt-10">
              <p className="text-[12px] text-white/90 font-medium truncate">
                {current?.file_name}
              </p>
              <div className="flex gap-3 mt-0.5 text-[10px] text-white/50">
                <span>{current ? formatFileSize(current.file_size) : ""}</span>
                {current?.width && current?.height && (
                  <span>
                    {current.width}x{current.height}
                  </span>
                )}
                {current?.date_taken && (
                  <span>{formatDate(current.date_taken)}</span>
                )}
              </div>
              {/* Quality score bar */}
              {currentQuality && (
                <div className={`flex items-center gap-2 mt-1.5 text-[10px] tabular-nums ${
                  currentQuality.total < 40
                    ? "text-danger/80"
                    : currentQuality.total > 70
                      ? "text-success/80"
                      : "text-white/50"
                }`}>
                  <span>{t("review.quality", "품질")} {Math.round(currentQuality.total)}%</span>
                  <span className="text-white/20">|</span>
                  <span>{t("review.sharpness", "선명도")} {Math.round(currentQuality.sharpness)}%</span>
                  <span className="text-white/20">|</span>
                  <span>{t("review.exposure", "노출")} {Math.round(currentQuality.exposure)}%</span>
                </div>
              )}
            </div>
          </div>

          {/* Action buttons bar */}
          <div className="flex items-center justify-center gap-3 py-2 bg-bg-primary shrink-0">
            <button
              onClick={() => markKeepAndNext()}
              className={`flex items-center gap-1.5 px-5 py-2 rounded-lg text-xs font-semibold transition-all ${
                current?.mark === "keep"
                  ? "bg-success text-white shadow-sm shadow-success/30"
                  : "bg-success/10 text-success hover:bg-success/20"
              }`}
            >
              <Check size={14} />
              유지
              <kbd className="ml-1 text-[9px] opacity-50 bg-success/20 px-1 rounded">
                K
              </kbd>
            </button>
            <button
              onClick={() => markDiscardAndNext()}
              className={`flex items-center gap-1.5 px-5 py-2 rounded-lg text-xs font-semibold transition-all ${
                current?.mark === "discard"
                  ? "bg-danger text-white shadow-sm shadow-danger/30"
                  : "bg-danger/10 text-danger hover:bg-danger/20"
              }`}
            >
              <X size={14} />
              B컷
              <kbd className="ml-1 text-[9px] opacity-50 bg-danger/20 px-1 rounded">
                Space
              </kbd>
            </button>
          </div>

          {/* Thumbnail strip */}
          <div
            ref={stripRef}
            className="flex gap-px px-1 py-1 bg-[#0a0a0a] overflow-x-auto scrollbar-thin shrink-0"
          >
            {files.map((file, idx) => {
              const q = qualityScores.get(file.id);
              return (
                <button
                  key={file.id}
                  onClick={() => setCurrentIdx(idx)}
                  className={`relative shrink-0 w-11 h-11 rounded-sm overflow-hidden transition-all duration-150 ${
                    idx === currentIdx
                      ? "ring-2 ring-accent ring-offset-1 ring-offset-[#0a0a0a] scale-110 z-10"
                      : file.mark === "discard"
                        ? "opacity-25"
                        : file.mark === "keep"
                          ? "ring-1 ring-success/40"
                          : "opacity-60 hover:opacity-90"
                  }`}
                >
                  {thumbSrc(file.thumbnail) ? (
                    <img
                      src={thumbSrc(file.thumbnail)!}
                      alt=""
                      className="w-full h-full object-cover"
                    />
                  ) : (
                    <div className="w-full h-full bg-white/5" />
                  )}
                  {/* Tiny mark dot */}
                  {file.mark === "discard" && (
                    <div className="absolute bottom-0.5 right-0.5 w-2 h-2 rounded-full bg-danger" />
                  )}
                  {file.mark === "keep" && (
                    <div className="absolute bottom-0.5 right-0.5 w-2 h-2 rounded-full bg-success" />
                  )}
                  {/* BEST crown for auto-analyzed */}
                  {file.isBest && (
                    <div className="absolute top-0 right-0 bg-success rounded-bl-sm p-px">
                      <Crown size={7} className="text-white" />
                    </div>
                  )}
                  {/* Quality dot */}
                  {q && (
                    <div className={`absolute top-0.5 left-0.5 w-1.5 h-1.5 rounded-full ${
                      q.total < 40
                        ? "bg-danger"
                        : q.total <= 70
                          ? "bg-yellow-500"
                          : "bg-success"
                    }`} />
                  )}
                </button>
              );
            })}
          </div>
        </>
      )}

      {/* Shortcuts modal */}
      {showShortcuts && (
        <div
          className="absolute inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50"
          onClick={() => setShowShortcuts(false)}
        >
          <div
            className="bg-bg-elevated rounded-xl border border-border p-5 w-72 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <p className="text-sm font-semibold text-text-primary mb-3">
              키보드 단축키
            </p>
            <div className="space-y-2 text-[11px]">
              {[
                ["Space", "B컷 마킹 + 다음"],
                ["K", "유지 마킹 + 다음"],
                ["← →", "이전 / 다음 사진"],
                ["H / L", "이전 / 다음 사진"],
                ["Esc", "마킹 해제"],
                ["?", "단축키 도움말"],
              ].map(([key, desc]) => (
                <div key={key} className="flex items-center justify-between">
                  <kbd className="bg-bg-secondary px-2 py-0.5 rounded text-text-primary font-mono text-[10px]">
                    {key}
                  </kbd>
                  <span className="text-text-secondary">{desc}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

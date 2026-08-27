import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatFileSize } from "@/utils/format";
import { useDuplicateProgress } from "@/hooks/useTauriEvents";
import {
  Copy,
  Focus,
  ArrowRightLeft,
  RefreshCw,
  Check,
  ChevronRight,
  Play,
  SkipForward,
  Loader2,
} from "lucide-react";

type StepState = "pending" | "running" | "done" | "skipped";

interface StepInfo {
  id: string;
  label: string;
  icon: typeof Copy;
}

interface StepResult {
  filesProcessed: number;
  freedSpace: number;
  detail: string;
}

const STEPS: StepInfo[] = [
  { id: "overview", label: "통합 정리 마법사", icon: Play },
  { id: "duplicates", label: "중복 파일 정리", icon: Copy },
  { id: "bcuts", label: "B컷 정리", icon: Focus },
  { id: "organize", label: "폴더 정리", icon: ArrowRightLeft },
  { id: "summary", label: "완료", icon: Check },
];

export function CleanupWizardView() {
  const duplicateProgress = useDuplicateProgress();

  const [currentStep, setCurrentStep] = useState(0);
  const [stepStates, setStepStates] = useState<StepState[]>([
    "pending",
    "pending",
    "pending",
    "pending",
    "pending",
  ]);
  const [stepResults, setStepResults] = useState<(StepResult | null)[]>([
    null,
    null,
    null,
    null,
    null,
  ]);

  // Overview stats
  const [mediaStats, setMediaStats] = useState<{
    total_count: number;
    total_size: number;
    image_count: number;
    video_count: number;
  } | null>(null);

  // Duplicates
  const [dupGroups, setDupGroups] = useState<number>(0);
  const [dupFiles, setDupFiles] = useState<number>(0);
  const [dupSize, setDupSize] = useState<number>(0);

  // Bcuts
  const [bcutGroups, setBcutGroups] = useState<number>(0);
  const [bcutFiles, setBcutFiles] = useState<number>(0);
  const [bcutSize, setBcutSize] = useState<number>(0);

  // Organize
  const [organizeFiles, setOrganizeFiles] = useState<number>(0);
  const [organizeStrategy, setOrganizeStrategy] = useState<"date" | "type">("date");

  const updateStepState = useCallback(
    (step: number, state: StepState) => {
      setStepStates((prev) => {
        const next = [...prev];
        next[step] = state;
        return next;
      });
    },
    [],
  );

  const updateStepResult = useCallback(
    (step: number, result: StepResult) => {
      setStepResults((prev) => {
        const next = [...prev];
        next[step] = result;
        return next;
      });
    },
    [],
  );

  const advanceStep = useCallback(() => {
    setCurrentStep((prev) => Math.min(prev + 1, STEPS.length - 1));
  }, []);

  const skipStep = useCallback(() => {
    updateStepState(currentStep, "skipped");
    advanceStep();
  }, [currentStep, updateStepState, advanceStep]);

  // --- Step 0: Overview ---
  const handleStartOverview = async () => {
    updateStepState(0, "running");
    try {
      const stats = await invoke<{
        total_count: number;
        total_size: number;
        image_count: number;
        video_count: number;
      }>("get_media_stats");
      setMediaStats(stats);
    } catch {
      // Handle error
    }
    updateStepState(0, "done");
    advanceStep();
  };

  // --- Step 1: Duplicates ---
  const handleRunDuplicates = async () => {
    updateStepState(1, "running");
    try {
      await invoke("detect_duplicates");
      const groups = await invoke<any[]>("get_duplicate_groups");
      let totalFiles = 0;
      let totalSize = 0;
      for (const g of groups) {
        const nonPreferred = g.members.filter((m: any) => !m.is_preferred);
        totalFiles += nonPreferred.length;
        for (const m of nonPreferred) {
          totalSize += m.file_size;
        }
      }
      setDupGroups(groups.length);
      setDupFiles(totalFiles);
      setDupSize(totalSize);
    } catch {
      // Handle error
    }
  };

  const handleTrashDuplicates = async () => {
    try {
      const result = await invoke<{ success: number; failed: number }>(
        "trash_duplicate_files",
      );
      updateStepResult(1, {
        filesProcessed: result.success,
        freedSpace: dupSize,
        detail: `${result.success}개 중복 파일 정리 완료`,
      });
    } catch {
      // Handle error
    }
    updateStepState(1, "done");
    advanceStep();
  };

  // --- Step 2: B-cuts ---
  const handleRunBcuts = async () => {
    updateStepState(2, "running");
    try {
      await invoke("detect_bcuts", { timeGapSeconds: 5 });
      const summary = await invoke<{
        total_groups: number;
        total_bcuts: number;
        groups: any[];
      }>("get_bcut_groups");
      setBcutGroups(summary.total_groups);
      setBcutFiles(summary.total_bcuts);
      let totalSize = 0;
      for (const g of summary.groups) {
        for (const m of g.members) {
          if (!m.is_best) totalSize += m.file_size;
        }
      }
      setBcutSize(totalSize);
    } catch {
      // Handle error
    }
  };

  const handleTrashBcuts = async () => {
    try {
      const result = await invoke<{ success: number; failed: number }>(
        "trash_bcut_files",
      );
      updateStepResult(2, {
        filesProcessed: result.success,
        freedSpace: bcutSize,
        detail: `${result.success}개 B컷 정리 완료`,
      });
    } catch {
      // Handle error
    }
    updateStepState(2, "done");
    advanceStep();
  };

  // --- Step 3: Organize ---
  const handleRunOrganize = async () => {
    updateStepState(3, "running");
    try {
      const plan = await invoke<{
        moves: any[];
        total_files: number;
        total_size: number;
        new_folders: number;
      }>("preview_organize", { strategy: organizeStrategy });
      setOrganizeFiles(plan.total_files);
    } catch {
      // Handle error
    }
  };

  const handleExecuteOrganize = async () => {
    try {
      const result = await invoke<{ moved: number; failed: number }>(
        "execute_organize",
      );
      updateStepResult(3, {
        filesProcessed: result.moved,
        freedSpace: 0,
        detail: `${result.moved}개 파일 정리 완료`,
      });
    } catch {
      // Handle error
    }
    updateStepState(3, "done");
    advanceStep();
  };

  // --- Computed totals for summary ---
  const totalFreedSpace = stepResults.reduce(
    (sum, r) => sum + (r?.freedSpace ?? 0),
    0,
  );
  const totalFilesProcessed = stepResults.reduce(
    (sum, r) => sum + (r?.filesProcessed ?? 0),
    0,
  );

  const renderStepIndicator = () => (
    <div className="flex items-center justify-center gap-0 py-4 px-6 shrink-0">
      {STEPS.map((step, idx) => {
        const state = stepStates[idx];
        const isCurrent = idx === currentStep;
        return (
          <div key={step.id} className="flex items-center">
            {idx > 0 && (
              <div
                className={`w-10 h-0.5 ${
                  state === "done"
                    ? "bg-success"
                    : state === "skipped"
                      ? "bg-text-secondary/20"
                      : "bg-border"
                }`}
              />
            )}
            <div
              className={`w-8 h-8 rounded-full flex items-center justify-center text-[11px] font-bold transition-all ${
                state === "done"
                  ? "bg-success text-white"
                  : state === "skipped"
                    ? "bg-text-secondary/10 text-text-secondary/40"
                    : state === "running"
                      ? "bg-accent text-white animate-pulse"
                      : isCurrent
                        ? "bg-accent text-white shadow-sm shadow-accent/30"
                        : "bg-bg-secondary text-text-secondary border border-border"
              }`}
              title={step.label}
            >
              {state === "done" ? (
                <Check size={14} />
              ) : state === "running" ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                idx + 1
              )}
            </div>
          </div>
        );
      })}
    </div>
  );

  const renderBottomBar = (
    onSkip?: () => void,
    onAction?: () => void,
    actionLabel?: string,
    actionDisabled?: boolean,
  ) => (
    <div className="px-6 py-3 border-t border-border flex items-center justify-between shrink-0">
      <div>
        {onSkip && stepStates[currentStep] !== "done" && (
          <button
            onClick={onSkip}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] font-medium text-text-secondary hover:bg-bg-secondary transition-colors"
          >
            <SkipForward size={12} />
            건너뛰기
          </button>
        )}
      </div>
      <div>
        {onAction && (
          <button
            onClick={onAction}
            disabled={actionDisabled}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg text-[11px] font-semibold bg-accent text-white hover:bg-accent-hover disabled:opacity-50 shadow-sm shadow-accent/20 transition-all"
          >
            {stepStates[currentStep] === "running" ? (
              <Loader2 size={13} className="animate-spin" />
            ) : (
              <ChevronRight size={13} />
            )}
            {actionLabel ?? "다음 단계"}
          </button>
        )}
      </div>
    </div>
  );

  const renderStepContent = () => {
    switch (currentStep) {
      // --- Overview ---
      case 0:
        return (
          <div className="flex-1 flex items-center justify-center p-6">
            <div className="text-center max-w-md">
              <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-accent/10 flex items-center justify-center">
                <RefreshCw size={28} className="text-accent" />
              </div>
              <h2 className="text-lg font-bold text-text-primary mb-2">
                통합 정리 마법사
              </h2>
              <p className="text-[12px] text-text-secondary leading-relaxed mb-6">
                중복 파일, B컷, 폴더 정리를 단계별로 진행합니다.
                <br />
                각 단계에서 미리보기 후 실행할 수 있으며, 건너뛸 수도 있습니다.
              </p>
              {mediaStats && (
                <div className="inline-flex items-center gap-4 text-[11px] text-text-secondary bg-bg-secondary rounded-lg px-4 py-2 mb-6">
                  <span>
                    파일{" "}
                    <strong className="text-text-primary">
                      {mediaStats.total_count}
                    </strong>
                    개
                  </span>
                  <span>
                    사진{" "}
                    <strong className="text-text-primary">
                      {mediaStats.image_count}
                    </strong>
                    개
                  </span>
                  <span>
                    영상{" "}
                    <strong className="text-text-primary">
                      {mediaStats.video_count}
                    </strong>
                    개
                  </span>
                  <span>
                    용량{" "}
                    <strong className="text-text-primary">
                      {formatFileSize(mediaStats.total_size)}
                    </strong>
                  </span>
                </div>
              )}
            </div>
          </div>
        );

      // --- Duplicates ---
      case 1:
        return (
          <div className="flex-1 flex flex-col items-center justify-center p-6 gap-4">
            <div className="w-12 h-12 rounded-xl bg-blue-500/10 flex items-center justify-center">
              <Copy size={22} className="text-blue-500" />
            </div>
            <h3 className="text-sm font-bold text-text-primary">
              중복 파일 정리
            </h3>

            {stepStates[1] === "pending" && (
              <p className="text-[11px] text-text-secondary text-center">
                라이브러리에서 중복 파일을 탐지합니다.
              </p>
            )}

            {stepStates[1] === "running" && dupGroups === 0 && (
              <div className="flex items-center gap-2 text-[11px] text-accent">
                <Loader2 size={14} className="animate-spin" />
                중복 탐지 중...
                {duplicateProgress && duplicateProgress.total > 0 && (
                  <span className="tabular-nums text-text-secondary">
                    {duplicateProgress.current} / {duplicateProgress.total}
                  </span>
                )}
              </div>
            )}

            {(dupGroups > 0 || stepStates[1] === "done") && (
              <div className="bg-bg-secondary rounded-lg px-5 py-3 text-center space-y-1">
                <div className="text-[11px] text-text-secondary">
                  <strong className="text-text-primary">{dupGroups}</strong> 그룹
                  {" / "}
                  <strong className="text-danger">{dupFiles}</strong> 중복 파일
                </div>
                <div className="text-[12px] font-semibold text-accent">
                  {formatFileSize(dupSize)} 절약 가능
                </div>
              </div>
            )}

            {stepResults[1] && (
              <div className="text-[11px] text-success bg-success/10 px-3 py-1.5 rounded-full flex items-center gap-1.5">
                <Check size={11} />
                {stepResults[1].detail}
              </div>
            )}
          </div>
        );

      // --- B-cuts ---
      case 2:
        return (
          <div className="flex-1 flex flex-col items-center justify-center p-6 gap-4">
            <div className="w-12 h-12 rounded-xl bg-amber-500/10 flex items-center justify-center">
              <Focus size={22} className="text-amber-500" />
            </div>
            <h3 className="text-sm font-bold text-text-primary">B컷 정리</h3>

            {stepStates[2] === "pending" && (
              <p className="text-[11px] text-text-secondary text-center">
                연사 그룹에서 품질이 낮은 B컷을 탐지합니다.
              </p>
            )}

            {stepStates[2] === "running" && bcutGroups === 0 && (
              <div className="flex items-center gap-2 text-[11px] text-accent">
                <Loader2 size={14} className="animate-spin" />
                B컷 탐지 중...
              </div>
            )}

            {(bcutGroups > 0 || stepStates[2] === "done") && (
              <div className="bg-bg-secondary rounded-lg px-5 py-3 text-center space-y-1">
                <div className="text-[11px] text-text-secondary">
                  <strong className="text-text-primary">{bcutGroups}</strong>{" "}
                  그룹 {" / "}
                  <strong className="text-danger">{bcutFiles}</strong> B컷
                </div>
                <div className="text-[12px] font-semibold text-accent">
                  {formatFileSize(bcutSize)} 절약 가능
                </div>
              </div>
            )}

            {stepResults[2] && (
              <div className="text-[11px] text-success bg-success/10 px-3 py-1.5 rounded-full flex items-center gap-1.5">
                <Check size={11} />
                {stepResults[2].detail}
              </div>
            )}
          </div>
        );

      // --- Organize ---
      case 3:
        return (
          <div className="flex-1 flex flex-col items-center justify-center p-6 gap-4">
            <div className="w-12 h-12 rounded-xl bg-violet-500/10 flex items-center justify-center">
              <ArrowRightLeft size={22} className="text-violet-500" />
            </div>
            <h3 className="text-sm font-bold text-text-primary">폴더 정리</h3>

            {stepStates[3] === "pending" && (
              <>
                <p className="text-[11px] text-text-secondary text-center">
                  파일을 날짜 또는 유형별로 정리합니다.
                </p>
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => setOrganizeStrategy("date")}
                    className={`px-3 py-1.5 rounded-md text-[11px] font-medium transition-all ${
                      organizeStrategy === "date"
                        ? "bg-accent text-white"
                        : "bg-bg-secondary text-text-secondary hover:text-text-primary"
                    }`}
                  >
                    날짜별
                  </button>
                  <button
                    onClick={() => setOrganizeStrategy("type")}
                    className={`px-3 py-1.5 rounded-md text-[11px] font-medium transition-all ${
                      organizeStrategy === "type"
                        ? "bg-accent text-white"
                        : "bg-bg-secondary text-text-secondary hover:text-text-primary"
                    }`}
                  >
                    유형별
                  </button>
                </div>
              </>
            )}

            {stepStates[3] === "running" && organizeFiles === 0 && (
              <div className="flex items-center gap-2 text-[11px] text-accent">
                <Loader2 size={14} className="animate-spin" />
                정리 미리보기 중...
              </div>
            )}

            {organizeFiles > 0 && stepStates[3] !== "done" && (
              <div className="bg-bg-secondary rounded-lg px-5 py-3 text-center space-y-1">
                <div className="text-[11px] text-text-secondary">
                  <strong className="text-text-primary">{organizeFiles}</strong>{" "}
                  개 파일 이동 예정
                </div>
              </div>
            )}

            {stepResults[3] && (
              <div className="text-[11px] text-success bg-success/10 px-3 py-1.5 rounded-full flex items-center gap-1.5">
                <Check size={11} />
                {stepResults[3].detail}
              </div>
            )}
          </div>
        );

      // --- Summary ---
      case 4:
        return (
          <div className="flex-1 flex items-center justify-center p-6">
            <div className="text-center max-w-sm">
              <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-success/10 flex items-center justify-center">
                <Check size={32} className="text-success" />
              </div>
              <h2 className="text-lg font-bold text-text-primary mb-2">
                완료
              </h2>
              <p className="text-[12px] text-text-secondary mb-6">
                통합 정리가 완료되었습니다.
              </p>

              <div className="bg-bg-secondary rounded-xl p-4 space-y-3">
                <div className="flex items-center justify-between text-[11px]">
                  <span className="text-text-secondary">처리된 파일</span>
                  <strong className="text-text-primary">
                    {totalFilesProcessed}개
                  </strong>
                </div>
                <div className="flex items-center justify-between text-[11px]">
                  <span className="text-text-secondary">절약된 용량</span>
                  <strong className="text-success">
                    {formatFileSize(totalFreedSpace)}
                  </strong>
                </div>

                <div className="border-t border-border pt-3 space-y-1.5">
                  {STEPS.slice(1, 4).map((step, idx) => {
                    const realIdx = idx + 1;
                    const state = stepStates[realIdx];
                    const result = stepResults[realIdx];
                    return (
                      <div
                        key={step.id}
                        className="flex items-center justify-between text-[10px]"
                      >
                        <span className="text-text-secondary">{step.label}</span>
                        <span
                          className={
                            state === "done"
                              ? "text-success"
                              : state === "skipped"
                                ? "text-text-secondary/40"
                                : "text-text-secondary"
                          }
                        >
                          {state === "done"
                            ? result?.detail ?? "완료"
                            : state === "skipped"
                              ? "건너뜀"
                              : "-"}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  const getBottomBar = () => {
    switch (currentStep) {
      case 0:
        return renderBottomBar(undefined, handleStartOverview, "정리 시작");
      case 1:
        if (stepStates[1] === "done") return null;
        if (dupGroups > 0) {
          return renderBottomBar(skipStep, handleTrashDuplicates, "실행");
        }
        return renderBottomBar(
          skipStep,
          handleRunDuplicates,
          stepStates[1] === "running" ? "탐지 중..." : "다음 단계",
          stepStates[1] === "running",
        );
      case 2:
        if (stepStates[2] === "done") return null;
        if (bcutGroups > 0) {
          return renderBottomBar(skipStep, handleTrashBcuts, "실행");
        }
        return renderBottomBar(
          skipStep,
          handleRunBcuts,
          stepStates[2] === "running" ? "탐지 중..." : "다음 단계",
          stepStates[2] === "running",
        );
      case 3:
        if (stepStates[3] === "done") return null;
        if (organizeFiles > 0) {
          return renderBottomBar(skipStep, handleExecuteOrganize, "실행");
        }
        return renderBottomBar(
          skipStep,
          handleRunOrganize,
          stepStates[3] === "running" ? "분석 중..." : "다음 단계",
          stepStates[3] === "running",
        );
      case 4:
        return null;
      default:
        return null;
    }
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-bg-primary">
      {renderStepIndicator()}
      {renderStepContent()}
      {getBottomBar()}
    </div>
  );
}

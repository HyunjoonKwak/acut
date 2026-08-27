import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useOrganizeProgress } from "@/hooks/useTauriEvents";
import { formatFileSize } from "@/utils/format";
import { toast } from "@/stores/toastStore";
import {
  ArrowRightLeft,
  FolderOpen,
  Calendar,
  FileType,
  Play,
  CheckCircle,
  ArrowRight,
  FolderPlus,
} from "lucide-react";
import type { OrganizePlan } from "@/types";

type Strategy = "date" | "type";

export function OrganizeView() {
  const [targetDir, setTargetDir] = useState<string>("");
  const [strategy, setStrategy] = useState<Strategy>("date");
  const [plan, setPlan] = useState<OrganizePlan | null>(null);
  const [loading, setLoading] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<any>(null);

  const progress = useOrganizeProgress();

  const handleSelectTarget = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      setTargetDir(selected as string);
    }
  };

  const handlePreview = async () => {
    if (!targetDir) return;
    setLoading(true);
    try {
      const result = await invoke<OrganizePlan>("preview_organize", {
        targetDir,
        strategy,
      });
      setPlan(result);
    } catch (e) {
      toast.error(`정리 미리보기에 실패했습니다: ${e}`);
    }
    setLoading(false);
  };

  const handleExecute = async () => {
    if (!plan) return;
    setExecuting(true);
    try {
      const res = await invoke("execute_organize", {
        plan,
        batchName: `Organize by ${strategy}`,
      });
      setResult(res);
      setPlan(null);
    } catch (e) {
      toast.error(`정리 실행에 실패했습니다: ${e}`);
    }
    setExecuting(false);
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="p-4 border-b border-border">
        <div className="flex items-center gap-3 mb-4">
          <ArrowRightLeft size={20} className="text-accent" />
          <h2 className="text-sm font-semibold text-text-primary">
            스마트 정리
          </h2>
        </div>

        <div className="grid grid-cols-3 gap-4">
          {/* Target folder */}
          <div>
            <label className="text-xs text-text-secondary block mb-1">
              대상 폴더
            </label>
            <button
              onClick={handleSelectTarget}
              className="w-full flex items-center gap-2 px-3 py-2 rounded-md text-xs border border-border bg-bg-primary hover:bg-bg-secondary transition-colors text-left"
            >
              <FolderOpen size={14} className="text-text-secondary shrink-0" />
              <span className="truncate text-text-primary">
                {targetDir || "대상 폴더 선택..."}
              </span>
            </button>
          </div>

          {/* Strategy */}
          <div>
            <label className="text-xs text-text-secondary block mb-1">
              분류 방식
            </label>
            <div className="flex gap-2">
              <button
                onClick={() => setStrategy("date")}
                className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-xs font-medium transition-colors ${
                  strategy === "date"
                    ? "bg-accent text-white"
                    : "bg-bg-primary border border-border text-text-secondary hover:text-text-primary"
                }`}
              >
                <Calendar size={14} />
                날짜별
              </button>
              <button
                onClick={() => setStrategy("type")}
                className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 rounded-md text-xs font-medium transition-colors ${
                  strategy === "type"
                    ? "bg-accent text-white"
                    : "bg-bg-primary border border-border text-text-secondary hover:text-text-primary"
                }`}
              >
                <FileType size={14} />
                유형별
              </button>
            </div>
          </div>

          {/* Actions */}
          <div className="flex items-end gap-2">
            <button
              onClick={handlePreview}
              disabled={!targetDir || loading}
              className="flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-md text-xs font-medium bg-bg-primary border border-border text-text-primary hover:bg-bg-secondary disabled:opacity-50 transition-colors"
            >
              {loading ? "로딩 중..." : "미리보기"}
            </button>
            <button
              onClick={handleExecute}
              disabled={!plan || executing}
              className="flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
            >
              <Play size={14} />
              {executing ? "실행 중..." : "실행"}
            </button>
          </div>
        </div>
      </div>

      {/* Progress */}
      {executing && progress && (
        <div className="p-4 border-b border-border bg-accent/5">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-medium text-text-primary">
              파일 이동 중...
            </span>
            <span className="text-xs text-text-secondary">
              {progress.current}/{progress.total}
            </span>
          </div>
          <div className="h-2 bg-bg-secondary rounded-full overflow-hidden">
            <div
              className="h-full bg-accent rounded-full transition-all"
              style={{
                width: `${progress.total > 0 ? (progress.current / progress.total) * 100 : 0}%`,
              }}
            />
          </div>
          <p className="text-[10px] text-text-secondary mt-1 truncate">
            {progress.current_file}
          </p>
        </div>
      )}

      {/* Result */}
      {result && (
        <div className="p-4 border-b border-border bg-success/5">
          <div className="flex items-center gap-2">
            <CheckCircle size={16} className="text-success" />
            <span className="text-sm font-medium text-text-primary">
              완료!
            </span>
          </div>
          <p className="text-xs text-text-secondary mt-1">
            이동 {result.moved}개 파일
            {result.failed > 0 && ` · ${result.failed} 실패`}
          </p>
        </div>
      )}

      {/* Before-After Preview */}
      {plan ? (
        <div className="flex-1 overflow-y-auto p-4">
          {/* Stats */}
          <div className="grid grid-cols-3 gap-3 mb-4">
            <div className="rounded-lg border border-border p-3 text-center">
              <p className="text-2xl font-bold text-accent">
                {plan.total_files}
              </p>
              <p className="text-xs text-text-secondary">이동할 파일</p>
            </div>
            <div className="rounded-lg border border-border p-3 text-center">
              <p className="text-2xl font-bold text-accent">
                {plan.new_folders}
              </p>
              <p className="text-xs text-text-secondary">새 폴더</p>
            </div>
            <div className="rounded-lg border border-border p-3 text-center">
              <p className="text-2xl font-bold text-accent">
                {formatFileSize(plan.total_size)}
              </p>
              <p className="text-xs text-text-secondary">전체 크기</p>
            </div>
          </div>

          {/* Move list */}
          <h3 className="text-xs font-semibold text-text-primary mb-2">
            변경 예정 항목 ({plan.moves.length})
          </h3>
          <div className="space-y-1 max-h-[60vh] overflow-y-auto">
            {plan.moves.slice(0, 200).map((move, idx) => (
              <div
                key={idx}
                className="flex items-center gap-2 px-3 py-2 rounded-md bg-bg-secondary text-xs"
              >
                <span className="text-danger truncate flex-1">
                  {move.source.split("/").pop()}
                </span>
                <ArrowRight size={12} className="text-text-secondary shrink-0" />
                <span className="text-success truncate flex-1">
                  {move.target.split("/").slice(-2).join("/")}
                </span>
                <span className="text-text-secondary shrink-0">
                  {formatFileSize(move.file_size)}
                </span>
              </div>
            ))}
            {plan.moves.length > 200 && (
              <p className="text-xs text-text-secondary text-center py-2">
                외 {plan.moves.length - 200}건 더
              </p>
            )}
          </div>
        </div>
      ) : (
        <div className="flex-1 flex items-center justify-center">
          <div className="text-center text-text-secondary">
            <FolderPlus size={48} className="mx-auto mb-3 opacity-30" />
            <p className="text-sm">대상 폴더와 분류 방식을 선택하세요</p>
            <p className="text-xs mt-1">
              "미리보기"를 눌러 변경 사항을 확인하세요
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

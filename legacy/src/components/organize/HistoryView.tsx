import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatDateTime } from "@/utils/format";
import { toast } from "@/stores/toastStore";
import { History, Undo2, CheckCircle, ChevronDown, ChevronRight } from "lucide-react";
import type { UndoBatch } from "@/types";

export function HistoryView() {
  const [batches, setBatches] = useState<UndoBatch[]>([]);
  const [expandedBatch, setExpandedBatch] = useState<string | null>(null);
  const [undoing, setUndoing] = useState<string | null>(null);

  const loadHistory = async () => {
    try {
      const result = await invoke<UndoBatch[]>("get_undo_history", {
        limit: 50,
      });
      setBatches(result);
    } catch {
      // Handle error
    }
  };

  useEffect(() => {
    loadHistory();
  }, []);

  const handleUndo = async (batchId: string) => {
    setUndoing(batchId);
    try {
      await invoke("undo_batch", { batchId });
      await loadHistory();
      toast.success("되돌리기가 완료되었습니다");
    } catch (e) {
      toast.error(`되돌리기에 실패했습니다: ${e}`);
    }
    setUndoing(null);
  };

  if (batches.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center text-text-secondary">
          <History size={48} className="mx-auto mb-3 opacity-30" />
          <p className="text-sm">아직 히스토리가 없습니다</p>
          <p className="text-xs mt-1">파일을 정리하면 여기에 기록이 표시됩니다</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <div className="p-4 border-b border-border flex items-center gap-3">
        <History size={20} className="text-accent" />
        <div>
          <h2 className="text-sm font-semibold text-text-primary">
            작업 히스토리
          </h2>
          <p className="text-xs text-text-secondary">
            {batches.length}개 작업 · 되돌리기 가능
          </p>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-2">
        {batches.map((batch) => (
          <div
            key={batch.batch_id}
            className="rounded-lg border border-border overflow-hidden"
          >
            <div
              className="flex items-center gap-3 p-3 cursor-pointer hover:bg-bg-secondary transition-colors"
              onClick={() =>
                setExpandedBatch(
                  expandedBatch === batch.batch_id ? null : batch.batch_id
                )
              }
            >
              {expandedBatch === batch.batch_id ? (
                <ChevronDown size={14} className="text-text-secondary" />
              ) : (
                <ChevronRight size={14} className="text-text-secondary" />
              )}
              <CheckCircle size={14} className="text-success" />
              <div className="flex-1">
                <p className="text-xs font-medium text-text-primary">
                  {batch.batch_name || "이름 없는 작업"}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {batch.entries.length}개 동작
                  {batch.entries[0]?.executed_at &&
                    ` · ${formatDateTime(batch.entries[0].executed_at)}`}
                </p>
              </div>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handleUndo(batch.batch_id);
                }}
                disabled={undoing === batch.batch_id}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium bg-warning/10 text-warning hover:bg-warning/20 disabled:opacity-50 transition-colors"
              >
                <Undo2 size={12} />
                {undoing === batch.batch_id ? "되돌리는 중..." : "되돌리기"}
              </button>
            </div>

            {expandedBatch === batch.batch_id && (
              <div className="border-t border-border bg-bg-secondary/50 max-h-60 overflow-y-auto">
                {batch.entries.map((entry) => (
                  <div
                    key={entry.id}
                    className="flex items-center gap-2 px-4 py-1.5 text-[10px] border-b border-border/30 last:border-0"
                  >
                    <span className="text-text-secondary w-6">
                      #{entry.sequence + 1}
                    </span>
                    <span className="text-accent font-medium w-10">
                      {entry.operation}
                    </span>
                    <span className="text-danger truncate flex-1">
                      {entry.source_path?.split("/").pop()}
                    </span>
                    <span className="text-text-secondary">→</span>
                    <span className="text-success truncate flex-1">
                      {entry.target_path?.split("/").slice(-2).join("/")}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

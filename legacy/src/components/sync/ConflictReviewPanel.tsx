import { useTranslation } from "react-i18next";
import { formatFileSize, formatDateTime } from "@/utils/format";
import { AlertTriangle, Check } from "lucide-react";
import type { SyncConflict } from "@/types";

interface ConflictReviewPanelProps {
  conflicts: SyncConflict[];
  onResolve: (index: number, resolution: string) => void;
  onResolveAll: (resolution: string) => void;
}

const RESOLUTION_OPTIONS = ["force_copy", "rename_copy", "skip"] as const;

export function ConflictReviewPanel({
  conflicts,
  onResolve,
  onResolveAll,
}: ConflictReviewPanelProps) {
  const { t } = useTranslation();

  return (
    <div className="rounded-lg border border-border overflow-hidden">
      {/* Apply All buttons */}
      <div className="flex items-center gap-2 px-3 py-2 bg-bg-secondary border-b border-border">
        <span className="text-[10px] text-text-secondary mr-auto">
          {t("sync.conflict.applyAll")}
        </span>
        {RESOLUTION_OPTIONS.map((res) => (
          <button
            key={res}
            onClick={() => onResolveAll(res)}
            className="px-2 py-1 rounded text-[10px] font-medium border border-border bg-bg-primary text-text-secondary hover:text-text-primary hover:border-accent/50 transition-colors"
          >
            {t(`sync.conflict.${res}`)}
          </button>
        ))}
      </div>

      {/* Table header */}
      <div className="grid grid-cols-[1fr_1fr_auto_auto_auto_auto] gap-2 px-3 py-1.5 text-[10px] font-semibold text-text-secondary border-b border-border bg-bg-secondary">
        <span>{t("sync.conflict.sourcePath")}</span>
        <span>{t("sync.conflict.targetPath")}</span>
        <span className="text-right">{t("sync.conflict.sourceSize")}</span>
        <span className="text-right">{t("sync.conflict.targetSize")}</span>
        <span className="text-center">{t("sync.conflict.identical")}</span>
        <span className="text-center">{t("sync.conflict.resolution")}</span>
      </div>

      {/* Conflict rows */}
      <div className="max-h-64 overflow-y-auto divide-y divide-border">
        {conflicts.map((conflict, idx) => (
          <div
            key={idx}
            className="grid grid-cols-[1fr_1fr_auto_auto_auto_auto] gap-2 px-3 py-2 items-center hover:bg-bg-secondary/50 transition-colors"
          >
            {/* Source path */}
            <div className="min-w-0">
              <p className="text-[10px] text-text-primary truncate">
                {conflict.source_path.split("/").pop()}
              </p>
              <p className="text-[9px] text-text-secondary truncate">
                {formatDateTime(conflict.source_modified)}
              </p>
            </div>

            {/* Target path */}
            <div className="min-w-0">
              <p className="text-[10px] text-text-primary truncate">
                {conflict.target_path.split("/").pop()}
              </p>
              <p className="text-[9px] text-text-secondary truncate">
                {formatDateTime(conflict.target_modified)}
              </p>
            </div>

            {/* Source size */}
            <span className="text-[10px] text-text-secondary text-right whitespace-nowrap">
              {formatFileSize(conflict.source_size)}
            </span>

            {/* Target size */}
            <span className="text-[10px] text-text-secondary text-right whitespace-nowrap">
              {formatFileSize(conflict.target_size)}
            </span>

            {/* Identical badge */}
            <div className="flex justify-center">
              {conflict.content_identical ? (
                <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full text-[9px] font-medium bg-success/10 text-success">
                  <Check size={10} />
                  {t("sync.conflict.same")}
                </span>
              ) : (
                <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full text-[9px] font-medium bg-warning/10 text-warning">
                  <AlertTriangle size={10} />
                  {t("sync.conflict.different")}
                </span>
              )}
            </div>

            {/* Resolution dropdown */}
            <select
              value={conflict.resolution}
              onChange={(e) => onResolve(idx, e.target.value)}
              className="px-2 py-1 rounded text-[10px] border border-border bg-bg-primary text-text-primary focus:outline-none focus:ring-1 focus:ring-accent"
            >
              {RESOLUTION_OPTIONS.map((res) => (
                <option key={res} value={res}>
                  {t(`sync.conflict.${res}`)}
                </option>
              ))}
            </select>
          </div>
        ))}
      </div>
    </div>
  );
}

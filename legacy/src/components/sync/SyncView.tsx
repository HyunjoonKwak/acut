import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useSyncProgress } from "@/hooks/useTauriEvents";
import { formatFileSize } from "@/utils/format";
import { ConflictReviewPanel } from "./ConflictReviewPanel";
import {
  RefreshCw,
  FolderInput,
  FolderOutput,
  Play,
  Square,
  Eye,
  Save,
  Trash2,
  AlertTriangle,
  Check,
  X,
} from "lucide-react";
import type {
  SyncTask,
  SyncPlan,
  SyncResult,
  SyncPreset,
} from "@/types";

type Phase = "configure" | "previewed" | "syncing" | "done";

export function SyncView() {
  const { t } = useTranslation();
  const progress = useSyncProgress();

  // Task configuration
  const [sourceDir, setSourceDir] = useState("");
  const [targetDir, setTargetDir] = useState("");
  const [exclusionPatterns, setExclusionPatterns] = useState("");
  const [verifyChecksum, setVerifyChecksum] = useState(false);
  const [detectOrphans, setDetectOrphans] = useState(false);
  const [autoEject, setAutoEject] = useState(false);

  // Workflow state
  const [phase, setPhase] = useState<Phase>("configure");
  const [plan, setPlan] = useState<SyncPlan | null>(null);
  const [result, setResult] = useState<SyncResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Presets
  const [presets, setPresets] = useState<SyncPreset[]>([]);
  const [presetName, setPresetName] = useState("");
  const [presetsLoaded, setPresetsLoaded] = useState(false);

  // Load presets on first render
  if (!presetsLoaded) {
    setPresetsLoaded(true);
    invoke<SyncPreset[]>("get_sync_presets")
      .then(setPresets)
      .catch(() => {});
  }

  const buildTask = useCallback((): SyncTask => {
    return {
      id: crypto.randomUUID(),
      name: "",
      source_dir: sourceDir,
      target_dir: targetDir,
      exclusion_patterns: exclusionPatterns
        .split("\n")
        .map((p) => p.trim())
        .filter(Boolean),
      verify_checksum: verifyChecksum,
      detect_orphans: detectOrphans,
    };
  }, [sourceDir, targetDir, exclusionPatterns, verifyChecksum, detectOrphans]);

  const handleSelectSource = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      setSourceDir(selected as string);
    }
  };

  const handleSelectTarget = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      setTargetDir(selected as string);
    }
  };

  const handlePreview = async () => {
    if (!sourceDir || !targetDir) return;
    setLoading(true);
    setError(null);
    try {
      const task = buildTask();
      const syncPlan = await invoke<SyncPlan>("preview_sync", { task });
      setPlan(syncPlan);
      setPhase("previewed");
    } catch (e) {
      setError(String(e));
    }
    setLoading(false);
  };

  const handleStartSync = async () => {
    if (!plan) return;
    setPhase("syncing");
    setError(null);
    try {
      const syncResult = await invoke<SyncResult>("execute_sync", { plan });
      setResult(syncResult);
      setPhase("done");
    } catch (e) {
      setError(String(e));
      setPhase("previewed");
    }
  };

  const handleCancel = async () => {
    try {
      await invoke("cancel_sync");
    } catch {
      // ignore
    }
  };

  const handleReset = () => {
    setPhase("configure");
    setPlan(null);
    setResult(null);
    setError(null);
  };

  const handleConflictResolve = (index: number, resolution: string) => {
    if (!plan) return;
    const updated = { ...plan };
    updated.conflicts = updated.conflicts.map((c, i) =>
      i === index ? { ...c, resolution } : c
    );
    setPlan(updated);
  };

  const handleConflictResolveAll = (resolution: string) => {
    if (!plan) return;
    const updated = { ...plan };
    updated.conflicts = updated.conflicts.map((c) => ({ ...c, resolution }));
    setPlan(updated);
  };

  // Preset management
  const handleSavePreset = async () => {
    if (!presetName.trim() || !sourceDir || !targetDir) return;
    const preset: SyncPreset = {
      id: crypto.randomUUID(),
      name: presetName.trim(),
      source_dir: sourceDir,
      target_dir: targetDir,
      exclusion_patterns: exclusionPatterns
        .split("\n")
        .map((p) => p.trim())
        .filter(Boolean),
      verify_checksum: verifyChecksum,
      detect_orphans: detectOrphans,
      auto_eject: autoEject,
    };
    try {
      await invoke("save_sync_preset", { preset });
      setPresets((prev) => [...prev, preset]);
      setPresetName("");
    } catch {
      // ignore
    }
  };

  const handleLoadPreset = (preset: SyncPreset) => {
    setSourceDir(preset.source_dir);
    setTargetDir(preset.target_dir);
    setExclusionPatterns(preset.exclusion_patterns.join("\n"));
    setVerifyChecksum(preset.verify_checksum);
    setDetectOrphans(preset.detect_orphans);
    setAutoEject(preset.auto_eject);
    setPhase("configure");
    setPlan(null);
    setResult(null);
  };

  const handleDeletePreset = async (id: string) => {
    try {
      await invoke("delete_sync_preset", { presetId: id });
      setPresets((prev) => prev.filter((p) => p.id !== id));
    } catch {
      // ignore
    }
  };

  const totalNewSize = plan
    ? plan.files_to_copy.reduce((s, f) => s + f.file_size, 0)
    : 0;
  const totalUpdateSize = plan
    ? plan.files_to_update.reduce((s, f) => s + f.file_size, 0)
    : 0;

  return (
    <div className="flex-1 flex overflow-hidden">
      {/* Left: Main area */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Header */}
        <div className="p-4 border-b border-border">
          <div className="flex items-center gap-3 mb-4">
            <RefreshCw size={20} className="text-accent" />
            <h2 className="text-sm font-semibold text-text-primary">
              {t("sync.title")}
            </h2>
          </div>

          {/* Source / Target folder selectors */}
          <div className="grid grid-cols-2 gap-4 mb-4">
            <div>
              <label className="text-xs text-text-secondary block mb-1">
                {t("sync.source")}
              </label>
              <button
                onClick={handleSelectSource}
                className="w-full flex items-center gap-2 px-3 py-2 rounded-md text-xs border border-border bg-bg-primary hover:bg-bg-secondary transition-colors text-left"
              >
                <FolderInput
                  size={14}
                  className="text-text-secondary shrink-0"
                />
                <span className="truncate text-text-primary">
                  {sourceDir || t("sync.selectSource")}
                </span>
              </button>
            </div>
            <div>
              <label className="text-xs text-text-secondary block mb-1">
                {t("sync.target")}
              </label>
              <button
                onClick={handleSelectTarget}
                className="w-full flex items-center gap-2 px-3 py-2 rounded-md text-xs border border-border bg-bg-primary hover:bg-bg-secondary transition-colors text-left"
              >
                <FolderOutput
                  size={14}
                  className="text-text-secondary shrink-0"
                />
                <span className="truncate text-text-primary">
                  {targetDir || t("sync.selectTarget")}
                </span>
              </button>
            </div>
          </div>

          {/* Exclusion patterns */}
          <div className="mb-4">
            <label className="text-xs text-text-secondary block mb-1">
              {t("sync.exclusionPatterns")}
            </label>
            <textarea
              value={exclusionPatterns}
              onChange={(e) => setExclusionPatterns(e.target.value)}
              placeholder={t("sync.exclusionPlaceholder")}
              rows={3}
              className="w-full px-3 py-2 rounded-md text-xs border border-border bg-bg-primary text-text-primary placeholder:text-text-secondary/50 resize-none focus:outline-none focus:ring-1 focus:ring-accent"
            />
          </div>

          {/* Checkboxes */}
          <div className="flex flex-wrap gap-4 mb-4">
            <label className="flex items-center gap-2 text-xs text-text-primary cursor-pointer">
              <input
                type="checkbox"
                checked={verifyChecksum}
                onChange={(e) => setVerifyChecksum(e.target.checked)}
                className="rounded border-border accent-accent"
              />
              {t("sync.verifyChecksum")}
            </label>
            <label className="flex items-center gap-2 text-xs text-text-primary cursor-pointer">
              <input
                type="checkbox"
                checked={detectOrphans}
                onChange={(e) => setDetectOrphans(e.target.checked)}
                className="rounded border-border accent-accent"
              />
              {t("sync.detectOrphans")}
            </label>
            <label className="flex items-center gap-2 text-xs text-text-primary cursor-pointer">
              <input
                type="checkbox"
                checked={autoEject}
                onChange={(e) => setAutoEject(e.target.checked)}
                className="rounded border-border accent-accent"
              />
              {t("sync.autoEject")}
            </label>
          </div>

          {/* Action buttons */}
          <div className="flex items-center gap-2">
            {phase === "configure" && (
              <button
                onClick={handlePreview}
                disabled={!sourceDir || !targetDir || loading}
                className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-bg-primary border border-border text-text-primary hover:bg-bg-secondary disabled:opacity-50 transition-colors"
              >
                <Eye size={14} />
                {loading ? t("sync.loading") : t("sync.preview")}
              </button>
            )}
            {phase === "previewed" && (
              <>
                <button
                  onClick={handleStartSync}
                  className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors"
                >
                  <Play size={14} />
                  {t("sync.startSync")}
                </button>
                <button
                  onClick={handleReset}
                  className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-bg-primary border border-border text-text-secondary hover:text-text-primary transition-colors"
                >
                  <X size={14} />
                  {t("sync.reset")}
                </button>
              </>
            )}
            {phase === "syncing" && (
              <button
                onClick={handleCancel}
                className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-danger text-white hover:bg-danger/90 transition-colors"
              >
                <Square size={14} />
                {t("sync.cancel")}
              </button>
            )}
            {phase === "done" && (
              <button
                onClick={handleReset}
                className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover transition-colors"
              >
                <RefreshCw size={14} />
                {t("sync.newSync")}
              </button>
            )}
          </div>
        </div>

        {/* Error */}
        {error && (
          <div className="p-4 border-b border-border bg-danger/5">
            <div className="flex items-center gap-2">
              <AlertTriangle size={16} className="text-danger" />
              <span className="text-xs text-danger">{error}</span>
            </div>
          </div>
        )}

        {/* Progress bar */}
        {phase === "syncing" && progress && (
          <div className="p-4 border-b border-border bg-accent/5">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-text-primary">
                {progress.phase === "copying"
                  ? t("sync.progressCopying")
                  : t("sync.progressPhase", { phase: progress.phase })}
              </span>
              <span className="text-xs text-text-secondary">
                {progress.current}/{progress.total}
              </span>
            </div>
            <div className="h-2 bg-bg-secondary rounded-full overflow-hidden">
              <div
                className="h-full bg-accent rounded-full transition-all"
                style={{
                  width: `${
                    progress.bytes_total > 0
                      ? (progress.bytes_done / progress.bytes_total) * 100
                      : progress.total > 0
                        ? (progress.current / progress.total) * 100
                        : 0
                  }%`,
                }}
              />
            </div>
            <div className="flex items-center justify-between mt-1">
              <p className="text-[10px] text-text-secondary truncate flex-1">
                {progress.current_file}
              </p>
              <span className="text-[10px] text-text-secondary shrink-0 ml-2">
                {formatFileSize(progress.bytes_done)} /{" "}
                {formatFileSize(progress.bytes_total)}
              </span>
            </div>
          </div>
        )}

        {/* Result display */}
        {phase === "done" && result && (
          <div
            className={`p-4 border-b border-border ${result.cancelled ? "bg-warning/5" : result.errors.length > 0 ? "bg-danger/5" : "bg-success/5"}`}
          >
            <div className="flex items-center gap-2 mb-2">
              {result.cancelled ? (
                <AlertTriangle size={16} className="text-warning" />
              ) : result.errors.length > 0 ? (
                <AlertTriangle size={16} className="text-danger" />
              ) : (
                <Check size={16} className="text-success" />
              )}
              <span className="text-sm font-medium text-text-primary">
                {result.cancelled
                  ? t("sync.resultCancelled")
                  : result.errors.length > 0
                    ? t("sync.resultWithErrors")
                    : t("sync.resultSuccess")}
              </span>
            </div>
            <div className="grid grid-cols-4 gap-3">
              <div className="text-center">
                <p className="text-lg font-bold text-accent">
                  {result.files_copied}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {t("sync.copied")}
                </p>
              </div>
              <div className="text-center">
                <p className="text-lg font-bold text-accent">
                  {result.files_updated}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {t("sync.updated")}
                </p>
              </div>
              <div className="text-center">
                <p className="text-lg font-bold text-text-secondary">
                  {result.files_skipped}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {t("sync.skipped")}
                </p>
              </div>
              <div className="text-center">
                <p className="text-lg font-bold text-accent">
                  {formatFileSize(result.bytes_transferred)}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {t("sync.transferred")}
                </p>
              </div>
            </div>
            {result.errors.length > 0 && (
              <div className="mt-3 max-h-32 overflow-y-auto">
                {result.errors.map((err, i) => (
                  <p key={i} className="text-[10px] text-danger">
                    {err}
                  </p>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Plan summary */}
        {phase === "previewed" && plan && (
          <div className="flex-1 overflow-y-auto p-4">
            <div className="grid grid-cols-4 gap-3 mb-4">
              <div className="rounded-lg border border-border p-3 text-center">
                <p className="text-2xl font-bold text-accent">
                  {plan.files_to_copy.length}
                </p>
                <p className="text-xs text-text-secondary">
                  {t("sync.newFiles")}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {formatFileSize(totalNewSize)}
                </p>
              </div>
              <div className="rounded-lg border border-border p-3 text-center">
                <p className="text-2xl font-bold text-accent">
                  {plan.files_to_update.length}
                </p>
                <p className="text-xs text-text-secondary">
                  {t("sync.updatedFiles")}
                </p>
                <p className="text-[10px] text-text-secondary">
                  {formatFileSize(totalUpdateSize)}
                </p>
              </div>
              <div className="rounded-lg border border-border p-3 text-center">
                <p className="text-2xl font-bold text-warning">
                  {plan.conflicts.length}
                </p>
                <p className="text-xs text-text-secondary">
                  {t("sync.conflicts")}
                </p>
              </div>
              <div className="rounded-lg border border-border p-3 text-center">
                <p className="text-2xl font-bold text-text-secondary">
                  {plan.orphan_files.length}
                </p>
                <p className="text-xs text-text-secondary">
                  {t("sync.orphans")}
                </p>
              </div>
            </div>

            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-text-primary">
                {t("sync.totalSize")}
              </span>
              <span className="text-xs text-text-secondary">
                {formatFileSize(plan.total_bytes)} / {plan.total_files}{" "}
                {t("sync.files")}
              </span>
            </div>

            {/* Conflict review */}
            {plan.conflicts.length > 0 && (
              <div className="mt-4">
                <h3 className="text-xs font-semibold text-text-primary mb-2 flex items-center gap-2">
                  <AlertTriangle size={14} className="text-warning" />
                  {t("sync.conflictReview")}
                </h3>
                <ConflictReviewPanel
                  conflicts={plan.conflicts}
                  onResolve={handleConflictResolve}
                  onResolveAll={handleConflictResolveAll}
                />
              </div>
            )}

            {/* Orphan files */}
            {plan.orphan_files.length > 0 && (
              <div className="mt-4">
                <h3 className="text-xs font-semibold text-text-primary mb-2">
                  {t("sync.orphanFiles")} ({plan.orphan_files.length})
                </h3>
                <div className="space-y-1 max-h-40 overflow-y-auto">
                  {plan.orphan_files.slice(0, 100).map((path, idx) => (
                    <div
                      key={idx}
                      className="px-3 py-1.5 rounded-md bg-bg-secondary text-xs text-text-secondary truncate"
                    >
                      {path}
                    </div>
                  ))}
                  {plan.orphan_files.length > 100 && (
                    <p className="text-[10px] text-text-secondary text-center py-1">
                      {t("sync.andMore", {
                        count: plan.orphan_files.length - 100,
                      })}
                    </p>
                  )}
                </div>
              </div>
            )}
          </div>
        )}

        {/* Empty state */}
        {phase === "configure" && !plan && (
          <div className="flex-1 flex items-center justify-center">
            <div className="text-center text-text-secondary">
              <RefreshCw size={48} className="mx-auto mb-3 opacity-30" />
              <p className="text-sm">{t("sync.emptyTitle")}</p>
              <p className="text-xs mt-1">{t("sync.emptyDescription")}</p>
            </div>
          </div>
        )}
      </div>

      {/* Right: Preset sidebar */}
      <div className="w-64 border-l border-border bg-bg-secondary flex flex-col overflow-hidden">
        <div className="p-3 border-b border-border">
          <h3 className="text-xs font-semibold text-text-primary mb-2">
            {t("sync.presets")}
          </h3>
          <div className="flex gap-2">
            <input
              type="text"
              value={presetName}
              onChange={(e) => setPresetName(e.target.value)}
              placeholder={t("sync.presetName")}
              className="flex-1 px-2 py-1.5 rounded-md text-xs border border-border bg-bg-primary text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent"
            />
            <button
              onClick={handleSavePreset}
              disabled={!presetName.trim() || !sourceDir || !targetDir}
              className="p-1.5 rounded-md bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
              title={t("sync.savePreset")}
            >
              <Save size={14} />
            </button>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-2 space-y-1">
          {presets.length === 0 && (
            <p className="text-[10px] text-text-secondary text-center py-4">
              {t("sync.noPresets")}
            </p>
          )}
          {presets.map((preset) => (
            <div
              key={preset.id}
              className="group rounded-md border border-border bg-bg-primary p-2 hover:border-accent/30 transition-colors"
            >
              <div className="flex items-center justify-between mb-1">
                <button
                  onClick={() => handleLoadPreset(preset)}
                  className="text-xs font-medium text-text-primary hover:text-accent transition-colors truncate flex-1 text-left"
                >
                  {preset.name}
                </button>
                <button
                  onClick={() => handleDeletePreset(preset.id)}
                  className="p-0.5 rounded opacity-0 group-hover:opacity-100 hover:text-danger transition-all"
                  title={t("sync.deletePreset")}
                >
                  <Trash2 size={12} />
                </button>
              </div>
              <p className="text-[10px] text-text-secondary truncate">
                {preset.source_dir}
              </p>
              <p className="text-[10px] text-text-secondary truncate">
                {preset.target_dir}
              </p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

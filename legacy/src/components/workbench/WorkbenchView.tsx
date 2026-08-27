import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { toast } from "@/stores/toastStore";
import { useVolumeEvents } from "@/hooks/useTauriEvents";
import { runPhase1Loop } from "@/utils/phase1";
import { formatFileSize } from "@/utils/format";
import {
  Import,
  CheckCircle2,
  FolderCog,
  UploadCloud,
  Sparkles,
  ArrowRight,
  Usb,
  Undo2,
  FolderPlus,
  RefreshCw,
} from "lucide-react";
import type { AppView, MediaStats, UndoBatch } from "@/types";

interface SourceFolder {
  id: string;
  path: string;
  name: string;
}

interface NasStatus {
  connected: boolean;
}

interface WatchStatus {
  watched_folders: string[];
}

interface McpStatus {
  running: boolean;
}

interface WorkbenchStats {
  total_count: number;
  total_size: number;
  unuploaded_count: number;
  undated_files: number;
  date_format_folders: number;
  dup_groups: number;
  burst_groups: number;
}

/**
 * 작업대 — 자료를 가져오고, 고르고, 정리하고, NAS에 올리는
 * 전 과정을 지휘하는 메인 컨트롤 화면.
 */
export function WorkbenchView() {
  const { t } = useTranslation();
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const stats = useAppStore((s) => s.stats);
  const setStats = useAppStore((s) => s.setStats);
  const setIsScanning = useAppStore((s) => s.setIsScanning);
  const refreshCounter = useAppStore((s) => s.refreshCounter);
  const triggerRefresh = useAppStore((s) => s.triggerRefresh);
  const config = useAppStore((s) => s.config);

  const [sources, setSources] = useState<SourceFolder[]>([]);
  // 작업 대상 — null이면 전체 라이브러리, 아니면 특정 소스 폴더 경로
  const [scopePath, setScopePath] = useState<string | null>(null);
  const [wbStats, setWbStats] = useState<WorkbenchStats | null>(null);
  const [nasConnected, setNasConnected] = useState<boolean | null>(null);
  const [watchCount, setWatchCount] = useState(0);
  const [mcpRunning, setMcpRunning] = useState(false);
  const [recentBatches, setRecentBatches] = useState<UndoBatch[]>([]);
  const [dismissedMounts, setDismissedMounts] = useState<Set<string>>(
    new Set(),
  );

  const { mounted } = useVolumeEvents();
  const pendingMount = [...mounted]
    .reverse()
    .find((mp) => !dismissedMounts.has(mp));

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const list = await invoke<SourceFolder[]>("get_source_folders");
        if (!cancelled) setSources(list);
      } catch {
        /* DB not ready */
      }
      try {
        const stats = await invoke<WorkbenchStats>("get_workbench_stats", {
          scope: scopePath,
        });
        if (!cancelled) setWbStats(stats);
      } catch {
        if (!cancelled) setWbStats(null);
      }
      try {
        const status = await invoke<NasStatus>("nas_status");
        if (!cancelled) setNasConnected(status.connected);
      } catch {
        if (!cancelled) setNasConnected(null);
      }
      try {
        const status = await invoke<WatchStatus>("get_watch_status");
        if (!cancelled) setWatchCount(status.watched_folders.length);
      } catch {
        /* ignore */
      }
      try {
        const status = await invoke<McpStatus>("get_mcp_status");
        if (!cancelled) setMcpRunning(status.running);
      } catch {
        /* ignore */
      }
      try {
        const batches = await invoke<UndoBatch[]>("get_undo_history", {
          limit: 3,
        });
        if (!cancelled) setRecentBatches(batches);
      } catch {
        /* ignore */
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [refreshCounter, scopePath]);

  /** Register a path as a source and run the scan flow. */
  const addAndScan = useCallback(
    async (path: string) => {
      const name = path.split("/").pop() || path;
      try {
        await invoke("add_source_folder", { path, name });
        setIsScanning(true);
        await invoke("scan_directory", { path });
        await invoke("update_folder_scan_time", { path });
        const newStats = await invoke<MediaStats>("get_media_stats");
        setStats(newStats);
        setIsScanning(false);
        triggerRefresh();
        runPhase1Loop();
        toast.success(t("workbench.importStarted", { name }));
      } catch (e) {
        setIsScanning(false);
        toast.error(`${e}`);
      }
    },
    [setIsScanning, setStats, triggerRefresh, t],
  );

  const handleAddFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) await addAndScan(selected as string);
  };

  const handleRescanAll = async () => {
    if (sources.length === 0) return;
    setIsScanning(true);
    try {
      for (const folder of sources) {
        await invoke("scan_directory", { path: folder.path });
        await invoke("update_folder_scan_time", { path: folder.path });
      }
      const newStats = await invoke<MediaStats>("get_media_stats");
      setStats(newStats);
      triggerRefresh();
      runPhase1Loop();
    } catch (e) {
      toast.error(`${e}`);
    }
    setIsScanning(false);
  };

  const handleUndo = async (batchId: string) => {
    try {
      await invoke("undo_batch", { batchId });
      toast.success(t("workbench.undone"));
      triggerRefresh();
    } catch (e) {
      toast.error(`${e}`);
    }
  };

  const enabledSchedules =
    config?.schedules.filter((s) => s.enabled).length ?? 0;

  const go = (view: AppView) => setCurrentView(view);

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="max-w-4xl mx-auto px-6 py-6">
        {/* Header + working-set selector */}
        <div className="flex items-center gap-3 mb-1">
          <h1 className="text-[16px] font-bold text-text-primary">
            {t("workbench.title")}
          </h1>
          {sources.length > 0 && (
            <div className="flex items-center gap-1.5 ml-auto">
              <span className="text-[10px] text-text-secondary">
                {t("workbench.scopeLabel")}
              </span>
              <select
                value={scopePath ?? ""}
                onChange={(e) => setScopePath(e.target.value || null)}
                className="h-7 px-2 rounded-md text-[11px] bg-bg-secondary border border-border text-text-primary focus:outline-none focus:border-accent/50 max-w-[220px]"
              >
                <option value="">{t("workbench.scopeAll")}</option>
                {sources.map((folder) => (
                  <option key={folder.path} value={folder.path}>
                    {folder.name}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>
        <p className="text-[11px] text-text-secondary mb-4">
          {sources.length > 0
            ? t("workbench.subtitle", {
                count: sources.length,
                total: (wbStats?.total_count ?? stats?.total_count)?.toLocaleString() ?? 0,
              })
            : t("workbench.subtitleEmpty")}
        </p>

        {/* Device alert */}
        {pendingMount && (
          <div className="mb-4 flex items-center gap-3 px-4 py-2.5 rounded-lg border border-warning/40 bg-warning/10">
            <Usb size={15} className="text-warning shrink-0" />
            <div className="flex-1 min-w-0">
              <p className="text-[12px] font-semibold text-text-primary">
                {t("workbench.deviceMounted", {
                  name: pendingMount.split("/").pop(),
                })}
              </p>
              <p className="text-[10px] text-text-secondary truncate">
                {pendingMount}
              </p>
            </div>
            <button
              onClick={() => {
                setDismissedMounts((prev) => new Set(prev).add(pendingMount));
                addAndScan(pendingMount);
              }}
              className="px-3 py-1.5 rounded-md text-[11px] font-bold bg-warning text-white hover:opacity-90 transition-opacity shrink-0"
            >
              {t("workbench.putOnBench")}
            </button>
            <button
              onClick={() =>
                setDismissedMounts((prev) => new Set(prev).add(pendingMount))
              }
              className="text-[10px] text-text-secondary hover:text-text-primary shrink-0"
            >
              {t("workbench.later")}
            </button>
          </div>
        )}

        {/* Pipeline board */}
        <p className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider mb-2">
          {t("workbench.pipeline")}
        </p>
        <div className="flex items-stretch gap-1.5 mb-4">
          {/* 01 가져오기 */}
          <StationCard
            step="01"
            icon={<Import size={14} />}
            title={t("workbench.stImport")}
          >
            <p className="text-[10.5px] text-text-secondary leading-relaxed">
              {t("workbench.stImportDesc", {
                sources: scopePath ? 1 : sources.length,
                total: wbStats?.total_count?.toLocaleString() ?? "—",
                size: wbStats ? formatFileSize(wbStats.total_size) : "—",
              })}
            </p>
            <div className="flex gap-1 mt-2">
              <StationButton onClick={handleAddFolder} icon={<FolderPlus size={10} />}>
                {t("workbench.addFolder")}
              </StationButton>
              <StationButton
                onClick={handleRescanAll}
                disabled={sources.length === 0}
                icon={<RefreshCw size={10} />}
              >
                {t("workbench.rescan")}
              </StationButton>
            </div>
          </StationCard>

          <Arrow />

          {/* 02 고르기 */}
          <StationCard
            step="02"
            icon={<CheckCircle2 size={14} />}
            title={t("workbench.stSelect")}
          >
            <p className="text-[10.5px] text-text-secondary leading-relaxed">
              {t("workbench.stSelectDesc", {
                dup: wbStats?.dup_groups ?? "—",
                burst: wbStats?.burst_groups ?? "—",
              })}
            </p>
            <div className="flex gap-1 mt-2">
              <StationButton primary onClick={() => go("duplicates")}>
                {t("workbench.goSelect")}
              </StationButton>
            </div>
          </StationCard>

          <Arrow />

          {/* 03 정리 */}
          <StationCard
            step="03"
            icon={<FolderCog size={14} />}
            title={t("workbench.stOrganize")}
          >
            <p className="text-[10.5px] text-text-secondary leading-relaxed">
              {t("workbench.stOrganizeDesc", {
                undated: wbStats?.undated_files?.toLocaleString() ?? "—",
                folders: wbStats?.date_format_folders ?? "—",
              })}
            </p>
            <div className="flex gap-1 mt-2">
              <StationButton onClick={() => go("organize")}>
                {t("hub.classify")}
              </StationButton>
              <StationButton onClick={() => go("foldertree")}>
                {t("hub.folders")}
              </StationButton>
            </div>
          </StationCard>

          <Arrow />

          {/* 04 올리기 */}
          <StationCard
            step="04"
            icon={<UploadCloud size={14} />}
            title={t("workbench.stUpload")}
          >
            <p className="text-[10.5px] text-text-secondary leading-relaxed">
              {t("workbench.stUploadDesc", {
                count: wbStats?.unuploaded_count?.toLocaleString() ?? "—",
              })}
              <br />
              <span
                className={
                  nasConnected ? "text-success" : "text-text-secondary/70"
                }
              >
                {nasConnected
                  ? t("workbench.nasConnected")
                  : t("workbench.nasDisconnected")}
              </span>
            </p>
            <div className="flex gap-1 mt-2">
              <StationButton primary onClick={() => go("nas")}>
                {t("workbench.goUpload")}
              </StationButton>
            </div>
          </StationCard>
        </div>

        {/* Smart cleanup CTA */}
        <button
          onClick={() => go("cleanup")}
          className="w-full flex items-center gap-3 px-4 py-3 rounded-lg border border-accent/40 bg-accent/10 hover:bg-accent/15 transition-colors text-left mb-5"
        >
          <Sparkles size={16} className="text-accent shrink-0" />
          <div className="flex-1">
            <p className="text-[12.5px] font-bold text-accent">
              {t("workbench.smartCleanup")}
            </p>
            <p className="text-[10.5px] text-text-secondary">
              {t("workbench.smartCleanupDesc")}
            </p>
          </div>
          <span className="px-3 py-1.5 rounded-md text-[11px] font-bold bg-accent text-white shrink-0">
            {t("workbench.start")}
          </span>
        </button>

        {/* Bottom row: recent activity + automation */}
        <div className="grid grid-cols-2 gap-4">
          <div className="rounded-lg border border-border bg-bg-secondary/30 p-3">
            <p className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider mb-2">
              {t("workbench.recent")}
            </p>
            {recentBatches.length === 0 ? (
              <p className="text-[10.5px] text-text-secondary/60 py-2">
                {t("workbench.noRecent")}
              </p>
            ) : (
              <div className="space-y-1">
                {recentBatches.map((batch) => (
                  <div
                    key={batch.batch_id}
                    className="flex items-center gap-2 py-1 border-b border-border/50 last:border-0"
                  >
                    <span className="flex-1 min-w-0 text-[11px] text-text-primary truncate">
                      {batch.batch_name || t("workbench.unnamedBatch")}
                    </span>
                    <button
                      onClick={() => handleUndo(batch.batch_id)}
                      className="flex items-center gap-1 text-[9.5px] text-text-secondary hover:text-accent transition-colors shrink-0"
                    >
                      <Undo2 size={10} />
                      {t("workbench.undo")}
                    </button>
                  </div>
                ))}
                <button
                  onClick={() => go("history")}
                  className="flex items-center gap-1 text-[10px] text-accent hover:underline mt-1"
                >
                  {t("workbench.allHistory")}
                  <ArrowRight size={10} />
                </button>
              </div>
            )}
          </div>

          <div className="rounded-lg border border-border bg-bg-secondary/30 p-3">
            <p className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider mb-2">
              {t("workbench.automation")}
            </p>
            <div className="space-y-1.5 text-[11px]">
              <AutomationRow
                label={t("workbench.watch")}
                value={
                  watchCount > 0
                    ? t("workbench.watchActive", { count: watchCount })
                    : t("workbench.off")
                }
                active={watchCount > 0}
              />
              <AutomationRow
                label={t("workbench.schedule")}
                value={
                  enabledSchedules > 0
                    ? t("workbench.scheduleActive", { count: enabledSchedules })
                    : t("workbench.off")
                }
                active={enabledSchedules > 0}
              />
              <AutomationRow
                label="MCP"
                value={mcpRunning ? t("workbench.running") : t("workbench.off")}
                active={mcpRunning}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StationCard({
  step,
  icon,
  title,
  children,
}: {
  step: string;
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex-1 min-w-0 rounded-lg border border-border bg-bg-secondary/30 p-3">
      <div className="flex items-center gap-1.5 mb-1.5">
        <span className="text-[9px] font-bold text-accent tabular-nums">
          {step}
        </span>
        <span className="text-accent">{icon}</span>
        <span className="text-[12px] font-bold text-text-primary">
          {title}
        </span>
      </div>
      {children}
    </div>
  );
}

function StationButton({
  children,
  onClick,
  primary,
  disabled,
  icon,
}: {
  children: React.ReactNode;
  onClick: () => void;
  primary?: boolean;
  disabled?: boolean;
  icon?: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`flex items-center gap-1 px-2 py-1 rounded text-[9.5px] font-semibold transition-colors disabled:opacity-40 ${
        primary
          ? "bg-accent text-white hover:bg-accent/90"
          : "bg-bg-primary border border-border text-text-secondary hover:text-text-primary"
      }`}
    >
      {icon}
      {children}
    </button>
  );
}

function Arrow() {
  return (
    <div className="self-center text-accent/60 shrink-0">
      <ArrowRight size={13} />
    </div>
  );
}

function AutomationRow({
  label,
  value,
  active,
}: {
  label: string;
  value: string;
  active: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-text-secondary">{label}</span>
      <span
        className={`flex items-center gap-1.5 ${
          active ? "text-text-primary" : "text-text-secondary/60"
        }`}
      >
        <span
          className={`w-1.5 h-1.5 rounded-full ${
            active ? "bg-success" : "bg-border"
          }`}
        />
        {value}
      </span>
    </div>
  );
}

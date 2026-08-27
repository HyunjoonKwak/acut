import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { toast } from "@/stores/toastStore";
import { useVolumeEvents } from "@/hooks/useTauriEvents";
import { runPhase1Loop } from "@/utils/phase1";
import { formatFileSize } from "@/utils/format";
import { Usb, HardDrive, LogOut, ScanSearch, FolderPlus } from "lucide-react";
import type { MediaStats, VolumeInfo } from "@/types";

/**
 * 장치 패널 — 내장·외장 저장장치를 모두 나열한다.
 * 각 장치는 "전체 검색"(루트를 소스로 등록+스캔) 또는
 * "폴더 선택…"(해당 장치에서 폴더를 골라 등록)으로 가져올 수 있다.
 */
export function VolumePanel() {
  const { t } = useTranslation();
  const [volumes, setVolumes] = useState<VolumeInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const { mounted, unmounted } = useVolumeEvents();
  const setIsScanning = useAppStore((s) => s.setIsScanning);
  const setStats = useAppStore((s) => s.setStats);
  const setSelectedFolder = useAppStore((s) => s.setSelectedFolder);
  const triggerRefresh = useAppStore((s) => s.triggerRefresh);

  const loadVolumes = async () => {
    try {
      const vols = await invoke<VolumeInfo[]>("get_mounted_volumes", {
        includeInternal: true,
      });
      setVolumes(vols);
    } catch {
      // ignore
    }
    setLoading(false);
  };

  // Load on mount
  useEffect(() => {
    loadVolumes();
  }, []);

  // Refresh when volumes mount/unmount
  useEffect(() => {
    if (mounted.length > 0 || unmounted.length > 0) {
      loadVolumes();
    }
  }, [mounted.length, unmounted.length]);

  const handleEject = async (mountPoint: string) => {
    try {
      await invoke("eject_volume", { mountPoint });
      setVolumes((prev) => prev.filter((v) => v.mount_point !== mountPoint));
    } catch {
      // ignore
    }
  };

  /** Register a path as a source folder and run the full scan flow. */
  const addAndScan = useCallback(
    async (path: string) => {
      const name = path === "/" ? "Macintosh HD" : path.split("/").pop() || path;
      try {
        await invoke("add_source_folder", { path, name });
        setSelectedFolder(path);
        setIsScanning(true);
        await invoke("scan_directory", { path });
        await invoke("update_folder_scan_time", { path });
        const stats = await invoke<MediaStats>("get_media_stats");
        setStats(stats);
        setIsScanning(false);
        triggerRefresh();
        runPhase1Loop();
        toast.success(t("volume.scanStarted", { name }));
      } catch (e) {
        setIsScanning(false);
        toast.error(`${e}`);
      }
    },
    [setIsScanning, setStats, setSelectedFolder, triggerRefresh, t],
  );

  const handleScanAll = (volume: VolumeInfo) => {
    addAndScan(volume.mount_point);
  };

  const handlePickFolder = async (volume: VolumeInfo) => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: volume.mount_point,
      });
      if (selected) {
        await addAndScan(selected as string);
      }
    } catch {
      // dialog cancelled
    }
  };

  if (loading) {
    return (
      <div className="p-3">
        <div className="w-4 h-4 border-2 border-accent border-t-transparent rounded-full animate-spin mx-auto" />
      </div>
    );
  }

  return (
    <div className="p-2">
      <h3 className="text-[10px] font-semibold text-text-secondary uppercase tracking-wider px-2 mb-2">
        {t("volume.title")}
      </h3>

      {volumes.length === 0 && (
        <p className="text-[10px] text-text-secondary text-center py-3 px-2">
          {t("volume.noVolumes")}
        </p>
      )}

      <div className="space-y-1">
        {volumes.map((volume) => {
          const usedSpace = volume.total_space - volume.free_space;
          const usedPercent =
            volume.total_space > 0
              ? (usedSpace / volume.total_space) * 100
              : 0;

          return (
            <div
              key={volume.mount_point}
              className="rounded-md border border-border bg-bg-primary p-2 group"
            >
              {/* Volume info */}
              <div className="flex items-center gap-2 mb-1.5">
                {volume.is_removable ? (
                  <Usb size={14} className="text-accent shrink-0" />
                ) : (
                  <HardDrive size={14} className="text-text-secondary shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <p className="text-[11px] font-medium text-text-primary truncate">
                    {volume.label || volume.mount_point.split("/").pop()}
                    {!volume.is_removable && (
                      <span className="ml-1 text-[9px] text-text-secondary">
                        · {t("volume.internal")}
                      </span>
                    )}
                  </p>
                  <p className="text-[9px] text-text-secondary truncate">
                    {volume.mount_point}
                  </p>
                </div>
              </div>

              {/* Space bar */}
              <div className="h-1.5 bg-bg-secondary rounded-full overflow-hidden mb-1">
                <div
                  className={`h-full rounded-full transition-all ${
                    usedPercent > 90
                      ? "bg-danger"
                      : usedPercent > 70
                        ? "bg-warning"
                        : "bg-accent"
                  }`}
                  style={{ width: `${usedPercent}%` }}
                />
              </div>

              {/* Space text */}
              <div className="flex items-center justify-between mb-2">
                <span className="text-[9px] text-text-secondary">
                  {formatFileSize(volume.free_space)} {t("volume.free")}
                </span>
                <span className="text-[9px] text-text-secondary">
                  {formatFileSize(volume.total_space)} {t("volume.total")}
                </span>
              </div>

              {/* Actions */}
              <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                <button
                  onClick={() => handleScanAll(volume)}
                  className="flex-1 flex items-center justify-center gap-1 px-2 py-1 rounded text-[10px] font-medium border border-border text-text-secondary hover:text-accent hover:border-accent/50 transition-colors"
                  title={t("volume.scanAll")}
                >
                  <ScanSearch size={11} />
                  {t("volume.scanAll")}
                </button>
                <button
                  onClick={() => handlePickFolder(volume)}
                  className="flex-1 flex items-center justify-center gap-1 px-2 py-1 rounded text-[10px] font-medium border border-border text-text-secondary hover:text-accent hover:border-accent/50 transition-colors"
                  title={t("volume.pickFolder")}
                >
                  <FolderPlus size={11} />
                  {t("volume.pickFolder")}
                </button>
                {volume.is_removable && (
                  <button
                    onClick={() => handleEject(volume.mount_point)}
                    className="flex items-center justify-center gap-1 px-2 py-1 rounded text-[10px] font-medium border border-border text-text-secondary hover:text-danger hover:border-danger/50 transition-colors"
                    title={t("volume.eject")}
                  >
                    <LogOut size={11} />
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

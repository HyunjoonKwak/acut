import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { useUpdateStore } from "@/stores/updateStore";
import { formatFileSize, formatCount } from "@/utils/format";
import { Search, FolderOpen, ArrowUpCircle } from "lucide-react";

/**
 * Slim top bar: app identity, global search, scan progress and library stats.
 * All navigation lives in the NavRail.
 */
export function TitleBar() {
  const { t } = useTranslation();
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const searchQuery = useAppStore((s) => s.searchQuery);
  const setSearchQuery = useAppStore((s) => s.setSearchQuery);
  const stats = useAppStore((s) => s.stats);
  const isScanning = useAppStore((s) => s.isScanning);
  const scanProgress = useAppStore((s) => s.scanProgress);
  const updateInfo = useUpdateStore((s) => s.info);

  const [appVersion, setAppVersion] = useState("");
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => {});
  }, []);

  const updateAvailable = updateInfo?.update_available;

  return (
    <header
      className="h-11 bg-bg-secondary/80 backdrop-blur-md border-b border-border flex items-center px-4 gap-2 shrink-0 select-none"
      data-tauri-drag-region
    >
      {/* Logo - click to go to the workbench */}
      <div
        className="flex items-center gap-1.5 mr-1 cursor-pointer"
        onClick={() => setCurrentView("dashboard")}
      >
        <FolderOpen size={16} className="text-accent" />
        <span className="font-semibold text-[13px] text-text-primary tracking-tight">
          {t("app.name")}
        </span>
      </div>

      {/* Version — click opens Settings (About tab has the updater) */}
      {appVersion && (
        <button
          onClick={() => setCurrentView("settings")}
          title={
            updateAvailable
              ? t("settings.aboutUpdateAvailable", {
                  version: updateInfo.latest_version,
                })
              : t("settings.aboutVersion")
          }
          className={`flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-mono tabular-nums transition-colors ${
            updateAvailable
              ? "text-accent bg-accent/10 hover:bg-accent/20"
              : "text-text-secondary/70 hover:text-text-primary"
          }`}
        >
          v{appVersion}
          {updateAvailable && <ArrowUpCircle size={11} />}
        </button>
      )}

      {/* Search */}
      <div className="flex-1 max-w-sm mx-3">
        <div className="relative">
          <Search
            size={13}
            className="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-secondary"
          />
          <input
            type="text"
            placeholder={t("app.search")}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full h-7 pl-8 pr-3 rounded-md text-[11px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50 focus:border-accent/50 transition-shadow"
          />
        </div>
      </div>

      <div className="flex-1" data-tauri-drag-region />

      {/* Scan progress */}
      {isScanning && scanProgress && (
        <div className="flex items-center gap-2 text-[10px] text-accent">
          <div className="w-16 h-1 bg-bg-primary rounded-full overflow-hidden">
            <div
              className="h-full bg-accent rounded-full transition-all"
              style={{
                width: `${scanProgress.total > 0 ? (scanProgress.current / scanProgress.total) * 100 : 0}%`,
              }}
            />
          </div>
          <span className="tabular-nums">
            {scanProgress.current}/{scanProgress.total}
          </span>
        </div>
      )}

      {/* Stats */}
      {stats && (
        <div
          className="text-[10px] text-text-secondary/70 tabular-nums"
          title={t("app.statsTooltip", {
            count: stats.total_count.toLocaleString(),
            size: formatFileSize(stats.total_size),
          })}
        >
          {formatCount(stats.total_count)} · {formatFileSize(stats.total_size)}
        </div>
      )}
    </header>
  );
}

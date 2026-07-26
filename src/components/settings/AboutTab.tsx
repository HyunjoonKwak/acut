import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
  RefreshCw,
  CheckCircle2,
  Download,
  ExternalLink,
  PartyPopper,
} from "lucide-react";
import { useUpdateStore } from "@/stores/updateStore";
import { formatDate, formatFileSize } from "@/utils/format";

export function AboutTab() {
  const { t } = useTranslation();

  const info = useUpdateStore((s) => s.info);
  const checking = useUpdateStore((s) => s.checking);
  const checkError = useUpdateStore((s) => s.checkError);
  const downloading = useUpdateStore((s) => s.downloading);
  const progress = useUpdateStore((s) => s.progress);
  const downloadedPath = useUpdateStore((s) => s.downloadedPath);
  const downloadError = useUpdateStore((s) => s.downloadError);
  const checkForUpdate = useUpdateStore((s) => s.checkForUpdate);
  const downloadUpdate = useUpdateStore((s) => s.downloadUpdate);

  const [appVersion, setAppVersion] = useState("");
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => {});
  }, []);

  const currentVersion = info?.current_version || appVersion;

  const handleOpenRelease = () => {
    if (!info?.release_url) return;
    invoke("open_release_page", { url: info.release_url }).catch(() => {});
  };

  return (
    <div className="max-w-lg space-y-6">
      {/* App identity + current version */}
      <div className="flex items-center justify-between rounded-lg border border-border p-4">
        <div>
          <p className="text-sm font-semibold text-text-primary">
            {t("app.name")}
          </p>
          <p className="text-[10px] text-text-secondary mt-0.5">
            {t("settings.aboutVersion")}{" "}
            <span className="font-mono text-text-primary">
              {currentVersion ? `v${currentVersion}` : "-"}
            </span>
          </p>
        </div>
        <button
          onClick={() => checkForUpdate()}
          disabled={checking}
          className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
        >
          <RefreshCw size={13} className={checking ? "animate-spin" : ""} />
          {checking
            ? t("settings.aboutChecking")
            : t("settings.aboutCheckUpdate")}
        </button>
      </div>

      {/* Check failed */}
      {checkError && (
        <p className="text-[11px] text-danger">
          {t("settings.aboutCheckFailed")}: {checkError}
        </p>
      )}

      {/* Up to date */}
      {info && !info.update_available && !checkError && (
        <div className="flex items-center gap-2 rounded-lg border border-border p-4">
          <CheckCircle2 size={16} className="text-success shrink-0" />
          <p className="text-xs text-text-primary">
            {t("settings.aboutUpToDate")}
          </p>
        </div>
      )}

      {/* Update available */}
      {info?.update_available && (
        <div className="rounded-lg border border-accent/40 bg-accent/5 p-4 space-y-3">
          <div className="flex items-center gap-2">
            <PartyPopper size={16} className="text-accent shrink-0" />
            <p className="text-xs font-semibold text-text-primary">
              {t("settings.aboutUpdateAvailable", {
                version: info.latest_version,
              })}
            </p>
            {info.published_at && (
              <span className="text-[10px] text-text-secondary ml-auto">
                {formatDate(info.published_at)}
              </span>
            )}
          </div>

          {info.release_notes && (
            <div className="max-h-40 overflow-y-auto rounded-md bg-bg-primary border border-border p-3">
              <p className="text-[11px] text-text-secondary whitespace-pre-wrap">
                {info.release_notes}
              </p>
            </div>
          )}

          {/* Download progress */}
          {downloading && (
            <div className="space-y-1">
              <div className="h-1.5 rounded-full bg-bg-primary overflow-hidden">
                <div
                  className="h-full bg-accent transition-all"
                  style={{ width: `${progress?.percent ?? 0}%` }}
                />
              </div>
              <p className="text-[10px] text-text-secondary">
                {t("settings.aboutDownloading", {
                  percent: progress?.percent ?? 0,
                })}
                {progress &&
                  ` (${formatFileSize(progress.downloaded)} / ${formatFileSize(progress.total)})`}
              </p>
            </div>
          )}

          {/* Downloaded */}
          {downloadedPath && !downloading && (
            <p className="text-[11px] text-success">
              {t("settings.aboutDownloaded")}
            </p>
          )}

          {/* Download failed */}
          {downloadError && !downloading && (
            <p className="text-[11px] text-danger">
              {t("settings.aboutDownloadFailed")}: {downloadError}
            </p>
          )}

          <div className="flex items-center gap-2">
            {info.asset_url && (
              <button
                onClick={() => downloadUpdate()}
                disabled={downloading}
                className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
              >
                <Download size={13} />
                {t("settings.aboutDownload")}
                {info.asset_size
                  ? ` (${formatFileSize(info.asset_size)})`
                  : ""}
              </button>
            )}
            <button
              onClick={handleOpenRelease}
              className="flex items-center gap-2 px-4 py-2 rounded-md text-xs font-medium border border-border text-text-secondary hover:text-text-primary transition-colors"
            >
              <ExternalLink size={13} />
              {t("settings.aboutOpenRelease")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

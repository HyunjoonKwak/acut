import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { useUpdateStore } from "@/stores/updateStore";
import { toast } from "@/stores/toastStore";
import { useScanProgress } from "@/hooks/useTauriEvents";
import { TitleBar } from "@/components/layout/TitleBar";
import { NavRail } from "@/components/layout/NavRail";
import { Sidebar } from "@/components/sidebar/Sidebar";
import { viewToArea } from "@/utils/navigation";
import { WorkbenchView } from "@/components/workbench/WorkbenchView";
import { LibraryView } from "@/components/library/LibraryView";
import { CullingWorkspace } from "@/components/culling/CullingWorkspace";
import { OrganizeHub } from "@/components/organize/OrganizeHub";
import { CleanupWizardView } from "@/components/cleanup/CleanupWizardView";
import { SettingsView } from "@/components/settings/SettingsView";
import { TagManager } from "@/components/tags/TagManager";
import { AlbumManager } from "@/components/albums/AlbumManager";
import { NasUploadView } from "@/components/nas/NasUploadView";
import { Toasts } from "@/components/common/Toasts";
import { resumePhase1IfPending } from "@/utils/phase1";
import type { AppConfig, MediaComment, MediaStats } from "@/types";

function App() {
  const { t } = useTranslation();
  const currentView = useAppStore((s) => s.currentView);
  const setStats = useAppStore((s) => s.setStats);
  const setConfig = useAppStore((s) => s.setConfig);
  const config = useAppStore((s) => s.config);
  const setNasUploadedIds = useAppStore((s) => s.setNasUploadedIds);
  const refreshCounter = useAppStore((s) => s.refreshCounter);

  useScanProgress();

  useEffect(() => {
    const loadInitial = async () => {
      try {
        const stats = await invoke<MediaStats>("get_media_stats");
        setStats(stats);
      } catch {
        // DB not ready yet
      }
      try {
        const config = await invoke<AppConfig>("get_config");
        setConfig(config);
      } catch {
        // Config not ready yet
      }
    };
    loadInitial();
  }, [setStats, setConfig, refreshCounter]);

  // Silent update check shortly after startup; nudge via toast + rail badge
  const checkForUpdate = useUpdateStore((s) => s.checkForUpdate);
  useEffect(() => {
    const timer = setTimeout(async () => {
      const info = await checkForUpdate();
      if (info?.update_available) {
        toast.info(
          t("settings.aboutUpdateToast", { version: info.latest_version })
        );
      }
    }, 3000);
    return () => clearTimeout(timer);
  }, [checkForUpdate, t]);

  // Apply theme to document when config changes
  useEffect(() => {
    if (!config) return;
    if (config.theme === "dark") {
      document.documentElement.setAttribute("data-theme", "dark");
    } else if (config.theme === "light") {
      document.documentElement.setAttribute("data-theme", "light");
    } else {
      document.documentElement.removeAttribute("data-theme");
    }
  }, [config]);

  useEffect(() => {
    const loadUploaded = async () => {
      try {
        const ids = await invoke<string[]>("nas_uploaded_media_ids");
        setNasUploadedIds(new Set(ids));
      } catch {
        // DB not ready yet
      }
    };
    loadUploaded();
  }, [setNasUploadedIds, refreshCounter]);

  // Load media comments for grid badges / inspector
  const setComments = useAppStore((s) => s.setComments);
  useEffect(() => {
    const loadComments = async () => {
      try {
        const list = await invoke<MediaComment[]>("get_media_comments");
        setComments(new Map(list.map((c) => [c.media_id, c.comment])));
      } catch {
        // DB not ready yet
      }
    };
    loadComments();
  }, [setComments, refreshCounter]);

  // Apply persisted gallery thumb size once config arrives
  const setThumbSize = useAppStore((s) => s.setThumbSize);
  useEffect(() => {
    if (config?.gallery?.thumb_size) {
      setThumbSize(config.gallery.thumb_size);
    }
  }, [config, setThumbSize]);

  // Resume thumbnail/EXIF processing left over from an interrupted scan
  // or the base64-to-file thumbnail migration
  useEffect(() => {
    resumePhase1IfPending();
  }, []);

  const renderMainContent = () => {
    switch (currentView) {
      case "dashboard":
        return <WorkbenchView />;
      case "gallery":
      case "map":
        return <LibraryView />;
      case "duplicates":
      case "bcut":
      case "review":
        return <CullingWorkspace />;
      case "organize":
      case "before-after":
      case "foldertree":
      case "sync":
      case "history":
        return <OrganizeHub />;
      case "cleanup":
        return <CleanupWizardView />;
      case "settings":
        return <SettingsView />;
      case "tags":
        return <TagManager />;
      case "albums":
        return <AlbumManager />;
      case "nas":
        return <NasUploadView />;
      default:
        return <WorkbenchView />;
    }
  };

  // The context sidebar (sources / filters) belongs to the library area only
  const showSidebar = viewToArea(currentView) === "library";

  return (
    <div className="flex flex-col h-screen bg-bg-primary">
      <TitleBar />
      <div className="flex flex-1 overflow-hidden">
        <NavRail />
        {showSidebar && <Sidebar />}
        <main className="flex-1 flex flex-col overflow-hidden">
          {renderMainContent()}
        </main>
      </div>
      <Toasts />
    </div>
  );
}

export default App;

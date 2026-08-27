import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { AreaTabs } from "@/components/layout/AreaTabs";
import { OrganizeView } from "@/components/organize/OrganizeView";
import { FolderTreeView } from "@/components/foldertree/FolderTreeView";
import { SyncView } from "@/components/sync/SyncView";
import { HistoryView } from "@/components/organize/HistoryView";
import type { AppView } from "@/types";

/**
 * 정리 허브 — 파일과 폴더를 옮기고 다듬는 모든 작업:
 * 자동 분류 / 폴더 / 동기화 / 히스토리.
 */
export function OrganizeHub() {
  const { t } = useTranslation();
  const currentView = useAppStore((s) => s.currentView);
  const setCurrentView = useAppStore((s) => s.setCurrentView);

  // "before-after" is the preview step of auto-sort — same tab
  const activeTab = currentView === "before-after" ? "organize" : currentView;

  const tabs = [
    { id: "organize", label: t("hub.classify") },
    { id: "foldertree", label: t("hub.folders") },
    { id: "sync", label: t("nav.sync") },
    { id: "history", label: t("nav.history") },
  ];

  return (
    <div className="flex flex-col flex-1 overflow-hidden">
      <div className="px-4 pt-3 pb-2 border-b border-border shrink-0">
        <AreaTabs
          tabs={tabs}
          activeId={activeTab}
          onSelect={(id) => setCurrentView(id as AppView)}
        />
      </div>
      <div className="flex-1 flex flex-col overflow-hidden">
        {(currentView === "organize" || currentView === "before-after") && (
          <OrganizeView />
        )}
        {currentView === "foldertree" && <FolderTreeView />}
        {currentView === "sync" && <SyncView />}
        {currentView === "history" && <HistoryView />}
      </div>
    </div>
  );
}

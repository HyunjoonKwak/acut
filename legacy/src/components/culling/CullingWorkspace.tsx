import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { AreaTabs } from "@/components/layout/AreaTabs";
import { DuplicatesView } from "@/components/duplicates/DuplicatesView";
import { BcutView } from "@/components/bcut/BcutView";
import { ReviewView } from "@/components/review/ReviewView";
import type { AppView } from "@/types";

interface GroupSummary {
  total_groups: number;
}

/**
 * 고르기 워크스페이스 — 같은 작업(그룹에서 남길 사진 고르기)을 하는
 * 세 모드를 하나의 탭 공간으로 묶는다: 중복 / 연사 / 한 장씩.
 */
export function CullingWorkspace() {
  const { t } = useTranslation();
  const currentView = useAppStore((s) => s.currentView);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const refreshCounter = useAppStore((s) => s.refreshCounter);
  const [dupCount, setDupCount] = useState<number | null>(null);
  const [burstCount, setBurstCount] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const summary = await invoke<GroupSummary>("get_duplicate_groups");
        if (!cancelled) setDupCount(summary.total_groups);
      } catch {
        if (!cancelled) setDupCount(null);
      }
      try {
        const summary = await invoke<GroupSummary>("get_bcut_groups");
        if (!cancelled) setBurstCount(summary.total_groups);
      } catch {
        if (!cancelled) setBurstCount(null);
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [refreshCounter, currentView]);

  const tabs = [
    { id: "duplicates", label: t("culling.duplicates"), count: dupCount },
    { id: "bcut", label: t("culling.bursts"), count: burstCount },
    { id: "review", label: t("culling.review") },
  ];

  return (
    <div className="flex flex-col flex-1 overflow-hidden">
      <div className="px-4 pt-3 pb-2 border-b border-border shrink-0">
        <AreaTabs
          tabs={tabs}
          activeId={currentView}
          onSelect={(id) => setCurrentView(id as AppView)}
        />
      </div>
      <div className="flex-1 flex flex-col overflow-hidden">
        {currentView === "duplicates" && <DuplicatesView />}
        {currentView === "bcut" && <BcutView />}
        {currentView === "review" && <ReviewView />}
      </div>
    </div>
  );
}

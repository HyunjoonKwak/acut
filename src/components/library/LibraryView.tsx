import { useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { AreaTabs } from "@/components/layout/AreaTabs";
import { GalleryGrid } from "@/components/gallery/GalleryGrid";
import { MapView } from "@/components/map/MapView";
import { InspectorPanel } from "@/components/library/InspectorPanel";

/** Discrete steps for ⌘+ / ⌘− zoom */
const THUMB_STEPS = [80, 120, 160, 240, 320];

/**
 * 라이브러리 — 모든 사진·영상 탐색.
 * 격자/리스트/지도는 별도 화면이 아니라 보기 방식 탭이고,
 * 우측 인스펙터에서 태그·앨범·코멘트를 처리한다.
 */
export function LibraryView() {
  const { t } = useTranslation();
  const currentView = useAppStore((s) => s.currentView);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const viewMode = useAppStore((s) => s.viewMode);
  const setViewMode = useAppStore((s) => s.setViewMode);
  const thumbSize = useAppStore((s) => s.thumbSize);
  const setThumbSize = useAppStore((s) => s.setThumbSize);
  const inspectorMedia = useAppStore((s) => s.inspectorMedia);
  const tagFilter = useAppStore((s) => s.tagFilter);
  const setTagFilter = useAppStore((s) => s.setTagFilter);
  const albumFilter = useAppStore((s) => s.albumFilter);
  const setAlbumFilter = useAppStore((s) => s.setAlbumFilter);

  const activeTab = currentView === "map" ? "map" : viewMode;

  const tabs = [
    { id: "grid", label: t("libraryView.grid") },
    { id: "list", label: t("libraryView.list") },
    { id: "map", label: t("libraryView.map") },
  ];

  const onSelect = (id: string) => {
    if (id === "map") {
      setCurrentView("map");
      return;
    }
    setViewMode(id as "grid" | "list");
    if (currentView !== "gallery") setCurrentView("gallery");
  };

  /** Persist the thumb size into config.yaml (called on slider release). */
  const persistThumbSize = useCallback((size: number) => {
    invoke("update_config", {
      partial: { gallery: { thumb_size: size } },
    }).catch(() => {
      // Non-fatal: the in-session value still applies
    });
  }, []);

  // ⌘+ / ⌘− step the thumbnail size (grid view only)
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      if (e.key !== "+" && e.key !== "=" && e.key !== "-") return;
      e.preventDefault();
      const current = useAppStore.getState().thumbSize;
      const idx = THUMB_STEPS.findIndex((s) => s >= current);
      const at = idx === -1 ? THUMB_STEPS.length - 1 : idx;
      const next =
        e.key === "-"
          ? THUMB_STEPS[Math.max(0, at - 1)]
          : THUMB_STEPS[Math.min(THUMB_STEPS.length - 1, at + 1)];
      setThumbSize(next);
      persistThumbSize(next);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [setThumbSize, persistThumbSize]);

  return (
    <div className="flex flex-col flex-1 overflow-hidden">
      <div className="px-4 pt-3 pb-2 border-b border-border shrink-0 flex items-center gap-4">
        <AreaTabs tabs={tabs} activeId={activeTab} onSelect={onSelect} />

        {/* Active tag/album filter chips */}
        {tagFilter && (
          <FilterChip
            label={`# ${tagFilter.name}`}
            title={t("libraryView.clearFilter")}
            onClear={() => setTagFilter(null)}
          />
        )}
        {albumFilter && (
          <FilterChip
            label={albumFilter.name}
            title={t("libraryView.clearFilter")}
            onClear={() => setAlbumFilter(null)}
          />
        )}

        {/* Thumbnail size slider (grid view only) */}
        {activeTab === "grid" && (
          <div
            className="flex items-center gap-2 text-text-secondary"
            title={t("libraryView.thumbSize")}
          >
            <span className="text-[9px]">S</span>
            <input
              type="range"
              min={80}
              max={320}
              step={8}
              value={thumbSize}
              onChange={(e) => setThumbSize(Number(e.target.value))}
              onPointerUp={() => persistThumbSize(useAppStore.getState().thumbSize)}
              className="w-24 accent-[var(--color-accent,#0E7A6C)]"
              aria-label={t("libraryView.thumbSize")}
            />
            <span className="text-[9px]">L</span>
            <span className="text-[9px] tabular-nums w-8">{thumbSize}px</span>
          </div>
        )}
      </div>
      <div className="flex-1 flex overflow-hidden">
        <div className="flex-1 flex flex-col overflow-hidden">
          {currentView === "map" ? <MapView /> : <GalleryGrid />}
        </div>
        {inspectorMedia && currentView !== "map" && <InspectorPanel />}
      </div>
    </div>
  );
}

function FilterChip({
  label,
  title,
  onClear,
}: {
  label: string;
  title: string;
  onClear: () => void;
}) {
  return (
    <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-accent/10 text-accent text-[10.5px] font-medium">
      {label}
      <button
        onClick={onClear}
        title={title}
        className="hover:text-danger transition-colors leading-none"
      >
        ✕
      </button>
    </span>
  );
}

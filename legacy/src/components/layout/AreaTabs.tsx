export interface AreaTab {
  id: string;
  label: string;
  /** Optional badge count shown next to the label (hidden when null/0). */
  count?: number | null;
}

/**
 * Segmented tab control shared by area workspaces
 * (culling, organize hub, library view switcher).
 */
export function AreaTabs({
  tabs,
  activeId,
  onSelect,
}: {
  tabs: AreaTab[];
  activeId: string;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="flex items-center gap-0.5 bg-bg-primary border border-border rounded-lg p-0.5 w-fit">
      {tabs.map((tab) => {
        const isActive = activeId === tab.id;
        return (
          <button
            key={tab.id}
            onClick={() => onSelect(tab.id)}
            className={`px-3 py-1 rounded-md text-[11px] font-medium transition-colors flex items-center gap-1.5 ${
              isActive
                ? "bg-bg-secondary text-text-primary shadow-sm"
                : "text-text-secondary hover:text-text-primary"
            }`}
          >
            {tab.label}
            {tab.count != null && tab.count > 0 && (
              <span
                className={`text-[9px] tabular-nums ${
                  isActive ? "text-accent" : "text-text-secondary/70"
                }`}
              >
                {tab.count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

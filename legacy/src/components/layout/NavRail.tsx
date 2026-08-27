import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { useUpdateStore } from "@/stores/updateStore";
import {
  viewToArea,
  AREA_DEFAULT_VIEW,
  type AppArea,
} from "@/utils/navigation";
import {
  LayoutDashboard,
  Images,
  CheckCircle2,
  FolderCog,
  UploadCloud,
  Settings,
} from "lucide-react";

const AREAS: Array<{
  id: AppArea;
  icon: React.ComponentType<{ size?: number }>;
  labelKey: string;
}> = [
  { id: "workbench", icon: LayoutDashboard, labelKey: "rail.workbench" },
  { id: "library", icon: Images, labelKey: "rail.library" },
  { id: "select", icon: CheckCircle2, labelKey: "rail.select" },
  { id: "organize", icon: FolderCog, labelKey: "rail.organize" },
  { id: "nas", icon: UploadCloud, labelKey: "rail.nas" },
];

/**
 * Left icon rail — the app's single top-level navigation.
 * Ordered top-to-bottom in workflow order: bring in, pick, organize, upload.
 */
export function NavRail() {
  const { t } = useTranslation();
  const currentView = useAppStore((s) => s.currentView);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const lastAreaViews = useAppStore((s) => s.lastAreaViews);
  const updateAvailable = useUpdateStore((s) => s.info?.update_available);
  const activeArea = viewToArea(currentView);

  const goToArea = (area: AppArea) => {
    if (area === activeArea) return;
    setCurrentView(lastAreaViews[area] ?? AREA_DEFAULT_VIEW[area]);
  };

  return (
    <nav
      className="w-14 shrink-0 bg-bg-secondary border-r border-border flex flex-col items-center py-3 gap-1 select-none"
      aria-label={t("rail.ariaLabel")}
    >
      {AREAS.map((area) => (
        <RailButton
          key={area.id}
          icon={area.icon}
          label={t(area.labelKey)}
          isActive={activeArea === area.id}
          onClick={() => goToArea(area.id)}
        />
      ))}
      <div className="flex-1" />
      <RailButton
        icon={Settings}
        label={t("rail.settings")}
        isActive={activeArea === "settings"}
        onClick={() => goToArea("settings")}
        badge={updateAvailable}
      />
    </nav>
  );
}

function RailButton({
  icon: Icon,
  label,
  isActive,
  onClick,
  badge,
}: {
  icon: React.ComponentType<{ size?: number }>;
  label: string;
  isActive: boolean;
  onClick: () => void;
  badge?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      className={`relative w-11 h-11 rounded-lg flex flex-col items-center justify-center gap-0.5 transition-colors ${
        isActive
          ? "bg-accent/15 text-accent"
          : "text-text-secondary hover:text-text-primary hover:bg-bg-primary"
      }`}
    >
      {badge && (
        <span className="absolute top-1.5 right-1.5 w-1.5 h-1.5 rounded-full bg-accent" />
      )}
      <Icon size={17} />
      <span className="text-[8px] font-medium leading-none">{label}</span>
    </button>
  );
}

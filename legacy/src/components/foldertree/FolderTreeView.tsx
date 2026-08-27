import { useState } from "react";
import { FolderTree, HardDrive, Columns2, CalendarDays } from "lucide-react";
import { TreePanel } from "./TreePanel";
import { SizeAnalysisPanel } from "./SizeAnalysisPanel";
import { DateFolderRenamePanel } from "./DateFolderRenamePanel";

type ViewMode = "tree" | "size" | "split" | "date-rename";

const MODE_ITEMS: Array<{ id: ViewMode; label: string; icon: typeof FolderTree }> = [
  { id: "tree", label: "폴더 탐색", icon: FolderTree },
  { id: "size", label: "용량 분석", icon: HardDrive },
  { id: "split", label: "분할 비교", icon: Columns2 },
  { id: "date-rename", label: "날짜 폴더 정리", icon: CalendarDays },
];

export function FolderTreeView() {
  const [mode, setMode] = useState<ViewMode>("tree");

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Mode tab bar */}
      <div className="flex items-center gap-1 px-3 py-1 border-b border-border bg-bg-secondary/30">
        {MODE_ITEMS.map((item) => (
          <button
            key={item.id}
            onClick={() => setMode(item.id)}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-[11px] font-medium transition-colors ${
              mode === item.id
                ? "bg-accent text-white"
                : "text-text-secondary hover:bg-bg-primary hover:text-text-primary"
            }`}
          >
            <item.icon size={12} />
            {item.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 flex overflow-hidden">
        {mode === "tree" && <TreePanel />}
        {mode === "size" && <SizeAnalysisPanel />}
        {mode === "split" && (
          <>
            <TreePanel />
            <div className="w-px bg-border shrink-0" />
            <TreePanel />
          </>
        )}
        {mode === "date-rename" && <DateFolderRenamePanel />}
      </div>
    </div>
  );
}

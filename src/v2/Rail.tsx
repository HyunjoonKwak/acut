/**
 * 왼쪽 아이콘 레일 — 사이드바가 무엇을 보여줄지 고른다.
 *
 * Lap의 Home.vue와 같은 구조다. 사이드바를 여러 갈래(라이브러리·폴더·날짜·
 * 카메라…)로 쓰려면 그 갈래를 고르는 자리가 따로 있어야 한다. 갈래를 다시
 * 누르면 패널이 접혀 사진이 넓어진다.
 */

export type Source =
  "library" | "folder" | "date" | "camera" | "rating" | "trash";

export const SOURCES: { v: Source; icon: string; label: string }[] = [
  { v: "library", icon: "▤", label: "라이브러리" },
  { v: "folder", icon: "🗀", label: "폴더" },
  { v: "date", icon: "🗓", label: "날짜" },
  { v: "camera", icon: "📷", label: "카메라" },
  { v: "rating", icon: "★", label: "평점" },
  { v: "trash", icon: "🗑", label: "휴지통" },
];

export default function Rail({
  value,
  open,
  onPick,
  trashCount,
}: {
  value: Source;
  /** 패널이 펴져 있는가 */
  open: boolean;
  onPick: (s: Source) => void;
  trashCount: number;
}) {
  return (
    <div className="w-12 shrink-0 flex flex-col items-center gap-1 py-2 bg-rail border-r border-line">
      {SOURCES.map((s) => {
        const on = value === s.v && open;
        return (
          <button
            key={s.v}
            onClick={() => onPick(s.v)}
            title={s.label}
            className={`relative w-9 h-9 rounded-lg text-[15px] flex items-center justify-center ${
              on
                ? "bg-raised text-accent"
                : "text-fg-mute hover:text-fg hover:bg-chrome"
            }`}
          >
            {s.icon}
            {s.v === "trash" && trashCount > 0 && (
              <span className="absolute top-0.5 right-0.5 min-w-[14px] h-[14px] px-0.5 rounded-full bg-drop text-drop-fg text-[9px] font-bold flex items-center justify-center tabular-nums">
                {trashCount > 99 ? "99+" : trashCount}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

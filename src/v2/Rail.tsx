import { useState } from "react";
import {
  IconAlbum,
  IconAll,
  IconCalendar,
  IconCamera,
  IconLocation,
  IconPeople,
  IconSearch,
  IconSettings,
  IconSmart,
  IconTag,
  IconTrash,
} from "./icons";
import { FOOT, SOURCES, type Entry, type Source } from "./railItems";
import { usePref } from "./prefs";

const ICON: Record<Source, (p: { className?: string }) => React.ReactElement> =
  {
    all: IconAll,
    album: IconAlbum,
    smart: IconSmart,
    search: IconSearch,
    tag: IconTag,
    people: IconPeople,
    calendar: IconCalendar,
    location: IconLocation,
    camera: IconCamera,
    trash: IconTrash,
    settings: IconSettings,
  };

/**
 * 왼쪽 아이콘 레일 — 사이드바가 무엇을 보여줄지 고른다.
 *
 * 아이콘만 놓고 이름은 커서가 왔을 때만 띄운다. 열 칸 밑에 9pt 글자를
 * 항상 깔면 레일이 글자벽이 되고, 정작 아이콘은 작아져 알아보기 어렵다.
 *
 * 이름표는 `title` 속성이 아니라 직접 그린다 — 브라우저 기본 툴팁은 1초쯤
 * 뒤에야 뜨고 배경이 앱과 따로 논다.
 */
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
  const [hover, setHover] = useState<Source | null>(null);
  const [tooltips] = usePref("tooltips");

  const item = (s: Entry) => {
    const on = value === s.v && open;
    const Icon = ICON[s.v];
    return (
      <button
        key={s.v}
        onClick={() => onPick(s.v)}
        onPointerEnter={() => setHover(s.v)}
        onPointerLeave={() => setHover((h) => (h === s.v ? null : h))}
        onFocus={() => setHover(s.v)}
        onBlur={() => setHover((h) => (h === s.v ? null : h))}
        aria-label={s.label}
        aria-pressed={on}
        className={`relative w-11 h-11 rounded-xl flex items-center justify-center transition-colors ${
          on
            ? "bg-raised text-accent"
            : "text-fg-mute hover:text-fg hover:bg-chrome"
        }`}
      >
        <Icon className="w-[21px] h-[21px]" />

        {/* 고른 갈래에는 왼쪽에 짧은 막대 — 아이콘 색만으로는 약하다 */}
        {on && (
          <span className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-5 rounded-r bg-accent" />
        )}

        {s.v === "trash" && trashCount > 0 && (
          <span className="absolute top-0.5 right-0.5 min-w-[15px] h-[15px] px-1 rounded-full bg-drop text-drop-fg text-[10px] font-bold flex items-center justify-center tabular-nums">
            {trashCount > 99 ? "99+" : trashCount}
          </span>
        )}

        {tooltips && hover === s.v && (
          <span
            className="absolute left-full ml-1.5 z-50 px-2 py-1 rounded-md bg-raised text-fg
              text-[13px] whitespace-nowrap shadow-lg ring-1 ring-line-strong pointer-events-none"
          >
            {s.label}
          </span>
        )}
      </button>
    );
  };

  return (
    <div className="w-14 shrink-0 flex flex-col items-center gap-1 py-2 bg-rail border-r border-line">
      {SOURCES.map(item)}
      <div className="flex-1" />
      {FOOT.map(item)}
    </div>
  );
}

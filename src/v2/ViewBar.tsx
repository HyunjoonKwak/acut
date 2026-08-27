import { useState } from "react";
import {
  IconCaption,
  IconCard,
  IconContain,
  IconFill,
  IconFilmstrip,
  IconJustified,
  IconStretch,
  IconTile,
} from "./icons";
import type { GridStyle, Scaling } from "./gridStyle";
import { next } from "./cycle";

/**
 * 보기 방식 — 툴바의 버튼 넷.
 *
 * 격자 모양과 담는 방식은 **누를 때마다 다음 것으로** 바뀐다. 셋을 나란히
 * 늘어놓으니 툴바가 아이콘 여덟 개가 되어 읽히지 않았다. 버튼에 지금 상태의
 * 그림과 이름을 함께 써서 무엇인지 열어 보지 않아도 안다.
 * 이름·크기와 필름스트립은 켜고 끄는 것이라 그대로.
 */

type IconOf = (p: { className?: string }) => React.ReactElement;

const STYLES: { v: GridStyle; label: string; Icon: IconOf }[] = [
  { v: "card", label: "카드", Icon: IconCard },
  { v: "tile", label: "타일", Icon: IconTile },
  { v: "justified", label: "양쪽 맞춤", Icon: IconJustified },
];

const SCALINGS: { v: Scaling; label: string; Icon: IconOf }[] = [
  { v: "cover", label: "채우기", Icon: IconFill },
  { v: "contain", label: "전체", Icon: IconContain },
  { v: "fill", label: "늘리기", Icon: IconStretch },
];

function Tip({ children }: { children: React.ReactNode }) {
  return (
    <span
      className="absolute top-full mt-1.5 left-1/2 -translate-x-1/2 z-50 px-2 py-1 rounded-md
        bg-raised text-fg text-[11.5px] whitespace-nowrap shadow-lg ring-1 ring-line-strong
        pointer-events-none"
    >
      {children}
    </span>
  );
}

/** 돌아가며 바뀌는 버튼 — 지금 것의 그림과 이름을 쓴다 */
function Cycle<T extends { v: string; label: string; Icon: IconOf }>({
  items,
  value,
  onChange,
  what,
}: {
  items: T[];
  value: string;
  onChange: (v: T["v"]) => void;
  /** 이름표 앞에 붙는 말 — «보기 방식» */
  what: string;
}) {
  const [hover, setHover] = useState(false);
  const cur = items.find((x) => x.v === value) ?? items[0];
  const nxt = next(items, value);
  return (
    <button
      onClick={() => onChange(nxt.v)}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      onFocus={() => setHover(true)}
      onBlur={() => setHover(false)}
      aria-label={`${what}: ${cur.label}`}
      className="relative h-control pl-1.5 pr-2 rounded-md inline-flex items-center gap-1.5
        bg-raised text-fg-dim hover:text-fg transition-colors"
    >
      <cur.Icon className="w-[17px] h-[17px]" />
      <span className="text-[12px]">{cur.label}</span>
      {hover && (
        <Tip>
          {what}: {cur.label} → 누르면 {nxt.label}
        </Tip>
      )}
    </button>
  );
}

/** 켜고 끄는 버튼 */
function Toggle({
  label,
  on,
  onClick,
  children,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  const [hover, setHover] = useState(false);
  return (
    <button
      onClick={onClick}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      onFocus={() => setHover(true)}
      onBlur={() => setHover(false)}
      aria-label={label}
      aria-pressed={on}
      className={`relative h-control w-control rounded-md inline-flex items-center justify-center
        transition-colors ${on ? "bg-canvas text-accent shadow-sm ring-1 ring-line" : "bg-raised text-fg-mute hover:text-fg"}`}
    >
      {children}
      {hover && (
        <Tip>
          {label} {on ? "끄기" : "켜기"}
        </Tip>
      )}
    </button>
  );
}

export default function ViewBar({
  style,
  scaling,
  onStyle,
  onScaling,
  caption,
  onCaption,
  filmstrip,
  onFilmstrip,
}: {
  style: GridStyle;
  scaling: Scaling;
  onStyle: (s: GridStyle) => void;
  onScaling: (s: Scaling) => void;
  caption: boolean;
  onCaption: (v: boolean) => void;
  filmstrip: boolean;
  onFilmstrip: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-1">
      <Cycle items={STYLES} value={style} onChange={onStyle} what="보기" />
      {/* 양쪽 맞춤은 사진 비를 지키므로 담는 방식이 의미 없다 */}
      {style !== "justified" && (
        <Cycle
          items={SCALINGS}
          value={scaling}
          onChange={onScaling}
          what="담기"
        />
      )}
      <Toggle
        label="이름·크기 표시"
        on={caption}
        onClick={() => onCaption(!caption)}
      >
        <IconCaption className="w-[17px] h-[17px]" />
      </Toggle>
      <Toggle
        label="필름스트립"
        on={filmstrip}
        onClick={() => onFilmstrip(!filmstrip)}
      >
        <IconFilmstrip className="w-[17px] h-[17px]" />
      </Toggle>
    </div>
  );
}

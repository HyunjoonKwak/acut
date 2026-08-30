import { chips, without } from "./chips";
import type { Picks } from "./picks";
import { EMPTY } from "./picks";

/**
 * 툴바에 늘어놓는 「지금 걸린 조건」.
 *
 * 사이드바에서 태그나 자리를 고르면 목록이 확 줄어드는데, 화면 어디에도
 * 무엇 때문에 줄었는지가 없었다. 사이드바를 접으면 단서가 아예 사라진다.
 */
export default function FilterChips({
  value,
  onChange,
  tagName,
}: {
  value: Picks;
  onChange: (p: Picks) => void;
  /** 태그 id로 이름을 찾는다 */
  tagName: (id: number) => string | undefined;
}) {
  const items = chips(value, tagName);
  if (items.length === 0) return null;

  return (
    <div className="flex items-center gap-1 min-w-0">
      {items.map((c) => (
        <button
          key={c.key}
          onClick={() => onChange(without(value, c.key))}
          title={`${c.label} 조건 떼기`}
          className="group flex items-center gap-1 h-control max-w-[180px] pl-2 pr-1.5
            rounded-md bg-raised text-[12.5px] text-fg-dim hover:text-fg"
        >
          <span className="truncate">{c.label}</span>
          <span className="text-fg-faint group-hover:text-drop">✕</span>
        </button>
      ))}
      {items.length > 1 && (
        <button
          onClick={() => onChange(EMPTY)}
          className="h-control px-2 rounded-md text-[12.5px] text-fg-mute hover:text-fg"
        >
          모두 지우기
        </button>
      )}
    </div>
  );
}

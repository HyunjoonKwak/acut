import { Btn, Menu, MenuItem, MenuSep } from "./ui";
import { SORT_ITEMS as ITEMS, sortLabel, type Sort } from "./sortItems";

export default function SortMenu({
  value,
  onChange,
  compact = false,
}: {
  value: Sort;
  onChange: (s: Sort) => void;
  /** 좁은 창 — 방향 화살표만 */
  compact?: boolean;
}) {
  return (
    <Menu
      align="right"
      trigger={() => (
        <Btn title={`정렬: ${sortLabel(value)}`}>
          <span className="text-fg-mute">{value.desc ? "↓" : "↑"}</span>
          {!compact && sortLabel(value)}
        </Btn>
      )}
    >
      {(close) => (
        <>
          {ITEMS.map((i) => (
            <MenuItem
              key={i.by}
              selected={i.by === value.by}
              hint={i.by === value.by ? (value.desc ? "↓" : "↑") : undefined}
              onClick={() => {
                // 같은 기준을 다시 누르면 방향이 바뀐다
                onChange(
                  i.by === value.by
                    ? { by: i.by, desc: !value.desc }
                    : { by: i.by, desc: true },
                );
                close();
              }}
            >
              {i.label}
            </MenuItem>
          ))}
          <MenuSep />
          <MenuItem
            onClick={() => {
              onChange({ ...value, desc: !value.desc });
              close();
            }}
          >
            {value.desc ? "오름차순으로" : "내림차순으로"}
          </MenuItem>
        </>
      )}
    </Menu>
  );
}

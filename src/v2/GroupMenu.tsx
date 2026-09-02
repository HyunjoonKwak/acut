import { Btn, Menu, MenuItem } from "./ui";
import { GROUP_ITEMS as ITEMS, type GroupBy } from "./groupItems";

export default function GroupMenu({
  value,
  onChange,
  compact = false,
}: {
  value: GroupBy;
  onChange: (g: GroupBy) => void;
  /** 좁은 창 — 아이콘만 */
  compact?: boolean;
}) {
  const cur = ITEMS.find((i) => i.by === value) ?? ITEMS[0];
  const on = value !== "none";
  return (
    <Menu
      align="right"
      trigger={(_, props) => (
        <Btn
          {...props}
          active={on}
          title={on ? `묶기: ${cur.label}` : "묶어 보기"}
        >
          <span className={on ? "text-accent" : undefined}>▤</span>
          {!compact && (on ? cur.label : "묶기")}
        </Btn>
      )}
    >
      {(close) =>
        ITEMS.map((i) => (
          <MenuItem
            key={i.by}
            selected={i.by === value}
            onClick={() => {
              onChange(i.by);
              close();
            }}
          >
            {i.label}
          </MenuItem>
        ))
      }
    </Menu>
  );
}

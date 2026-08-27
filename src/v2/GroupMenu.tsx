import { Btn, Menu, MenuItem } from "./ui";
import { GROUP_ITEMS as ITEMS, type GroupBy } from "./groupItems";

export default function GroupMenu({
  value,
  onChange,
}: {
  value: GroupBy;
  onChange: (g: GroupBy) => void;
}) {
  const cur = ITEMS.find((i) => i.by === value) ?? ITEMS[0];
  const on = value !== "none";
  return (
    <Menu
      align="right"
      trigger={() => (
        <Btn active={on} title="묶어 보기">
          <span className={on ? "text-accent" : undefined}>▤</span>
          {on ? cur.label : "묶기"}
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

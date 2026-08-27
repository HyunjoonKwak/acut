import { Btn, Menu, MenuItem } from "./ui";

/** 백엔드 `db::query::GroupBy`와 이름이 같아야 한다 */
export type GroupBy =
  | "none"
  | "folder"
  | "day"
  | "month"
  | "year"
  | "rating"
  | "camera"
  | "lens"
  | "file_type"
  | "culling";

const ITEMS: { by: GroupBy; label: string }[] = [
  { by: "none", label: "묶지 않음" },
  { by: "day", label: "날짜" },
  { by: "month", label: "월" },
  { by: "year", label: "연도" },
  { by: "folder", label: "폴더" },
  { by: "rating", label: "평점" },
  { by: "culling", label: "판정" },
  { by: "file_type", label: "종류" },
  { by: "camera", label: "카메라" },
  { by: "lens", label: "렌즈" },
];

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

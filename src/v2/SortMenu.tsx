import { Btn, Menu, MenuItem, MenuSep } from "./ui";

/** 백엔드 `db::query::SortBy`와 이름이 같아야 한다 */
export type SortBy =
  | "taken_at"
  | "created_at"
  | "modified_at"
  | "name"
  | "size"
  | "pixels"
  | "duration";

export type Sort = { by: SortBy; desc: boolean };

export const DEFAULT_SORT: Sort = { by: "taken_at", desc: true };

/** Lap의 정렬 목록과 같다 */
const ITEMS: { by: SortBy; label: string }[] = [
  { by: "taken_at", label: "촬영일" },
  { by: "created_at", label: "생성일" },
  { by: "modified_at", label: "수정일" },
  { by: "name", label: "이름" },
  { by: "size", label: "크기" },
  { by: "pixels", label: "픽셀 크기" },
  { by: "duration", label: "재생시간" },
];

export const sortLabel = (s: Sort) =>
  ITEMS.find((i) => i.by === s.by)?.label ?? "정렬";

export default function SortMenu({
  value,
  onChange,
}: {
  value: Sort;
  onChange: (s: Sort) => void;
}) {
  return (
    <Menu
      align="right"
      trigger={() => (
        <Btn title="정렬 기준">
          <span className="text-fg-mute">{value.desc ? "↓" : "↑"}</span>
          {sortLabel(value)}
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

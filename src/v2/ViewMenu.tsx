import { Btn, Menu, MenuItem, MenuSep } from "./ui";
import { SCALINGS, STYLES, type GridStyle, type Scaling } from "./gridStyle";

/** 보기 방식 — 격자 모양과 사진을 칸에 어떻게 담을지 */
export default function ViewMenu({
  style,
  scaling,
  onStyle,
  onScaling,
  filmstrip,
  onFilmstrip,
}: {
  style: GridStyle;
  scaling: Scaling;
  onStyle: (s: GridStyle) => void;
  onScaling: (s: Scaling) => void;
  filmstrip: boolean;
  onFilmstrip: (v: boolean) => void;
}) {
  const icon = style === "card" ? "▢" : style === "tile" ? "▦" : "▤";
  return (
    <Menu align="right" trigger={() => <Btn title="보기 방식">{icon}</Btn>}>
      {(close) => (
        <>
          {STYLES.map((s) => (
            <MenuItem
              key={s.v}
              selected={s.v === style}
              onClick={() => {
                onStyle(s.v);
                close();
              }}
            >
              {s.label}
            </MenuItem>
          ))}
          {/* 양쪽 맞춤은 사진 비를 지키므로 담는 방식이 의미 없다 */}
          {style !== "justified" && (
            <>
              <MenuSep />
              {SCALINGS.map((s) => (
                <MenuItem
                  key={s.v}
                  selected={s.v === scaling}
                  onClick={() => {
                    onScaling(s.v);
                    close();
                  }}
                >
                  {s.label}
                </MenuItem>
              ))}
            </>
          )}
          <MenuSep />
          <MenuItem
            selected={filmstrip}
            onClick={() => {
              onFilmstrip(!filmstrip);
              close();
            }}
          >
            필름스트립
          </MenuItem>
        </>
      )}
    </Menu>
  );
}

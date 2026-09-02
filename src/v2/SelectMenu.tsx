import { useSelection } from "./selectionStore";
import { useUi } from "./uiStore";
import { Btn, Menu, MenuItem, MenuSep } from "./ui";
import type { FileRow, Mark } from "./types";

/**
 * 선택 메뉴 — 툴바에서 여러 장을 한 번에 고르고 처리한다 (Lap의 «선택항목»).
 *
 * 고르는 조건은 **지금 불러온 목록** 안에서만 본다 — 목록은 쪽 단위로 오니
 * 아직 안 내려온 사진은 고를 수 없다. ⌘A와 같은 범위다.
 * 처리(남김·제외·정리·휴지통)는 아래 선택 패널과 같은 손잡이를 쓴다 —
 * 메뉴는 «한 곳에 모아 둔 것»이지 다른 일이 아니다.
 */
export default function SelectMenu({
  rows,
  matched,
  compareIds,
  markPicked,
  onTrash,
}: {
  rows: FileRow[];
  /** 현재 조건 전체 장수 — 아직 불러오지 않은 사진이 있음을 정확히 말한다 */
  matched: number;
  /** 나란히 놓을 것 — 목록 순서로 앞의 넷 */
  compareIds: number[];
  markPicked: (patch: Mark) => void;
  onTrash: (ids: number[]) => Promise<boolean>;
}) {
  const picked = useSelection((s) => s.picked);
  const setUi = useUi((s) => s.set);
  const n = picked.size;
  const allLabel =
    matched > rows.length
      ? `불러온 ${rows.length.toLocaleString()}장 모두 고르기`
      : "모두 고르기";

  const pickWhere = (keep: (r: FileRow) => boolean) =>
    useSelection.getState().setPicked(rows.filter(keep).map((r) => r.id));

  return (
    <Menu
      align="right"
      width={210}
      trigger={(_, props) => (
        <Btn {...props} active={n > 0} title="선택">
          <span className={n > 0 ? "text-accent" : undefined}>☑</span>
          {n > 0 ? `${n.toLocaleString()}장 선택` : "선택"}
        </Btn>
      )}
    >
      {(close) => {
        const run = (f: () => void) => () => {
          f();
          close();
        };
        return (
          <>
            <MenuItem hint="⌘A" onClick={run(() => pickWhere(() => true))}>
              {allLabel}
            </MenuItem>
            <MenuItem
              onClick={run(() => pickWhere((r) => !picked.has(r.id)))}
            >
              반대로 고르기
            </MenuItem>
            {n > 0 && (
              <MenuItem
                hint="Esc"
                onClick={run(() => useSelection.getState().clearPicked())}
              >
                고른 것 풀기
              </MenuItem>
            )}
            <MenuSep />
            <MenuItem onClick={run(() => pickWhere((r) => r.culling_flag === 1))}>
              남김만 고르기
            </MenuItem>
            <MenuItem onClick={run(() => pickWhere((r) => r.culling_flag === 2))}>
              제외만 고르기
            </MenuItem>
            <MenuItem onClick={run(() => pickWhere((r) => r.culling_flag === 0))}>
              판정 없는 것만 고르기
            </MenuItem>
            <MenuItem onClick={run(() => pickWhere((r) => r.favorite))}>
              즐겨찾기만 고르기
            </MenuItem>
            <MenuItem onClick={run(() => pickWhere((r) => r.rating > 0))}>
              별점 있는 것만 고르기
            </MenuItem>
            <MenuItem onClick={run(() => pickWhere((r) => r.kind === 1))}>
              영상만 고르기
            </MenuItem>
            {n > 0 && (
              <>
                <MenuSep />
                {n >= 2 && (
                  <MenuItem
                    hint="C"
                    onClick={run(() => setUi({ comparing: compareIds }))}
                  >
                    나란히 보기
                  </MenuItem>
                )}
                <MenuItem hint="P" onClick={run(() => markPicked({ cullingFlag: 1 }))}>
                  고른 것 남김
                </MenuItem>
                <MenuItem hint="X" onClick={run(() => markPicked({ cullingFlag: 2 }))}>
                  고른 것 제외
                </MenuItem>
                <MenuItem hint="F" onClick={run(() => markPicked({ favorite: true }))}>
                  고른 것 즐겨찾기
                </MenuItem>
                <MenuItem onClick={run(() => markPicked({ cullingFlag: 0 }))}>
                  남김·제외 표시 취소
                </MenuItem>
                <MenuItem
                  onClick={run(() =>
                    markPicked({ cullingFlag: 0, favorite: false, rating: 0 }),
                  )}
                >
                  판정·별점 지우기
                </MenuItem>
                <MenuSep />
                <MenuItem onClick={run(() => setUi({ organizing: true }))}>
                  정리…
                </MenuItem>
                <MenuItem
                  danger
                  onClick={run(async () => {
                    if (await onTrash([...picked]))
                      useSelection.getState().clearPicked();
                  })}
                >
                  휴지통으로
                </MenuItem>
              </>
            )}
          </>
        );
      }}
    </Menu>
  );
}

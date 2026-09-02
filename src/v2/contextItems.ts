import { invoke } from "@tauri-apps/api/core";
import type { MenuItem } from "./ContextMenu";
import { useSelection } from "./selectionStore";
import { useUi } from "./uiStore";
import { toast } from "./toastStore";
import type { FileRow, Mark } from "./types";

/**
 * 타일 우클릭 메뉴의 항목들.
 *
 * `ids`는 우클릭한 순간 잡힌 사진들 — 고른 것 밖을 우클릭하면 그것 하나만.
 * 안 그러면 눈에 안 보이는 선택에 대고 일이 벌어진다 (잡는 쪽에서 처리).
 */
export function contextItems(
  ids: number[],
  rows: FileRow[],
  act: {
    markOne: (id: number, patch: Mark) => Promise<void>;
    trashFiles: (ids: number[]) => Promise<boolean>;
  },
): MenuItem[] {
  const n = ids.length;
  const many = n > 1 ? ` ${n.toLocaleString()}장` : "";
  const mark = (patch: Mark) => async () => {
    try {
      await Promise.all(ids.map((id) => act.markOne(id, patch)));
    } catch (e) {
      toast(`판정을 저장하지 못했습니다 — ${String(e)}`, "drop");
    }
  };
  const ui = useUi.getState;

  return [
    {
      kind: "item",
      label: "크게 보기",
      hint: "Space",
      run: () => {
        const i = rows.findIndex((r) => r.id === ids[0]);
        if (i >= 0) ui().set({ viewerAt: i });
      },
    },
    ...(n >= 2
      ? ([
          {
            kind: "item",
            label: `나란히 보기 (${Math.min(n, 4)}장)`,
            run: () => ui().set({ comparing: ids.slice(0, 4) }),
          },
        ] as MenuItem[])
      : []),
    { kind: "sep" },
    {
      kind: "item",
      label: `남김으로${many}`,
      hint: "P",
      run: mark({ cullingFlag: 1 }),
    },
    {
      kind: "item",
      label: `제외로${many}`,
      hint: "X",
      run: mark({ cullingFlag: 2 }),
    },
    {
      kind: "item",
      label: "판정 지우기",
      hint: "0",
      run: mark({ cullingFlag: 0 }),
    },
    {
      kind: "item",
      label: "즐겨찾기",
      hint: "F",
      run: mark({ favorite: true }),
    },
    { kind: "sep" },
    {
      kind: "item",
      label: `정리하기${many}`,
      run: () => {
        useSelection.getState().setPicked(ids);
        ui().set({ organizing: true, organizeSelection: null });
      },
    },
    {
      kind: "item",
      label: `휴지통으로 보내기${many}`,
      danger: true,
      run: async () => {
        await act.trashFiles(ids);
      },
    },
    { kind: "sep" },
    ...(n === 1
      ? ([
          {
            kind: "item",
            label: "비슷한 사진 찾기",
            run: () => ui().set({ similarFor: ids[0] }),
          },
          {
            kind: "item",
            label: "이름 바꾸기…",
            run: () => ui().set({ renaming: ids[0] }),
          },
        ] as MenuItem[])
      : []),
    {
      kind: "item",
      label: "Finder에서 보기",
      run: () => {
        invoke("reveal_in_finder", { id: ids[0] }).catch((e) =>
          toast(String(e), "drop"),
        );
      },
    },
  ];
}

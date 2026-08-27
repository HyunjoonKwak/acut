import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import SmartEdit from "./SmartEdit";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import { Btn } from "./ui";

export type SmartAlbum = {
  id: number;
  name: string;
  filter: unknown;
  sort: unknown;
};

/**
 * 스마트 앨범 — 조건에 이름을 붙여 둔 것.
 *
 * 「별 4개 이상 영상」처럼 되풀이해 쓰는 조건을 매번 다시 고르지 않게 한다.
 * 누르면 그 조건이 걸리고, ✎로 고치고, 지금 조건으로 새로 만들 수 있다.
 */
export default function SmartPanel({
  current,
  currentSort,
  hasFilter,
  onApply,
}: {
  /** 지금 걸린 조건 — «지금 조건으로 만들기»의 시작점 */
  current: unknown;
  currentSort: unknown;
  hasFilter: boolean;
  onApply: (filter: unknown, sort: unknown) => void;
}) {
  const ask = useConfirm();
  const [items, setItems] = useState<SmartAlbum[]>([]);
  /// 편집 상자. `null`은 닫힘, `{ id: 0 }`은 새로 만들기
  const [editing, setEditing] = useState<SmartAlbum | null | "new">(null);

  const reload = useCallback(() => {
    invoke<SmartAlbum[]>("smart_list")
      .then(setItems)
      .catch(() => setItems([]));
  }, []);
  useEffect(reload, [reload]);

  const remove = async (a: SmartAlbum) => {
    const ok = await ask({
      title: `「${a.name}」을 지웁니다`,
      lines: ["· 조건만 사라집니다. 사진은 그대로입니다"],
      confirmLabel: "지우기",
      danger: true,
    });
    if (!ok) return;
    await invoke("smart_delete", { id: a.id });
    toast(`「${a.name}」 지웠습니다`);
    reload();
  };

  return (
    <>
      <div className="px-2 pb-2 flex flex-wrap gap-1">
        <Btn tone="accent" onClick={() => setEditing("new")}>
          {hasFilter ? "지금 조건으로 만들기…" : "새로 만들기…"}
        </Btn>
      </div>

      {items.length === 0 ? (
        <div className="px-3 py-2 text-[12px] text-fg-mute leading-relaxed">
          저장한 것이 없습니다.
          <br />
          찾기·태그·평점을 고른 뒤 「지금 조건으로 만들기」.
        </div>
      ) : (
        items.map((a) => (
          <div
            key={a.id}
            className="group flex items-center pr-1 hover:bg-raised"
          >
            <button
              onClick={() => onApply(a.filter, a.sort)}
              className="flex-1 min-w-0 text-left px-3 py-1.5 text-[12.5px] text-fg-dim hover:text-fg truncate"
            >
              ✦ {a.name}
            </button>
            <button
              onClick={() => setEditing(a)}
              title="고치기"
              className="hidden group-hover:block px-1 text-fg-mute hover:text-fg"
            >
              ✎
            </button>
            <button
              onClick={() => remove(a)}
              title="지우기"
              className="hidden group-hover:block px-1 text-fg-mute hover:text-drop"
            >
              ✕
            </button>
          </div>
        ))
      )}

      {editing !== null && (
        <SmartEdit
          initial={
            editing === "new"
              ? hasFilter
                ? { id: 0, name: "", filter: current, sort: currentSort }
                : null
              : editing
          }
          onClose={() => setEditing(null)}
          onSaved={reload}
        />
      )}
    </>
  );
}

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
 * 지금 화면에 걸린 조건을 그대로 저장한다.
 */
export default function SmartPanel({
  current,
  currentSort,
  hasFilter,
  onApply,
}: {
  /** 지금 걸린 조건 */
  current: unknown;
  currentSort: unknown;
  /** 저장할 만한 조건이 걸려 있는가 */
  hasFilter: boolean;
  onApply: (filter: unknown, sort: unknown) => void;
}) {
  const [items, setItems] = useState<SmartAlbum[]>([]);
  const [name, setName] = useState("");

  const reload = useCallback(() => {
    invoke<SmartAlbum[]>("smart_list")
      .then(setItems)
      .catch(() => setItems([]));
  }, []);
  useEffect(reload, [reload]);

  const save = async () => {
    const n = name.trim();
    if (!n) return;
    await invoke("smart_save", { name: n, filter: current, sort: currentSort });
    setName("");
    reload();
  };

  return (
    <>
      <div className="px-2 pb-2 flex gap-1">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
          placeholder="지금 조건을 이 이름으로…"
          className="flex-1 min-w-0 h-control px-2 rounded-md bg-canvas text-[12px] text-fg
            placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
        />
        <Btn onClick={save} disabled={!name.trim()}>
          저장
        </Btn>
      </div>
      {!hasFilter && (
        <div className="px-3 pb-2 text-[11px] text-fg-faint leading-relaxed">
          지금은 아무 조건도 안 걸려 있습니다. 찾기·태그·평점 등을 고른 뒤
          저장하세요.
        </div>
      )}

      {items.length === 0 ? (
        <div className="px-3 py-2 text-[12px] text-fg-mute">
          저장한 것이 없습니다
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
              onClick={async () => {
                await invoke("smart_delete", { id: a.id });
                reload();
              }}
              title="지우기"
              className="hidden group-hover:block px-1 text-fg-mute hover:text-drop"
            >
              ✕
            </button>
          </div>
        ))
      )}
    </>
  );
}

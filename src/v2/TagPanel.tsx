import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Btn } from "./ui";
import { useConfirm } from "./confirmContext";

export type Tag = {
  id: number;
  name: string;
  color: string | null;
  count: number;
};

/**
 * 태그 갈래 — 폴더로는 표현 못 하는 묶음.
 *
 * 사진 하나는 폴더 한 곳에만 있지만 태그는 여럿 붙는다. 「주원」이면서
 * 「생일」인 사진이 그렇다.
 */
export default function TagPanel({
  selected,
  onPick,
  pickedIds,
  onChanged,
}: {
  selected: number | null;
  onPick: (id: number | null) => void;
  /** 지금 고른 사진들 — 여기에 태그를 붙인다 */
  pickedIds: number[];
  onChanged: () => void;
}) {
  const ask = useConfirm();
  const [tags, setTags] = useState<Tag[]>([]);
  const [adding, setAdding] = useState("");

  const reload = useCallback(() => {
    invoke<Tag[]>("tags_list")
      .then(setTags)
      .catch(() => setTags([]));
  }, []);
  useEffect(reload, [reload]);

  const attach = async () => {
    const name = adding.trim();
    if (!name || pickedIds.length === 0) return;
    await invoke("tag_add", { ids: pickedIds, name });
    setAdding("");
    reload();
    onChanged();
  };

  return (
    <>
      {pickedIds.length > 0 && (
        <div className="px-2 pb-2 flex gap-1">
          <input
            value={adding}
            onChange={(e) => setAdding(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && attach()}
            placeholder={`${pickedIds.length}장에 태그…`}
            className="flex-1 min-w-0 h-control px-2 rounded-md bg-canvas text-[12px] text-fg
              placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
          />
          <Btn onClick={attach}>붙이기</Btn>
        </div>
      )}

      {tags.length === 0 ? (
        <div className="px-3 py-2 text-[12px] text-fg-mute leading-relaxed">
          아직 태그가 없습니다.
          <br />
          사진을 고른 뒤 위에 이름을 적으세요.
        </div>
      ) : (
        <>
          <button
            onClick={() => onPick(null)}
            className={`w-full text-left px-3 py-1.5 text-[12.5px] ${
              selected === null ? "bg-raised text-fg" : "text-fg-dim"
            }`}
          >
            전체
          </button>
          {tags.map((t) => (
            <div
              key={t.id}
              className={`group flex items-center pr-1 ${
                selected === t.id ? "bg-raised" : ""
              }`}
            >
              <button
                onClick={() => onPick(selected === t.id ? null : t.id)}
                className={`flex-1 min-w-0 text-left px-3 py-1 text-[12.5px] truncate ${
                  selected === t.id ? "text-fg" : "text-fg-dim hover:text-fg"
                }`}
              >
                {t.name}
              </button>
              <span className="text-fg-faint tabular-nums text-[11px] px-1">
                {t.count.toLocaleString()}
              </span>
              <button
                onClick={async () => {
                  const ok = await ask({
                    title: `태그 「${t.name}」을 지웁니다`,
                    lines: [
                      `· ${t.count.toLocaleString()}장에서 이 태그가 떨어집니다`,
                      "· 사진은 지워지지 않습니다",
                    ],
                    confirmLabel: "태그 지우기",
                    danger: true,
                  });
                  if (!ok) return;
                  await invoke("tag_delete", { tagId: t.id });
                  if (selected === t.id) onPick(null);
                  reload();
                }}
                title="태그 지우기"
                className="hidden group-hover:block px-1 text-fg-mute hover:text-drop"
              >
                ✕
              </button>
            </div>
          ))}
        </>
      )}
    </>
  );
}

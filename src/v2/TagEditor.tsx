import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * 한 장의 태그 — 인스펙터 안.
 *
 * 사이드바의 태그 갈래가 "이 태그가 붙은 것 보기"라면, 여기는 "이 사진에
 * 무엇을 붙일까"다. 붙일 때는 이미 쓴 이름을 먼저 보여 준다 — 같은 뜻인데
 * 「여행」과 「여행지」로 갈라지는 것을 막는다.
 */
export default function TagEditor({
  id,
  onChanged,
}: {
  id: number;
  /** 목록의 장수가 달라질 수 있어 바깥에 알린다 */
  onChanged?: () => void;
}) {
  const [mine, setMine] = useState<{ id: number; name: string }[]>([]);
  const [all, setAll] = useState<string[]>([]);
  const [text, setText] = useState("");

  const reload = useCallback(() => {
    invoke<{ id: number; name: string }[]>("tags_of", { id })
      .then(setMine)
      .catch(() => setMine([]));
  }, [id]);
  useEffect(reload, [reload]);
  useEffect(() => {
    invoke<{ name: string }[]>("tags_list")
      .then((t) => setAll(t.map((x) => x.name)))
      .catch(() => setAll([]));
  }, []);

  const add = async (name: string) => {
    const n = name.trim();
    if (!n) return;
    await invoke("tag_add", { ids: [id], name: n });
    setText("");
    reload();
    onChanged?.();
  };

  const remove = async (tagId: number) => {
    await invoke("tag_remove", { ids: [id], tagId });
    reload();
    onChanged?.();
  };

  // 이미 붙은 것은 뺀 추천 — 앞 글자가 맞는 것만
  const hint = text.trim()
    ? all
        .filter(
          (n) =>
            !mine.some((m) => m.name === n) &&
            n.toLowerCase().startsWith(text.trim().toLowerCase()),
        )
        .slice(0, 5)
    : [];

  return (
    <>
      <div className="text-[10.5px] text-fg-mute uppercase tracking-wider mb-2">
        태그
      </div>
      {mine.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-2">
          {mine.map((t) => (
            <span
              key={t.id}
              className="group flex items-center gap-1 h-5 pl-1.5 pr-1 rounded bg-chrome text-[11px] text-fg-dim"
            >
              {t.name}
              <button
                onClick={() => remove(t.id)}
                title="떼기"
                className="text-fg-faint hover:text-drop"
              >
                ✕
              </button>
            </span>
          ))}
        </div>
      )}
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          // 뷰어의 단축키(0–5, P, X…)로 새지 않게 여기서 막는다
          e.stopPropagation();
          if (e.key === "Enter") add(text);
        }}
        placeholder="태그 붙이기"
        className="w-full h-6 px-1.5 rounded bg-canvas text-[11.5px] text-fg
          placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
      />
      {hint.length > 0 && (
        <div className="flex flex-wrap gap-1 mt-1">
          {hint.map((n) => (
            <button
              key={n}
              onClick={() => add(n)}
              className="h-5 px-1.5 rounded bg-chrome text-[11px] text-fg-mute hover:text-fg"
            >
              {n}
            </button>
          ))}
        </div>
      )}
    </>
  );
}

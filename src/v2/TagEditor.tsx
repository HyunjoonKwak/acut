import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "./toastStore";

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
  const [got, setGot] = useState<{
    fileId: number;
    tags: { id: number; name: string }[];
  } | null>(null);
  const [all, setAll] = useState<string[]>([]);
  const [draft, setDraft] = useState<{ fileId: number; text: string } | null>(
    null,
  );
  const currentId = useRef(id);
  useEffect(() => {
    currentId.current = id;
  }, [id]);
  const mine = got?.fileId === id ? got.tags : [];
  const text = draft?.fileId === id ? draft.text : "";

  const reload = useCallback(async () => {
    const requestedId = id;
    const tags = await invoke<{ id: number; name: string }[]>("tags_of", {
      id: requestedId,
    });
    if (currentId.current === requestedId) {
      setGot({ fileId: requestedId, tags });
    }
  }, [id]);
  useEffect(() => {
    let live = true;
    invoke<{ id: number; name: string }[]>("tags_of", { id })
      .then((tags) => live && setGot({ fileId: id, tags }))
      .catch(() => live && setGot({ fileId: id, tags: [] }));
    return () => {
      live = false;
    };
  }, [id]);
  useEffect(() => {
    invoke<{ name: string }[]>("tags_list")
      .then((t) => setAll(t.map((x) => x.name)))
      .catch(() => setAll([]));
  }, []);

  const add = async (name: string) => {
    const n = name.trim();
    if (!n) return;
    try {
      await invoke("tag_add", { ids: [id], name: n });
      setDraft({ fileId: id, text: "" });
      await reload();
      onChanged?.();
    } catch (e) {
      toast(String(e), "drop");
    }
  };

  const remove = async (tagId: number) => {
    try {
      await invoke("tag_remove", { ids: [id], tagId });
      await reload();
      onChanged?.();
    } catch (e) {
      toast(String(e), "drop");
    }
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
      <div className="text-[11.5px] text-fg-mute uppercase tracking-wider mb-2">
        태그
      </div>
      {mine.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-2">
          {mine.map((t) => (
            <span
              key={t.id}
              className="group flex items-center gap-1 h-5 pl-1.5 pr-1 rounded bg-chrome text-[12px] text-fg-dim"
            >
              {t.name}
              <button
                onClick={() => remove(t.id)}
                title="떼기"
                aria-label={`${t.name} 태그 떼기`}
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
        onChange={(e) => setDraft({ fileId: id, text: e.target.value })}
        onKeyDown={(e) => {
          // 뷰어의 단축키(0–5, P, X…)로 새지 않게 여기서 막는다
          e.stopPropagation();
          if (e.key === "Enter") add(text);
        }}
        placeholder="태그 붙이기"
        className="w-full h-6 px-1.5 rounded bg-canvas text-[12.5px] text-fg
          placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
      />
      {hint.length > 0 && (
        <div className="flex flex-wrap gap-1 mt-1">
          {hint.map((n) => (
            <button
              key={n}
              onClick={() => add(n)}
              className="h-5 px-1.5 rounded bg-chrome text-[12px] text-fg-mute hover:text-fg"
            >
              {n}
            </button>
          ))}
        </div>
      )}
    </>
  );
}

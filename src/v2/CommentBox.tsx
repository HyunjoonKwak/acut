import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "./toastStore";

/**
 * 한 장의 코멘트 — 인스펙터 안. 초점을 잃을 때 저장한다.
 *
 * 글자마다 저장하지 않는다. 사진을 넘길 때(초점이 빠질 때) 한 번이면 된다.
 * 키는 뷰어로 새지 않게 막는다 — «p»를 치면 남김 판정이 찍힌다.
 */
export default function CommentBox({
  id,
  initial,
  onSaved,
}: {
  id: number;
  initial: string;
  onSaved?: (text: string) => void;
}) {
  const [text, setText] = useState(initial);
  const [saved, setSaved] = useState(initial);

  const save = async () => {
    const t = text.trim();
    if (t === saved) return;
    try {
      await invoke("file_comment", { id, text: t });
      setSaved(t);
      onSaved?.(t);
    } catch (e) {
      toast(String(e), "drop");
    }
  };

  return (
    <div className="pt-2">
      <div className="text-[10.5px] text-fg-mute uppercase tracking-wider mb-1">
        코멘트
      </div>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => e.stopPropagation()}
        placeholder="이 사진에 대해…"
        rows={2}
        aria-label="코멘트"
        className="w-full px-1.5 py-1 rounded bg-canvas text-[11.5px] text-fg leading-snug resize-y
          placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
      />
    </div>
  );
}

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Btn } from "./ui";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";

import { personLabel, type Person } from "./peopleItems";

const url = (thumb: string) =>
  `thumb://localhost/${thumb.split("/").map(encodeURIComponent).join("/")}`;

/**
 * 얼굴 한 칸 — 썸네일에서 얼굴 자리만 오려 보인다.
 *
 * 그림의 실제 크기를 알아야 비율이 안 깨진다. 실려 온 뒤에 잰다.
 */
export function FaceCrop({
  person,
  size = 40,
}: {
  person: Person;
  size?: number;
}) {
  const [dims, setDims] = useState<{ w: number; h: number } | null>(null);
  const box = person.cover_bbox;
  if (!person.cover_thumb || !box) {
    return (
      <div
        className="rounded-full bg-raised text-fg-faint flex items-center justify-center text-[10px]"
        style={{ width: size, height: size }}
      >
        ?
      </div>
    );
  }
  let style: React.CSSProperties = { visibility: "hidden" };
  if (dims) {
    // 얼굴 상자를 1.7배 넓게 잡아 칸에 채운다 — 머리와 턱이 들어온다
    const fw = box.w * dims.w;
    const fh = box.h * dims.h;
    const scale = size / (Math.max(fw, fh) * 1.7);
    const cx = (box.x + box.w / 2) * dims.w * scale;
    const cy = (box.y + box.h / 2) * dims.h * scale;
    style = {
      width: dims.w * scale,
      height: dims.h * scale,
      left: size / 2 - cx,
      top: size / 2 - cy,
      maxWidth: "none",
    };
  }
  return (
    <div
      className="relative rounded-full overflow-hidden bg-raised shrink-0"
      style={{ width: size, height: size }}
    >
      <img
        src={url(person.cover_thumb)}
        onLoad={(e) =>
          setDims({
            w: e.currentTarget.naturalWidth,
            h: e.currentTarget.naturalHeight,
          })
        }
        className="absolute"
        style={style}
      />
    </div>
  );
}

/**
 * 사람 갈래 — 얼굴로 묶은 사람들. 이름은 사용자가 붙인다.
 *
 * 묶기는 틀릴 수 있다. 같은 사람이 둘로 갈리면 「합치기」, 이름은 두 번
 * 눌러 바꾼다. 얼굴 하나짜리는 대개 지나가는 사람이라 접어 둔다.
 */
export default function PeoplePanel({
  selected,
  onPick,
}: {
  selected: number | null;
  onPick: (id: number | null) => void;
}) {
  const ask = useConfirm();
  const [people, setPeople] = useState<Person[]>([]);
  const [showSingles, setShowSingles] = useState(false);
  /** 이름 고치는 중 — 누구의, 무엇으로 */
  const [editing, setEditing] = useState<{ id: number; text: string } | null>(
    null,
  );
  /** 합칠 상대를 고르는 중 — 어느 사람을 (from) */
  const [merging, setMerging] = useState<number | null>(null);

  const reload = useCallback(() => {
    invoke<Person[]>("people_list")
      .then(setPeople)
      .catch(() => setPeople([]));
  }, []);
  useEffect(reload, [reload]);
  useEffect(() => {
    let un = () => {};
    listen("faces-done", reload).then((f) => {
      un = f;
    });
    return () => un();
  }, [reload]);

  const start = async () => {
    try {
      await invoke("ai_faces_start");
    } catch (e) {
      toast(String(e), "drop");
    }
  };

  const rename = async () => {
    if (!editing) return;
    await invoke("person_rename", { id: editing.id, name: editing.text });
    setEditing(null);
    reload();
  };

  const merge = async (into: number) => {
    const from = merging;
    setMerging(null);
    if (from === null || from === into) return;
    const a = people.find((p) => p.id === from);
    const b = people.find((p) => p.id === into);
    if (!a || !b) return;
    const ok = await ask({
      title: `「${personLabel(a)}」을 「${personLabel(b)}」에 합칠까요?`,
      lines: [
        `${a.count}장의 얼굴이 옮겨 가고 「${personLabel(a)}」은 사라집니다.`,
      ],
      confirmLabel: "합치기",
    });
    if (!ok) return;
    await invoke("person_merge", { into, from });
    if (selected === from) onPick(into);
    reload();
  };

  const many = people.filter((p) => p.count > 1);
  const singles = people.filter((p) => p.count <= 1);
  const shown = showSingles ? people : many;

  return (
    <>
      <div className="px-2 pb-2 flex items-center gap-2">
        <Btn onClick={start} hint="썸네일에서 얼굴을 찾아 사람으로 묶습니다">
          얼굴 찾기
        </Btn>
        <span className="text-[11px] text-fg-mute tabular-nums">
          {people.length > 0 && `${people.length}명`}
        </span>
      </div>

      {merging !== null && (
        <div className="mx-2 mb-2 px-2 py-1.5 rounded-md bg-raised text-[11.5px] text-fg-dim flex items-center gap-2">
          <span className="flex-1">합칠 사람을 누르세요</span>
          <button className="text-fg-mute" onClick={() => setMerging(null)}>
            취소
          </button>
        </div>
      )}

      {people.length === 0 && (
        <div className="px-3 py-2 text-[12px] text-fg-mute leading-relaxed">
          아직 사람이 없습니다.
          <br />
          「얼굴 찾기」를 누르면 썸네일에서 얼굴을 찾아 사람으로 묶습니다. 얼굴
          모델은 설정 › AI에서 받습니다.
        </div>
      )}

      <ul>
        {shown.map((p) => {
          const on = selected === p.id;
          return (
            <li key={p.id} className="group">
              <div
                className={`mx-1 px-1.5 h-12 rounded-md flex items-center gap-2 cursor-default
                  ${on ? "bg-accent/20 text-fg" : "text-fg-dim hover:bg-hover"}
                  ${merging !== null && merging !== p.id ? "ring-1 ring-accent/40" : ""}`}
                onClick={() =>
                  merging !== null ? merge(p.id) : onPick(on ? null : p.id)
                }
              >
                <FaceCrop person={p} />
                {editing?.id === p.id ? (
                  <input
                    autoFocus
                    value={editing.text}
                    onChange={(e) =>
                      setEditing({ id: p.id, text: e.target.value })
                    }
                    onKeyDown={(e) => {
                      if (e.key === "Enter") rename();
                      if (e.key === "Escape") setEditing(null);
                    }}
                    onBlur={rename}
                    onClick={(e) => e.stopPropagation()}
                    placeholder="이름"
                    className="flex-1 min-w-0 h-7 px-1.5 rounded bg-canvas text-[12px] text-fg outline-none ring-1 ring-accent"
                  />
                ) : (
                  <span
                    className={`flex-1 min-w-0 truncate text-[12.5px] ${p.name ? "" : "text-fg-mute italic"}`}
                    onDoubleClick={(e) => {
                      e.stopPropagation();
                      setEditing({ id: p.id, text: p.name ?? "" });
                    }}
                    title="두 번 누르면 이름을 바꿉니다"
                  >
                    {personLabel(p)}
                  </span>
                )}
                <span className="text-[11px] text-fg-mute tabular-nums">
                  {p.count}
                </span>
                <button
                  className="opacity-0 group-hover:opacity-100 text-[11px] text-fg-mute hover:text-fg px-1"
                  title="다른 사람에 합치기"
                  onClick={(e) => {
                    e.stopPropagation();
                    setMerging(merging === p.id ? null : p.id);
                  }}
                >
                  합치기
                </button>
              </div>
            </li>
          );
        })}
      </ul>

      {singles.length > 0 && (
        <button
          className="mx-3 my-2 text-[11px] text-fg-mute hover:text-fg"
          onClick={() => setShowSingles((v) => !v)}
        >
          {showSingles
            ? "얼굴 하나짜리 접기"
            : `얼굴 하나짜리 ${singles.length}명 보기`}
        </button>
      )}
    </>
  );
}

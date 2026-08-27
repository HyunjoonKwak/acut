import { useEffect } from "react";
import { EMPTY, isEmpty, type Picks } from "./picks";
import { useDebouncedText } from "./useDebouncedText";
import { Btn, Menu } from "./ui";

const KINDS = [
  { v: 0, label: "사진" },
  { v: 1, label: "영상" },
  { v: 2, label: "RAW" },
];
const FLAGS: { v: number; label: string; tone: "keep" | "drop" | undefined }[] =
  [
    { v: 1, label: "남김", tone: "keep" },
    { v: 2, label: "제외", tone: "drop" },
    { v: 0, label: "미판정", tone: undefined },
  ];

/**
 * 찾기 — 툴바의 버튼 하나와 그 안의 팝오버.
 *
 * 예전에는 툴바 아래 상시 줄이었다. 필터를 안 쓸 때도 사진 한 줄만큼을
 * 차지했고, 정렬·묶기·보기가 같은 줄에 끼면서 무엇이 무엇인지 흐려졌다.
 * 지금은 걸린 조건이 있을 때만 버튼에 표시가 붙는다.
 */
export default function FilterButton({
  value,
  onChange,
}: {
  value: Picks;
  onChange: (p: Picks) => void;
}) {
  const set = (patch: Partial<Picks>) => onChange({ ...value, ...patch });

  // 한 글자마다 14만 행을 훑지 않게 타이핑이 멎기를 기다린다
  const [text, setText] = useDebouncedText(value.name_like ?? "", 250, (t) => {
    const next = t.trim() || null;
    if (next !== (value.name_like ?? null)) onChange({ ...value, name_like: next });
  });

  useEffect(() => {
    if (value.name_like === null) setText((t) => (t.trim() === "" ? t : ""));
  }, [value.name_like]);

  const on = !isEmpty(value);

  return (
    <Menu
      align="right"
      width={250}
      trigger={() => (
        <Btn active={on} title="찾기">
          <span className={on ? "text-accent" : undefined}>⌕</span>
          찾기
          {on && <span className="w-1.5 h-1.5 rounded-full bg-accent" />}
        </Btn>
      )}
    >
      {(close) => (
        <div className="px-3 py-2 flex flex-col gap-3">
          <input
            value={text}
            autoFocus
            onChange={(e) => setText(e.target.value)}
            placeholder="파일명"
            className="h-control px-2 rounded-md bg-canvas text-[12.5px] text-fg
              placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
          />

          <Group label="종류">
            {KINDS.map((k) => (
              <Chip
                key={k.v}
                on={value.kind === k.v}
                onClick={() => set({ kind: value.kind === k.v ? null : k.v })}
              >
                {k.label}
              </Chip>
            ))}
          </Group>

          <Group label="판정">
            {FLAGS.map((f) => (
              <Chip
                key={f.v}
                on={value.culling_flag === f.v}
                tone={f.tone}
                onClick={() =>
                  set({ culling_flag: value.culling_flag === f.v ? null : f.v })
                }
              >
                {f.label}
              </Chip>
            ))}
          </Group>

          <Group label="별점 이상">
            <div className="flex items-center gap-0.5">
              {[1, 2, 3, 4, 5].map((n) => (
                <button
                  key={n}
                  onClick={() =>
                    set({ min_rating: value.min_rating === n ? null : n })
                  }
                  className={`w-5 h-6 text-[13px] ${
                    (value.min_rating ?? 0) >= n
                      ? "text-keep"
                      : "text-fg-faint hover:text-fg-mute"
                  }`}
                >
                  ★
                </button>
              ))}
            </div>
            <Chip
              on={value.favorite_only}
              tone="drop"
              onClick={() => set({ favorite_only: !value.favorite_only })}
            >
              ♥ 즐겨찾기
            </Chip>
          </Group>

          {on && (
            <button
              onClick={() => {
                setText("");
                onChange(EMPTY);
                close();
              }}
              className="h-control rounded-md text-[12px] text-fg-dim ring-1 ring-line-strong hover:bg-hover"
            >
              조건 모두 지우기
            </button>
          )}
        </div>
      )}
    </Menu>
  );
}

function Group({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-[10px] uppercase tracking-[0.08em] text-fg-faint">
        {label}
      </span>
      <div className="flex flex-wrap items-center gap-1.5">{children}</div>
    </div>
  );
}

function Chip({
  children,
  on,
  tone,
  onClick,
}: {
  children: React.ReactNode;
  on: boolean;
  tone?: "keep" | "drop";
  onClick: () => void;
}) {
  const active =
    tone === "keep"
      ? "bg-keep text-keep-fg"
      : tone === "drop"
        ? "bg-drop text-drop-fg"
        : "bg-accent text-accent-fg";
  return (
    <button
      onClick={onClick}
      className={`h-6 px-2 rounded text-[11.5px] whitespace-nowrap transition-colors ${
        on ? active : "text-fg-dim ring-1 ring-line-strong hover:text-fg"
      }`}
    >
      {children}
    </button>
  );
}

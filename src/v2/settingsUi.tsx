import { usePref, type Prefs } from "./prefs";

/** 설정 화면의 조각들 — 갈래 파일들이 함께 쓴다 */
export function Section({
  id,
  title,
  children,
}: {
  id: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section id={`settings-${id}`} className="scroll-mt-4">
      <h2 className="text-[10.5px] font-bold uppercase tracking-widest text-fg-mute mb-3">
        {title}
      </h2>
      <div className="rounded-lg bg-chrome ring-1 ring-line divide-y divide-line">
        {children}
      </div>
    </section>
  );
}

/** 한 줄 — 왼쪽에 이름과 설명, 오른쪽에 조작 */
export function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 px-4 py-2.5">
      <div className="flex-1 min-w-0">
        <div className="text-[13px] text-fg">{label}</div>
        {hint && (
          <div className="text-[11.5px] text-fg-mute leading-snug mt-0.5">
            {hint}
          </div>
        )}
      </div>
      <div className="shrink-0 flex items-center gap-2">{children}</div>
    </div>
  );
}

export function Select<K extends keyof Prefs>({
  k,
  options,
}: {
  k: K;
  options: { v: Prefs[K]; label: string }[];
}) {
  const [value, set] = usePref(k);
  return (
    <select
      value={String(value)}
      onChange={(e) => {
        const o = options.find((x) => String(x.v) === e.target.value);
        if (o) set(o.v);
      }}
      aria-label={String(k)}
      className="h-control min-w-[140px] px-2 rounded-md bg-raised text-[12.5px] text-fg ring-1 ring-line outline-none focus:ring-accent"
    >
      {options.map((o) => (
        <option key={String(o.v)} value={String(o.v)}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Toggle({ k }: { k: keyof Prefs }) {
  const [value, set] = usePref(k);
  const on = Boolean(value);
  return (
    <button
      role="switch"
      aria-checked={on}
      aria-label={String(k)}
      onClick={() => (set as (v: boolean) => void)(!on)}
      className={`relative w-9 h-5 rounded-full transition-colors ${on ? "bg-accent" : "bg-line-strong"}`}
    >
      <span
        className={`absolute left-0 top-0.5 w-4 h-4 rounded-full bg-white transition-transform ${
          on ? "translate-x-[18px]" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}

// ── 갈래들 ──────────────────────────────────────────────────────────────


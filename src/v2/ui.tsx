import {
  forwardRef,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { useUi } from "./uiStore";

/**
 * 화면 부품 — 버튼·메뉴·구분선.
 *
 * 이걸 만든 이유: 기능을 하나씩 붙이다 보니 버튼 높이가 8종류, 색이 42가지가
 * 됐다. 줄이 안 맞고 눌러도 되는 것과 안 되는 것이 구분되지 않았다.
 * 크기와 색을 여기서만 정한다.
 */

type Tone = "plain" | "accent" | "keep" | "drop";

const TONE: Record<Tone, string> = {
  plain: "text-fg-dim ring-1 ring-line-strong hover:text-fg hover:bg-hover",
  accent: "bg-accent text-accent-fg font-semibold hover:brightness-110",
  keep: "bg-keep text-keep-fg font-semibold hover:brightness-110",
  drop: "text-drop ring-1 ring-drop/40 hover:bg-drop/10",
};

type BtnProps = React.ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: Tone;
  /** 단축키 표시 */
  hint?: string;
  /** 켜져 있는 상태 (보기 방식처럼 토글되는 것) */
  active?: boolean;
};

export const Btn = forwardRef<HTMLButtonElement, BtnProps>(function Btn(
  { children, tone = "plain", hint, active, className = "", ...button },
  ref,
) {
  return (
    <button
      ref={ref}
      {...button}
      className={`h-control px-2.5 rounded-md text-[13.5px] whitespace-nowrap
        inline-flex items-center gap-1.5 transition-colors
        disabled:opacity-35 disabled:pointer-events-none
        ${active && tone === "plain" ? "bg-hover text-fg ring-1 ring-line-strong" : TONE[tone]} ${className}`}
    >
      {children}
      {hint && <Kbd>{hint}</Kbd>}
    </button>
  );
});

/** 아이콘만 있는 정사각 버튼 */
export function IconBtn({
  children,
  onClick,
  title,
  active,
  disabled,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  title?: string;
  active?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`h-control w-control rounded-md text-[14px] inline-flex items-center justify-center
        transition-colors disabled:opacity-35 disabled:pointer-events-none
        ${active ? "bg-hover text-accent" : "text-fg-dim hover:text-fg hover:bg-hover"}`}
    >
      {children}
    </button>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <span className="text-[11px] font-mono text-fg-faint leading-none">
      {children}
    </span>
  );
}

export function Sep() {
  return <span className="w-px h-4 bg-line shrink-0" />;
}

/** 목록 위의 작은 제목 */
export function Label({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-3 pt-2 pb-1 text-[11px] uppercase tracking-[0.08em] text-fg-faint">
      {children}
    </div>
  );
}

/**
 * 눌러서 여는 메뉴.
 *
 * 바깥을 누르거나 Esc면 닫힌다. 열린 채 남으면 사진을 가린다.
 */
export function Menu({
  trigger,
  children,
  align = "left",
  width,
  up = false,
}: {
  trigger: (
    open: boolean,
    props: React.ButtonHTMLAttributes<HTMLButtonElement> & {
      ref: React.Ref<HTMLButtonElement>;
    },
  ) => React.ReactNode;
  children: (close: () => void) => React.ReactNode;
  align?: "left" | "right";
  width?: number;
  /** 상태바처럼 화면 아래 붙은 자리에선 위로 연다 */
  up?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuId = useId();

  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) close();
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close();
    };
    const trigger = triggerRef.current;
    useUi.getState().set({ menuOpen: true });
    const frame = requestAnimationFrame(() => {
      const first = menuRef.current?.querySelector<HTMLElement>(
        "input, [role='menuitem']:not(:disabled)",
      );
      first?.focus();
    });
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => {
      cancelAnimationFrame(frame);
      useUi.getState().set({ menuOpen: false });
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
      requestAnimationFrame(() => trigger?.focus());
    };
  }, [open, close]);

  const moveInMenu = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
    const items = Array.from(
      menuRef.current?.querySelectorAll<HTMLElement>(
        "[role='menuitem']:not(:disabled)",
      ) ?? [],
    );
    if (items.length === 0) return;
    e.preventDefault();
    e.stopPropagation();
    const i = items.indexOf(document.activeElement as HTMLElement);
    const next =
      e.key === "Home"
        ? 0
        : e.key === "End"
          ? items.length - 1
          : e.key === "ArrowDown"
            ? (i + 1 + items.length) % items.length
            : (i - 1 + items.length) % items.length;
    items[next]?.focus();
  };

  return (
    <div ref={box} className="relative">
      {trigger(open, {
        ref: triggerRef,
        "aria-haspopup": "menu",
        "aria-expanded": open,
        "aria-controls": open ? menuId : undefined,
        onClick: () => (open ? close() : setOpen(true)),
        onKeyDown: (e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setOpen(true);
          }
        },
      })}
      {open && (
        <div
          ref={menuRef}
          id={menuId}
          role="menu"
          onKeyDown={moveInMenu}
          className={`absolute ${up ? "bottom-full mb-1.5" : "top-9"} z-40 bg-raised rounded-lg ring-1 ring-line-strong
            shadow-2xl shadow-black/50 py-1 ${align === "right" ? "right-0" : "left-0"}`}
          style={{ minWidth: width ?? 150 }}
        >
          {children(close)}
        </div>
      )}
    </div>
  );
}

/** 메뉴 안의 한 줄 */
export function MenuItem({
  children,
  onClick,
  selected,
  danger,
  hint,
}: {
  children: React.ReactNode;
  onClick: () => void;
  selected?: boolean;
  danger?: boolean;
  hint?: string;
}) {
  return (
    <button
      role="menuitem"
      onClick={onClick}
      className={`flex w-full items-center gap-3 px-3 py-1.5 text-[13.5px] text-left
        hover:bg-hover transition-colors
        ${danger ? "text-drop" : selected ? "text-accent" : "text-fg-dim"}`}
    >
      <span className="flex-1 whitespace-nowrap">{children}</span>
      {hint && <Kbd>{hint}</Kbd>}
    </button>
  );
}

export function MenuSep() {
  return <div role="separator" className="h-px bg-line my-1" />;
}

/**
 * 사이드바의 한 줄 — 이름과 (있으면) 장수.
 *
 * 「모든」 갈래처럼 목록이 아니라 손에 익은 몇 가지를 늘어놓는 자리에 쓴다.
 */
export function QuickRow({
  label,
  count,
  on,
  onClick,
}: {
  label: string;
  count?: number;
  on: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`w-full flex items-center gap-2 px-3 py-1.5 text-[13.5px] ${
        on ? "bg-raised text-fg" : "text-fg-dim hover:text-fg hover:bg-chrome"
      }`}
    >
      <span className="flex-1 text-left truncate">{label}</span>
      {count !== undefined && (
        <span className="text-fg-mute tabular-nums text-[12px]">
          {count.toLocaleString()}
        </span>
      )}
    </button>
  );
}

import { SECTIONS } from "./settingsItems";

/** 설정의 왼쪽 목록 — 누르면 본문의 그 갈래로 간다 */
export default function SettingsNav() {
  return (
    <>
      {SECTIONS.map((s) => (
        <button
          key={s.id}
          onClick={() =>
            document
              .getElementById(`settings-${s.id}`)
              ?.scrollIntoView({ behavior: "smooth", block: "start" })
          }
          className="w-full text-left px-3 py-1.5 text-[13.5px] text-fg-dim hover:text-fg hover:bg-chrome"
        >
          {s.label}
        </button>
      ))}
    </>
  );
}

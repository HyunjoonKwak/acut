import { useData } from "./dataStore";
import { IconNas } from "./icons";
import { usePref, usePrefs } from "./prefs";
import { fmtDateTime } from "./format";

/**
 * 툴바의 NAS 불 — 앱이 살핀 결과를 한눈에.
 *
 * 초록: 연결됨 · 붉음: 살폈는데 실패 · 흐림: 아직 안 살폈거나 «안 함».
 * 누르면 설정 › NAS로 간다.
 */
export default function NasBadge() {
  const st = useData((s) => s.nasStatus);
  const mode = usePrefs((s) => s.nasAuto);
  const [, setSource] = usePref("source");
  const color =
    mode === "off" || !st
      ? "var(--color-fg-faint)"
      : st.online
        ? "var(--color-ok)"
        : "var(--color-drop)";
  const title =
    mode === "off"
      ? "NAS — 앱을 열 때 살피지 않음 (설정 › NAS)"
      : !st
        ? "NAS — 살피는 중…"
        : st.online
          ? `NAS 연결됨 — ${st.hostname} (${fmtDateTime(st.at)})`
          : `NAS 연결 실패 — ${st.error ?? ""} (${fmtDateTime(st.at)})`;
  return (
    <button
      onClick={() => {
        setSource("settings");
        window.setTimeout(
          () =>
            document
              .getElementById("settings-nas")
              ?.scrollIntoView({ behavior: "smooth", block: "start" }),
          50,
        );
      }}
      title={title}
      className="relative h-control w-7 flex items-center justify-center rounded-md text-fg-mute hover:text-fg hover:bg-hover shrink-0"
    >
      <IconNas className="w-4 h-4" />
      <span
        className="absolute right-1 bottom-1 w-2 h-2 rounded-full ring-2 ring-chrome"
        style={{ background: color }}
      />
    </button>
  );
}

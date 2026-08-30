import { General, Library, Browse, ViewerSection } from "./SettingsGeneral";
import { Ai } from "./SettingsAi";
import { Database } from "./SettingsDatabase";
import { Backup } from "./SettingsBackup";
import { Nas } from "./SettingsNas";
import { Advanced, About } from "./SettingsMisc";


/**
 * 설정 — 본문을 통째로 쓴다 (Lap의 Settings.vue).
 *
 * 사이드바의 좁은 패널에 늘어놓다 보니 스크롤이 길어졌다. 왼쪽 목록에서
 * 갈래를 누르면 그 자리로 간다. 값은 전부 prefs에 — 켰다 꺼도 남는다.
 */
export default function SettingsView({
  onRescanAll,
}: {
  onRescanAll: () => void;
}) {
  return (
    <div className="flex-1 min-w-0 overflow-y-auto">
      <div className="max-w-[720px] mx-auto px-6 py-6 space-y-10">
        <General />
        <Library onRescanAll={onRescanAll} />
        <Browse />
        <ViewerSection />
        <Ai />
        <Database />
        <Backup />
        <Nas />
        <Advanced />
        <About />
      </div>
    </div>
  );
}

// ── 조각들 ──────────────────────────────────────────────────────────────


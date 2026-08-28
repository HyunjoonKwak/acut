import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { useJob } from "./jobStore";
import { usePrefs } from "./prefs";
import { toast } from "./toastStore";

export type Probe = {
  online: boolean;
  hostname: string;
  library_id: number | null;
  new_files: number;
  new_bytes: number;
  error: string | null;
};

/** 앱을 연 뒤 이만큼 있다가 처음 살핀다 — 첫 화면을 막지 않게 */
const FIRST_MS = 5_000;
/** 켜 둔 동안 이 간격으로 다시 — 폰은 하루 종일 올린다 */
const EVERY_MS = 30 * 60_000;

/**
 * NAS 1차 구역 살피기 — 앱을 열 때와 30분마다.
 *
 * NAS가 꺼져 있으면 아무 말도 않는다(로컬은 무영향). 받은 적 없는 사진이
 * 있으면 상태바에 «NAS 1차 새 사진 N장»을 띄우고, 설정이 «저절로»면 다른
 * 작업이 없을 때 바로 내려받는다.
 */
export function useNasAuto() {
  const mode = usePrefs((s) => s.nasAuto);
  useEffect(() => {
    if (mode === "off") return;
    let live = true;
    const probe = async () => {
      try {
        const p = await invoke<Probe>("nas_probe");
        if (!live || !p.online || p.library_id === null) return;
        if (p.new_files === 0) {
          useData.getState().setNasNew(null);
          return;
        }
        useData.getState().setNasNew({
          libraryId: p.library_id,
          files: p.new_files,
          bytes: p.new_bytes,
        });
        if (mode === "pull" && useJob.getState().job === null) {
          await invoke("nas_pull_start", { libraryId: p.library_id });
          toast(
            `NAS 1차에 새 사진 ${p.new_files.toLocaleString()}장 — 내려받습니다`,
            "ok",
          );
        }
      } catch {
        /* NAS가 없거나 꺼져 있다 — 조용히 */
      }
    };
    const t0 = window.setTimeout(probe, FIRST_MS);
    const t = window.setInterval(probe, EVERY_MS);
    return () => {
      live = false;
      window.clearTimeout(t0);
      window.clearInterval(t);
    };
  }, [mode]);
}

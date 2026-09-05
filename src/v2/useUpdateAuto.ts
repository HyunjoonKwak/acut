import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "./toastStore";

/** 새 판이 있는지 살피는 것 — src-tauri/src/api/update.rs 와 짝이다 */
type UpdateInfo = { current: string; latest: string; newer: boolean };

/** 첫 화면을 막지 않게 이만큼 있다가 살핀다 */
const DELAY_MS = 8_000;

/**
 * 앱을 열 때 새 판이 있는지 조용히 한 번 살핀다.
 *
 * 실제로 바깥에 묻는 것은 **하루 한 번**이고(백엔드가 마지막 시각을 기억한다),
 * 설정에서 끌 수 있다. 새 판이 있을 때만 한 줄 알리고, 인터넷이 없으면 아무
 * 말도 하지 않는다 — 오프라인이 기본인 앱이 연결을 잔소리하면 안 된다.
 */
export function useUpdateAuto() {
  useEffect(() => {
    let live = true;
    const timer = setTimeout(() => {
      invoke<UpdateInfo | null>("update_check_auto")
        .then((info) => {
          if (!live || !info?.newer) return;
          toast(
            `${info.latest} 이 나왔습니다 — 설정 › 정보에서 받으세요 (지금 ${info.current})`,
          );
        })
        .catch(() => {
          /* 살피지 못한 것은 알릴 일이 아니다 */
        });
    }, DELAY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, []);
}

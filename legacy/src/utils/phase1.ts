import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "@/stores/appStore";
import { toast } from "@/stores/toastStore";
import type { MediaStats } from "@/types";

// Phase 1 (EXIF + dimensions + thumbnails) runs as a frontend-driven batch
// loop so gallery queries stay responsive between batches. Shared by the
// sidebar (after a scan) and app startup (resume unprocessed files).
export function runPhase1Loop() {
  const store = useAppStore.getState();
  if (store.phase1Running) return;

  store.setPhase1Progress(0, 0, true);

  const refreshStats = async () => {
    const stats = await invoke<MediaStats>("get_media_stats");
    useAppStore.getState().setStats(stats);
    useAppStore.getState().triggerRefresh();
  };

  const run = async () => {
    try {
      const [processed, done, total] =
        await invoke<[number, number, number]>("process_phase1");

      useAppStore.getState().setPhase1Progress(done, total, processed > 0);

      // Refresh gallery every 5 batches (not every batch, to reduce flicker)
      if (processed > 0 && done % 250 < 50) {
        await refreshStats();
      }

      if (processed > 0) {
        setTimeout(run, 300);
      } else {
        await refreshStats();
        useAppStore.getState().setPhase1Progress(total, total, false);
      }
    } catch (e) {
      useAppStore.getState().setPhase1Progress(0, 0, false);
      toast.error(`미디어 처리 중 오류가 발생했습니다: ${e}`);
    }
  };
  run();
}

// On startup, resume Phase 1 if unprocessed files remain (e.g. interrupted
// scan or thumbnail-format migration reset)
export async function resumePhase1IfPending() {
  if (useAppStore.getState().phase1Running) return;
  try {
    const [processed] = await invoke<[number, number, number]>("process_phase1");
    if (processed > 0) {
      runPhase1Loop();
    }
  } catch {
    // DB not ready yet; a manual scan will pick things up
  }
}

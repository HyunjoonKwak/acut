import { invoke } from "@tauri-apps/api/core";
import { usePrefs } from "./prefs";
import { useView } from "./viewStore";
import type { FolderRow } from "./types";

/**
 * 시작 주소의 `?sel=<라이브러리 id>|<라이브러리 기준 경로>` 로 폴더를 바로 연다.
 *
 * 재현·자동 시험용이다 — 특정 폴더에서만 나는 문제를 사람이 클릭하지 않고
 * 다시 만들 수 있다. 릴리스 앱의 주소에는 물음표가 없으니 아무것도 안 한다.
 */
export async function applyStartupSel(): Promise<void> {
  const q = new URLSearchParams(location.search).get("sel");
  if (!q) return;
  const bar = q.indexOf("|");
  if (bar < 0) return;
  const libId = Number(q.slice(0, bar));
  const path = q.slice(bar + 1);
  if (!Number.isFinite(libId)) return;
  try {
    const folders = await invoke<FolderRow[]>("folders_list", {
      libraryId: libId,
    });
    const f = folders.find((r) => r.library_id === libId && r.path === path);
    if (!f) return;
    usePrefs.getState().set("libId", libId);
    const view = useView.getState();
    view.setSel({ libId, path: f.path, rel: f.rel_path });
    view.setViewTrash(false);
  } catch (e) {
    console.warn("startup sel failed", e);
  }
}

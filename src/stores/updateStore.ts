import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface UpdateInfo {
  current_version: string;
  latest_version: string;
  update_available: boolean;
  release_notes: string;
  release_url: string;
  published_at: string | null;
  asset_name: string | null;
  asset_url: string | null;
  asset_size: number | null;
}

export interface UpdateDownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
}

interface UpdateState {
  info: UpdateInfo | null;
  checking: boolean;
  checkError: string | null;
  downloading: boolean;
  progress: UpdateDownloadProgress | null;
  downloadedPath: string | null;
  downloadError: string | null;
  checkForUpdate: () => Promise<UpdateInfo | null>;
  downloadUpdate: () => Promise<void>;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  info: null,
  checking: false,
  checkError: null,
  downloading: false,
  progress: null,
  downloadedPath: null,
  downloadError: null,

  checkForUpdate: async () => {
    if (get().checking) return get().info;
    set({ checking: true, checkError: null });
    try {
      const info = await invoke<UpdateInfo>("check_for_update");
      set({ info, checking: false });
      return info;
    } catch (error) {
      set({ checking: false, checkError: String(error) });
      return null;
    }
  },

  downloadUpdate: async () => {
    const { info, downloading } = get();
    if (downloading || !info?.asset_url || !info.asset_name) return;
    set({
      downloading: true,
      downloadError: null,
      progress: null,
      downloadedPath: null,
    });
    const unlisten = await listen<UpdateDownloadProgress>(
      "update-download-progress",
      (e) => set({ progress: e.payload })
    );
    try {
      const path = await invoke<string>("download_update", {
        assetUrl: info.asset_url,
        assetName: info.asset_name,
      });
      set({ downloading: false, downloadedPath: path });
    } catch (error) {
      set({ downloading: false, downloadError: String(error) });
    } finally {
      unlisten();
    }
  },
}));

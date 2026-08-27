import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/appStore";
import type { ScanProgress, SyncProgress, WatchEvent } from "@/types";

export function useScanProgress() {
  const setScanProgress = useAppStore((s) => s.setScanProgress);
  const setIsScanning = useAppStore((s) => s.setIsScanning);

  useEffect(() => {
    const unlisten = listen<ScanProgress>("scan-progress", (event) => {
      setScanProgress(event.payload);
      const phase = event.payload.phase;
      if (phase === "complete" || phase === "cancelled") {
        setIsScanning(false);
      }
      // Phase 0 complete: file list is ready, background processing continues
      if (phase === "phase0_complete") {
        setIsScanning(false);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setScanProgress, setIsScanning]);
}

export function useDuplicateProgress() {
  const [progress, setProgress] = useState<{
    phase: string;
    total: number;
    current: number;
    detail: string;
  } | null>(null);

  useEffect(() => {
    const unlisten = listen("duplicate-progress", (event) => {
      setProgress(event.payload as any);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return progress;
}

export function useOrganizeProgress() {
  const [progress, setProgress] = useState<{
    phase: string;
    total: number;
    current: number;
    current_file: string;
  } | null>(null);

  useEffect(() => {
    const unlisten = listen("organize-progress", (event) => {
      setProgress(event.payload as any);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return progress;
}

export function useSyncProgress() {
  const [progress, setProgress] = useState<SyncProgress | null>(null);

  useEffect(() => {
    const unlisten = listen<SyncProgress>("sync-progress", (event) => {
      setProgress(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return progress;
}

export function useWatchEvents() {
  const [events, setEvents] = useState<WatchEvent[]>([]);

  useEffect(() => {
    const unlisten = listen<WatchEvent[]>("watch-changes", (event) => {
      setEvents((prev) => [...event.payload, ...prev].slice(0, 100));
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return events;
}

export function useBcutProgress() {
  const [progress, setProgress] = useState<{
    phase: string;
    current: number;
    total: number;
  } | null>(null);

  useEffect(() => {
    const unlisten = listen("bcut-progress", (event) => {
      setProgress(event.payload as any);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return progress;
}

export function useVolumeEvents() {
  const [mounted, setMounted] = useState<string[]>([]);
  const [unmounted, setUnmounted] = useState<string[]>([]);

  useEffect(() => {
    const unlistenMount = listen<string>("volume-mounted", (event) => {
      setMounted((prev) => [...prev, event.payload]);
    });
    const unlistenUnmount = listen<string>("volume-unmounted", (event) => {
      setUnmounted((prev) => [...prev, event.payload]);
    });

    return () => {
      unlistenMount.then((fn) => fn());
      unlistenUnmount.then((fn) => fn());
    };
  }, []);

  return { mounted, unmounted };
}

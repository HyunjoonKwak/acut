import type { AppView } from "@/types";

/**
 * Top-level areas shown in the nav rail, ordered by workflow:
 * bring in -> pick -> organize -> upload.
 */
export type AppArea =
  | "workbench"
  | "library"
  | "select"
  | "organize"
  | "nas"
  | "settings";

const VIEW_AREA: Record<AppView, AppArea> = {
  dashboard: "workbench",
  cleanup: "workbench",
  gallery: "library",
  map: "library",
  tags: "library",
  albums: "library",
  duplicates: "select",
  bcut: "select",
  review: "select",
  organize: "organize",
  "before-after": "organize",
  foldertree: "organize",
  sync: "organize",
  history: "organize",
  nas: "nas",
  settings: "settings",
};

export function viewToArea(view: AppView): AppArea {
  return VIEW_AREA[view] ?? "workbench";
}

/** Default landing view when an area is entered for the first time. */
export const AREA_DEFAULT_VIEW: Record<AppArea, AppView> = {
  workbench: "dashboard",
  library: "gallery",
  select: "duplicates",
  organize: "organize",
  nas: "nas",
  settings: "settings",
};

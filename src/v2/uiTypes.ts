export type MenuAt = { x: number; y: number } | null;

export type FolderAction = "create" | "rename" | "move" | "copy" | "trash";

export type FolderOperationTarget = {
  action: FolderAction;
  sourceLibraryId: number;
  sourceDir: string;
  sourceName: string;
};

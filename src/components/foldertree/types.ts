export interface TreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: string;
  file_type: string;
  children: TreeNode[];
  file_count: number;
  folder_count: number;
}

export type FileTypeFilter = "all" | "image" | "video" | "audio" | "document" | "archive" | "other";

export interface FileOpResult {
  success: number;
  failed: number;
  errors: string[];
}

export interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: string;
}

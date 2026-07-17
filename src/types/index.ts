export interface MediaFile {
  id: string;
  file_path: string;
  original_path: string;
  file_name: string;
  file_size: number;
  mime_type: string | null;
  sha256_hash: string | null;
  quick_hash: string | null;
  width: number | null;
  height: number | null;
  media_type: "image" | "video";
  created_at: string;
  modified_at: string;
  scanned_at: string;
  source_type: "local" | "external" | "cloud";
  thumbnail: string | null;
  scan_phase: number;
  date_taken: string | null;
}

export interface MediaListResponse {
  files: MediaFile[];
  total: number;
  total_size: number;
}

export interface MediaStats {
  total_count: number;
  total_size: number;
  image_count: number;
  video_count: number;
}

export interface ScanProgress {
  total: number;
  current: number;
  current_file: string;
  phase: "scanning" | "processing" | "phase0_complete" | "complete" | "cancelled";
}

export interface ScanResult {
  total_files: number;
  images: number;
  videos: number;
  total_size: number;
}

export interface DuplicateGroup {
  id: string;
  match_type: "exact" | "similar" | "ai_confirmed";
  similarity_score: number | null;
  status: string;
  members: DuplicateMember[];
}

export interface DuplicateMember {
  media_id: string;
  is_preferred: boolean;
  file_path: string;
  file_name: string;
  file_size: number;
}

export interface PlannedMove {
  source: string;
  target: string;
  file_name: string;
  file_size: number;
  reason: string;
}

export interface OrganizePlan {
  moves: PlannedMove[];
  total_files: number;
  total_size: number;
  new_folders: number;
}

export interface OrganizeProgress {
  phase: string;
  total: number;
  current: number;
  current_file: string;
}

export interface UndoBatch {
  batch_id: string;
  batch_name: string | null;
  entries: UndoEntry[];
}

export interface UndoEntry {
  id: string;
  sequence: number;
  operation: string;
  source_path: string | null;
  target_path: string | null;
  status: string;
  executed_at: string | null;
}

// Dry-run preview
export interface DryRunItem {
  media_id: string;
  file_path: string;
  file_name: string;
  file_size: number;
}

// Config
export interface WatchConfig {
  enabled: boolean;
  debounce_ms: number;
  auto_scan_on_change: boolean;
}

export interface SyncPreset {
  id: string;
  name: string;
  source_dir: string;
  target_dir: string;
  exclusion_patterns: string[];
  verify_checksum: boolean;
  detect_orphans: boolean;
  auto_eject: boolean;
}

export interface ScheduleConfig {
  id: string;
  name: string;
  cron_expression: string;
  task_type: string;
  task_params_json: string | null;
  enabled: boolean;
}

export interface AppConfig {
  theme: string;
  language: string;
  watch: WatchConfig;
  sync_presets: SyncPreset[];
  schedules: ScheduleConfig[];
  mcp_enabled: boolean;
  gallery: GalleryConfig;
  scan: ScanConfig;
}

export interface GalleryConfig {
  thumb_size: number;
}

export interface ScanConfig {
  exclude_dir_names: string[];
  exclude_suffixes: string[];
}

export interface MediaComment {
  media_id: string;
  comment: string;
}

// Sync
export interface SyncTask {
  id: string;
  name: string;
  source_dir: string;
  target_dir: string;
  exclusion_patterns: string[];
  verify_checksum: boolean;
  detect_orphans: boolean;
}

export interface SyncFileOp {
  source: string;
  target: string;
  file_size: number;
  reason: string;
}

export interface SyncConflict {
  source_path: string;
  target_path: string;
  source_modified: string;
  target_modified: string;
  source_size: number;
  target_size: number;
  content_identical: boolean;
  resolution: string;
}

export interface SyncPlan {
  files_to_copy: SyncFileOp[];
  files_to_update: SyncFileOp[];
  conflicts: SyncConflict[];
  orphan_files: string[];
  total_bytes: number;
  total_files: number;
}

export interface SyncResult {
  files_copied: number;
  files_updated: number;
  files_skipped: number;
  bytes_transferred: number;
  errors: string[];
  cancelled: boolean;
}

export interface SyncProgress {
  phase: string;
  current: number;
  total: number;
  current_file: string;
  bytes_done: number;
  bytes_total: number;
}

export interface SyncHistoryEntry {
  id: string;
  preset_id: string | null;
  source_dir: string;
  target_dir: string;
  started_at: string;
  finished_at: string | null;
  files_copied: number;
  files_updated: number;
  files_skipped: number;
  bytes_transferred: number;
  status: string;
  error_message: string | null;
}

// Watch
export interface WatchEvent {
  path: string;
  kind: string;
  timestamp: string;
}

export interface WatchStatus {
  watched_folders: string[];
}

// Volume
export interface VolumeInfo {
  mount_point: string;
  device_node: string;
  label: string;
  is_removable: boolean;
  total_space: number;
  free_space: number;
  file_system: string;
  uuid: string | null;
}

// MCP
export interface McpStatus {
  running: boolean;
  socket_path: string;
}

export interface Tag {
  id: string;
  name: string;
  category: string;
  created_at: string;
}

export interface MediaTag {
  media_id: string;
  tag_id: string;
  confidence: number;
  source: string;
}

export interface Album {
  id: string;
  name: string;
  description: string | null;
  cover_media_id: string | null;
  auto_generated: boolean;
  created_at: string;
  media_count?: number;
}

export interface GpsMediaItem {
  media_id: string;
  file_name: string;
  thumbnail: string | null;
  latitude: number;
  longitude: number;
  date_taken: string | null;
}

// NAS upload
export interface NasConfig {
  url: string;
  account: string;
  verify_tls: boolean;
  dest_root: string;
  organize_by_date: boolean;
}

export interface NasStatus {
  connected: boolean;
  account: string | null;
  url: string | null;
}

export interface NasEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export interface NasUploadProgress {
  phase: "uploading" | "complete" | "cancelled";
  total: number;
  current: number;
  current_file: string;
  uploaded: number;
  skipped: number;
  failed: number;
}

export interface FailedUpload {
  file_name: string;
  error: string;
}

export interface NasUploadResult {
  total: number;
  uploaded: number;
  skipped: number;
  failed: FailedUpload[];
  cancelled: boolean;
  ledger_failures: number;
}

export type ViewMode = "grid" | "list";
export type AppView =
  | "dashboard"
  | "gallery"
  | "duplicates"
  | "organize"
  | "before-after"
  | "history"
  | "foldertree"
  | "bcut"
  | "review"
  | "sync"
  | "cleanup"
  | "settings"
  | "tags"
  | "albums"
  | "map"
  | "nas";

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { formatFileSize } from "@/utils/format";
import {
  HardDrive,
  FolderOpen,
  ChevronDown,
  ChevronRight,
  FileImage,
  FileVideo,
  File,
  Search,
  AlertTriangle,
} from "lucide-react";

interface TypeStat {
  ext: string;
  count: number;
  size: number;
}

interface LargeFile {
  name: string;
  path: string;
  size: number;
}

interface FolderAnalysis {
  path: string;
  name: string;
  total_size: number;
  file_count: number;
  folder_count: number;
  children: FolderAnalysis[];
  type_stats: TypeStat[];
  largest_files: LargeFile[];
}

export function SizeAnalysisPanel() {
  const [analysis, setAnalysis] = useState<FolderAnalysis | null>(null);
  const [loading, setLoading] = useState(false);

  const handleAnalyze = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (!selected) return;
    setLoading(true);
    try {
      const result = await invoke<FolderAnalysis>("analyze_folder", {
        path: selected as string,
        depth: 5,
      });
      setAnalysis(result);
    } catch {
      // Silently handle - user sees empty state
    }
    setLoading(false);
  };

  if (!analysis) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center">
          <HardDrive size={40} className="mx-auto mb-3 text-text-secondary/20" />
          <p className="text-xs text-text-secondary mb-1">폴더의 용량을 분석합니다</p>
          <p className="text-[10px] text-text-secondary/60 mb-3">
            폴더별 용량 비율, 파일 유형 통계, 대용량 파일 확인
          </p>
          <button
            onClick={handleAnalyze}
            disabled={loading}
            className="px-5 py-2 rounded-md text-xs font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
          >
            {loading ? (
              <span className="flex items-center gap-2">
                <div className="w-3 h-3 border-2 border-white border-t-transparent rounded-full animate-spin" />
                분석 중...
              </span>
            ) : (
              <span className="flex items-center gap-2">
                <Search size={14} />
                폴더 선택 및 분석
              </span>
            )}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="p-2 border-b border-border flex items-center justify-between bg-bg-secondary/50">
        <div className="flex items-center gap-2">
          <HardDrive size={14} className="text-accent" />
          <span className="text-[11px] font-semibold text-text-primary truncate">{analysis.path}</span>
        </div>
        <div className="flex items-center gap-3 text-[10px] text-text-secondary">
          <span className="font-bold text-text-primary">{formatFileSize(analysis.total_size)}</span>
          <span>{analysis.file_count.toLocaleString()}개 파일</span>
          <span>{analysis.folder_count.toLocaleString()}개 폴더</span>
          <button
            onClick={handleAnalyze}
            className="px-2 py-0.5 rounded text-[10px] bg-accent text-white hover:bg-accent-hover transition-colors"
          >
            다른 폴더
          </button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Folder tree with size bars */}
        <div className="flex-1 overflow-y-auto p-2">
          {analysis.children.map((child) => (
            <FolderRow key={child.path} node={child} maxSize={analysis.total_size} depth={0} />
          ))}
        </div>

        {/* Stats panel */}
        <div className="w-60 border-l border-border overflow-y-auto p-2.5 bg-bg-secondary/30">
          {/* Type breakdown */}
          <h3 className="text-[11px] font-semibold text-text-primary mb-2">파일 유형별 용량</h3>
          <div className="space-y-1.5 mb-4">
            {analysis.type_stats.slice(0, 15).map((stat) => (
              <div key={stat.ext}>
                <div className="flex items-center justify-between text-[10px] mb-0.5">
                  <span className="flex items-center gap-1">
                    <TypeIcon ext={stat.ext} />
                    <span className="text-text-primary font-medium uppercase">{stat.ext}</span>
                    <span className="text-text-secondary">{stat.count}개</span>
                  </span>
                  <span className="text-text-primary">{formatFileSize(stat.size)}</span>
                </div>
                <div className="h-1.5 bg-bg-secondary rounded-full overflow-hidden">
                  <div
                    className="h-full rounded-full"
                    style={{
                      width: `${Math.max(1, (stat.size / analysis.total_size) * 100)}%`,
                      backgroundColor: getColorForExt(stat.ext),
                    }}
                  />
                </div>
              </div>
            ))}
          </div>

          {/* Largest files */}
          {analysis.largest_files.length > 0 && (
            <>
              <h3 className="text-[11px] font-semibold text-text-primary mb-2 flex items-center gap-1">
                <AlertTriangle size={11} className="text-warning" />
                대용량 파일 (10MB+)
              </h3>
              <div className="space-y-1">
                {analysis.largest_files.map((f) => (
                  <button
                    key={f.path}
                    onClick={() => invoke("preview_file", { path: f.path })}
                    title={f.path}
                    className="w-full text-left px-2 py-1 rounded hover:bg-bg-secondary transition-colors"
                  >
                    <div className="flex items-center justify-between text-[10px]">
                      <span className="text-text-primary truncate pr-2">{f.name}</span>
                      <span className="text-danger font-medium shrink-0">{formatFileSize(f.size)}</span>
                    </div>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function FolderRow({
  node,
  maxSize,
  depth,
}: {
  node: FolderAnalysis;
  maxSize: number;
  depth: number;
}) {
  const [isOpen, setIsOpen] = useState(depth < 1);
  const percent = maxSize > 0 ? (node.total_size / maxSize) * 100 : 0;
  const hasChildren = node.children.length > 0;

  return (
    <div>
      <div
        className="flex items-center gap-1.5 py-1 px-1 rounded hover:bg-bg-secondary/50 cursor-pointer transition-colors"
        style={{ paddingLeft: `${depth * 16 + 4}px` }}
        onClick={() => hasChildren && setIsOpen(!isOpen)}
      >
        {hasChildren ? (
          isOpen ? (
            <ChevronDown size={12} className="text-text-secondary shrink-0" />
          ) : (
            <ChevronRight size={12} className="text-text-secondary shrink-0" />
          )
        ) : (
          <span className="w-3 shrink-0" />
        )}
        <FolderOpen size={13} className="text-yellow-500 shrink-0" />
        <span className="text-xs text-text-primary truncate">{node.name}</span>
        <span className="text-[10px] text-text-secondary shrink-0 ml-1">
          {node.file_count.toLocaleString()}개
        </span>
        <div className="flex-1 mx-2">
          <div className="h-2 bg-bg-secondary rounded-full overflow-hidden">
            <div
              className="h-full bg-accent rounded-full transition-all"
              style={{ width: `${Math.max(0.5, percent)}%` }}
            />
          </div>
        </div>
        <span className="text-xs font-medium text-text-primary shrink-0 w-16 text-right">
          {formatFileSize(node.total_size)}
        </span>
        <span className="text-[10px] text-text-secondary shrink-0 w-10 text-right">
          {percent.toFixed(1)}%
        </span>
      </div>
      {hasChildren && isOpen && (
        <div>
          {node.children.map((child) => (
            <FolderRow key={child.path} node={child} maxSize={maxSize} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

function TypeIcon({ ext }: { ext: string }) {
  const imageExts = ["jpg", "jpeg", "png", "heic", "webp", "gif", "bmp", "tiff", "raw", "cr2", "nef", "arw"];
  const videoExts = ["mp4", "mov", "avi", "mkv", "webm", "m4v"];
  if (imageExts.includes(ext)) return <FileImage size={10} className="text-blue-400" />;
  if (videoExts.includes(ext)) return <FileVideo size={10} className="text-purple-400" />;
  return <File size={10} className="text-text-secondary" />;
}

function getColorForExt(ext: string): string {
  const colors: Record<string, string> = {
    jpg: "#3b82f6", jpeg: "#3b82f6", png: "#22c55e", heic: "#f59e0b",
    gif: "#ec4899", bmp: "#6366f1", tiff: "#8b5cf6", webp: "#14b8a6",
    raw: "#f97316", cr2: "#f97316", nef: "#f97316", arw: "#f97316",
    mp4: "#a855f7", mov: "#7c3aed", avi: "#9333ea", mkv: "#c084fc",
    webm: "#d946ef", m4v: "#a78bfa",
  };
  return colors[ext] || "#6b7280";
}

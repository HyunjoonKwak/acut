import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatFileSize } from "@/utils/format";
import {
  FolderOpen,
  FolderClosed,
  ChevronDown,
  ChevronRight,
  FileImage,
  FileVideo,
  FileAudio,
  FileText,
  FileArchive,
  File,
  Check,
} from "lucide-react";
import type { TreeNode, FileTypeFilter } from "./types";

export function TreeNodeRow({
  node,
  depth,
  searchQuery,
  typeFilter,
  showFiles,
  matchingPaths,
  selectedPaths,
  dropTargetPath,
  onNavigate,
  onToggleSelect,
  onDragStart,
  onDragOverFolder,
  onDropOnFolder,
  onDragEnd,
}: {
  node: TreeNode;
  depth: number;
  searchQuery: string;
  typeFilter: FileTypeFilter;
  showFiles: boolean;
  matchingPaths: Set<string> | null;
  selectedPaths: ReadonlySet<string>;
  dropTargetPath: string | null;
  onNavigate: (path: string, name: string) => void;
  onToggleSelect: (path: string, shiftKey: boolean) => void;
  onDragStart: (e: React.DragEvent, node: TreeNode) => void;
  onDragOverFolder: (path: string) => void;
  onDropOnFolder: (targetPath: string, e: React.DragEvent) => void;
  onDragEnd: () => void;
}) {
  const [isOpen, setIsOpen] = useState(depth < 1);

  const matchesSearch = searchQuery
    ? node.name.toLowerCase().includes(searchQuery)
    : true;
  const matchesType =
    typeFilter === "all" || node.is_dir || node.file_type === typeFilter;

  const isVisible = matchingPaths
    ? matchingPaths.has(node.path)
    : true;

  if (!node.is_dir && !showFiles) return null;
  if (!node.is_dir && (!matchesSearch || !matchesType)) return null;
  if (matchingPaths && !isVisible) return null;

  const hasChildren = node.is_dir && node.children.length > 0;
  const effectiveOpen = searchQuery || typeFilter !== "all" ? true : isOpen;
  const isSelected = selectedPaths.has(node.path);
  const isDropTarget = node.is_dir && dropTargetPath === node.path;

  const handleClick = (e: React.MouseEvent) => {
    if (node.is_dir) {
      // ⌘/Ctrl-click selects a folder for copy/move/rename/trash;
      // plain click just expands it
      if (e.metaKey || e.ctrlKey) {
        onToggleSelect(node.path, e.shiftKey);
      } else {
        setIsOpen(!isOpen);
      }
    } else {
      onToggleSelect(node.path, e.shiftKey);
    }
  };

  const handleDoubleClick = () => {
    if (node.is_dir) {
      onNavigate(node.path, node.name);
    } else {
      invoke("preview_file", { path: node.path }).catch(() => {});
    }
  };

  const handleDragOver = (e: React.DragEvent) => {
    if (node.is_dir) {
      e.preventDefault();
      e.stopPropagation();
      onDragOverFolder(node.path);
    }
  };

  const handleDrop = (e: React.DragEvent) => {
    if (node.is_dir) {
      e.preventDefault();
      e.stopPropagation();
      onDropOnFolder(node.path, e);
    }
  };

  return (
    <div>
      <div
        className={`flex items-center gap-1 py-[3px] px-1 rounded cursor-pointer transition-colors group ${
          isDropTarget
            ? "bg-accent/20 ring-1 ring-accent"
            : isSelected
              ? "bg-accent/10"
              : "hover:bg-bg-secondary/60"
        }`}
        style={{ paddingLeft: `${depth * 18 + 4}px` }}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
        draggable
        onDragStart={(e) => onDragStart(e, node)}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onDragEnd={onDragEnd}
        title={node.path}
      >
        {/* Expand/collapse icon */}
        {hasChildren ? (
          effectiveOpen ? (
            <ChevronDown size={12} className="text-text-secondary/60 shrink-0" />
          ) : (
            <ChevronRight size={12} className="text-text-secondary/60 shrink-0" />
          )
        ) : (
          <span className="w-3 shrink-0" />
        )}

        {/* Selection checkbox for files */}
        {!node.is_dir && (
          <div
            onClick={(e) => {
              e.stopPropagation();
              onToggleSelect(node.path, e.shiftKey);
            }}
            className={`w-3.5 h-3.5 rounded border flex items-center justify-center shrink-0 transition-colors ${
              isSelected
                ? "bg-accent border-accent"
                : "border-border opacity-0 group-hover:opacity-100"
            }`}
          >
            {isSelected && <Check size={8} className="text-white" />}
          </div>
        )}

        {/* File/folder icon */}
        <NodeIcon node={node} isOpen={effectiveOpen} />

        {/* Name */}
        <span
          className={`text-xs truncate ${
            node.is_dir ? "text-text-primary font-medium" : "text-text-secondary"
          } ${searchQuery && matchesSearch ? "bg-yellow-500/20 rounded px-0.5" : ""}`}
        >
          {node.name}
        </span>

        {/* Folder info */}
        {node.is_dir && (
          <span className="text-[10px] text-text-secondary/50 shrink-0 ml-1">
            {node.file_count > 0 && `${node.file_count.toLocaleString()}개`}
            {node.folder_count > 0 && ` · ${node.folder_count.toLocaleString()}폴더`}
          </span>
        )}

        {/* Size */}
        <span className="text-[10px] text-text-secondary/60 shrink-0 ml-auto">
          {formatFileSize(node.size)}
        </span>

        {/* Modified date - visible on hover */}
        <span className="text-[10px] text-text-secondary/40 shrink-0 w-24 text-right hidden group-hover:inline">
          {node.modified}
        </span>
      </div>

      {/* Children */}
      {hasChildren && effectiveOpen && (
        <div>
          {node.children.map((child) => (
            <TreeNodeRow
              key={child.path}
              node={child}
              depth={depth + 1}
              searchQuery={searchQuery}
              typeFilter={typeFilter}
              showFiles={showFiles}
              matchingPaths={matchingPaths}
              selectedPaths={selectedPaths}
              dropTargetPath={dropTargetPath}
              onNavigate={onNavigate}
              onToggleSelect={onToggleSelect}
              onDragStart={onDragStart}
              onDragOverFolder={onDragOverFolder}
              onDropOnFolder={onDropOnFolder}
              onDragEnd={onDragEnd}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function NodeIcon({ node, isOpen }: { node: TreeNode; isOpen: boolean }) {
  if (node.is_dir) {
    return isOpen ? (
      <FolderOpen size={14} className="text-yellow-500 shrink-0" />
    ) : (
      <FolderClosed size={14} className="text-yellow-500 shrink-0" />
    );
  }

  switch (node.file_type) {
    case "image":
      return <FileImage size={13} className="text-blue-400 shrink-0" />;
    case "video":
      return <FileVideo size={13} className="text-purple-400 shrink-0" />;
    case "audio":
      return <FileAudio size={13} className="text-green-400 shrink-0" />;
    case "document":
      return <FileText size={13} className="text-orange-400 shrink-0" />;
    case "archive":
      return <FileArchive size={13} className="text-red-400 shrink-0" />;
    default:
      return <File size={13} className="text-text-secondary/60 shrink-0" />;
  }
}

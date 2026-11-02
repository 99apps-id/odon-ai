import React, { useState, useEffect } from 'react';
import {
  Folder,
  FolderOpen,
  File,
  FileCode,
  FileText,
  FileJson,
  Image as ImageIcon,
  ChevronRight,
  ChevronDown,
  RefreshCw,
  Plus,
  Trash2,
  Save,
  Search,
  ExternalLink,
  MessageSquare,
  Copy,
  Check,
  X,
  FilePlus,
  FolderPlus,
  ArrowUp,
} from 'lucide-react';
import { api, FsEntryItem, FsReadResult } from '../api';

interface FileExplorerViewProps {
  onAskAiAboutFile?: (path: string, content: string) => void;
  onOpenInTerminal?: (path: string) => void;
}

export const FileExplorerView: React.FC<FileExplorerViewProps> = ({
  onAskAiAboutFile,
  onOpenInTerminal,
}) => {
  const [currentPath, setCurrentPath] = useState<string>('.');
  const [entries, setEntries] = useState<FsEntryItem[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [searchQuery, setSearchQuery] = useState<string>('');
  
  // Selected file and its content
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileData, setFileData] = useState<FsReadResult | null>(null);
  const [editedContent, setEditedContent] = useState<string>('');
  const [isSaving, setIsSaving] = useState<boolean>(false);
  const [saveSuccess, setSaveSuccess] = useState<boolean>(false);
  const [copiedPath, setCopiedPath] = useState<boolean>(false);

  // Modal for new file / new folder
  const [createModal, setCreateModal] = useState<'file' | 'folder' | null>(null);
  const [newItemName, setNewItemName] = useState<string>('');

  const loadDirectory = async (path: string) => {
    setIsLoading(true);
    try {
      const res = await api.fsList(path);
      setEntries(res.entries || []);
      setCurrentPath(path);
    } catch (err) {
      console.error('Failed to load directory:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadDirectory('.');
  }, []);

  const handleOpenFile = async (entry: FsEntryItem) => {
    if (entry.is_dir) {
      loadDirectory(entry.path);
    } else {
      setSelectedFile(entry.path);
      try {
        const res = await api.fsRead(entry.path);
        setFileData(res);
        setEditedContent(res.content || '');
      } catch (err) {
        console.error('Failed to read file:', err);
      }
    }
  };

  const handleGoUp = () => {
    if (currentPath === '.' || !currentPath.includes('/')) {
      loadDirectory('.');
      return;
    }
    const parts = currentPath.split('/');
    parts.pop();
    const parent = parts.join('/') || '.';
    loadDirectory(parent);
  };

  const handleSaveFile = async () => {
    if (!selectedFile) return;
    setIsSaving(true);
    try {
      const res = await api.fsWrite(selectedFile, editedContent);
      if (res.success) {
        setSaveSuccess(true);
        setTimeout(() => setSaveSuccess(false), 2000);
      }
    } catch (err) {
      console.error('Failed to save file:', err);
    } finally {
      setIsSaving(false);
    }
  };

  const handleDeleteEntry = async (entry: FsEntryItem, e: React.MouseEvent) => {
    e.stopPropagation();
    if (!confirm(`Are you sure you want to delete ${entry.name}?`)) return;
    try {
      await api.fsDelete(entry.path);
      if (selectedFile === entry.path) {
        setSelectedFile(null);
        setFileData(null);
      }
      loadDirectory(currentPath);
    } catch (err) {
      console.error('Failed to delete path:', err);
    }
  };

  const handleCreateItem = async () => {
    if (!newItemName.trim() || !createModal) return;
    const target = currentPath === '.' ? newItemName.trim() : `${currentPath}/${newItemName.trim()}`;
    try {
      if (createModal === 'file') {
        await api.fsWrite(target, '');
      } else {
        await api.fsMkdir(target);
      }
      setCreateModal(null);
      setNewItemName('');
      loadDirectory(currentPath);
    } catch (err) {
      console.error('Failed to create item:', err);
    }
  };

  const handleCopyPath = () => {
    if (!selectedFile) return;
    navigator.clipboard.writeText(selectedFile);
    setCopiedPath(true);
    setTimeout(() => setCopiedPath(false), 1500);
  };

  const filteredEntries = entries.filter((e) =>
    e.name.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const getFileIcon = (entry: FsEntryItem) => {
    if (entry.is_dir) return <Folder className="w-4 h-4 text-blue-500 fill-blue-500/20" />;
    const ext = entry.extension?.toLowerCase();
    if (['ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'go', 'html', 'css'].includes(ext || '')) {
      return <FileCode className="w-4 h-4 text-emerald-600" />;
    }
    if (['json', 'yaml', 'yml', 'toml'].includes(ext || '')) {
      return <FileJson className="w-4 h-4 text-amber-600" />;
    }
    if (['png', 'jpg', 'jpeg', 'webp', 'svg', 'gif'].includes(ext || '')) {
      return <ImageIcon className="w-4 h-4 text-blue-600" />;
    }
    return <FileText className="w-4 h-4 text-neutral-500" />;
  };

  return (
    <div className="flex-1 flex overflow-hidden rounded-2xl border border-neutral-200/90 bg-white shadow-2xs">
      {/* ───────────────────────────────────────────────────────────
          LEFT: FILE TREE & EXPLORER SIDEBAR
      ─────────────────────────────────────────────────────────── */}
      <div className="w-80 shrink-0 border-r border-neutral-200 flex flex-col bg-neutral-50/50">
        {/* Explorer Header */}
        <div className="p-3 border-b border-neutral-200 space-y-2">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-1.5 font-bold text-xs text-neutral-800">
              <FolderOpen className="w-4 h-4 text-blue-600" />
              <span>Workspace Files</span>
            </div>

            <div className="flex items-center gap-1">
              <button
                onClick={() => setCreateModal('file')}
                className="p-1 rounded-md text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/70 cursor-pointer"
                title="New File"
              >
                <FilePlus className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => setCreateModal('folder')}
                className="p-1 rounded-md text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/70 cursor-pointer"
                title="New Folder"
              >
                <FolderPlus className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => loadDirectory(currentPath)}
                className="p-1 rounded-md text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/70 cursor-pointer"
                title="Refresh"
              >
                <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? 'animate-spin' : ''}`} />
              </button>
            </div>
          </div>

          {/* Search Input */}
          <div className="relative">
            <Search className="w-3.5 h-3.5 text-neutral-400 absolute left-2.5 top-2" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Filter files..."
              className="w-full pl-8 pr-2.5 py-1 text-xs rounded-lg border border-neutral-200 bg-white outline-none focus:border-blue-500 font-mono"
            />
          </div>

          {/* Breadcrumb Path & Up Button */}
          <div className="flex items-center justify-between text-[11px] text-neutral-500 font-mono pt-1">
            <span className="truncate max-w-[200px]" title={currentPath}>
              {currentPath}
            </span>
            {currentPath !== '.' && (
              <button
                onClick={handleGoUp}
                className="flex items-center gap-1 text-blue-600 hover:underline cursor-pointer"
              >
                <ArrowUp className="w-3 h-3" />
                <span>Up</span>
              </button>
            )}
          </div>
        </div>

        {/* Entries List */}
        <div className="flex-1 overflow-y-auto p-1.5 space-y-0.5">
          {filteredEntries.length === 0 ? (
            <div className="p-6 text-center text-xs text-neutral-400">
              {isLoading ? 'Loading...' : 'Empty folder'}
            </div>
          ) : (
            filteredEntries.map((entry) => {
              const isSelected = selectedFile === entry.path;
              return (
                <div
                  key={entry.path}
                  onClick={() => handleOpenFile(entry)}
                  className={`group flex items-center justify-between px-2.5 py-1.5 rounded-lg text-xs cursor-pointer transition-colors ${
                    isSelected
                      ? 'bg-blue-50 text-blue-700 font-medium'
                      : 'text-neutral-700 hover:bg-neutral-200/50'
                  }`}
                >
                  <div className="flex items-center gap-2 truncate">
                    {getFileIcon(entry)}
                    <span className="truncate font-mono">{entry.name}</span>
                  </div>

                  <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    {!entry.is_dir && entry.size > 0 && (
                      <span className="text-[10px] text-neutral-400 font-mono">
                        {(entry.size / 1024).toFixed(0)}k
                      </span>
                    )}
                    <button
                      onClick={(e) => handleDeleteEntry(entry, e)}
                      className="p-1 rounded text-neutral-400 hover:text-red-600 hover:bg-red-50 cursor-pointer"
                      title="Delete"
                    >
                      <Trash2 className="w-3 h-3" />
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>

      {/* ───────────────────────────────────────────────────────────
          RIGHT: FILE VIEWER / EDITOR
      ─────────────────────────────────────────────────────────── */}
      <div className="flex-1 flex flex-col overflow-hidden bg-white">
        {selectedFile && fileData ? (
          <>
            {/* File Header Bar */}
            <div className="h-10 px-4 border-b border-neutral-200 flex items-center justify-between bg-neutral-50/70 shrink-0">
              <div className="flex items-center gap-2 font-mono text-xs text-neutral-800 truncate">
                <span className="font-semibold">{fileData.name}</span>
                <span className="text-[11px] text-neutral-400">({(fileData.size / 1024).toFixed(1)} KB)</span>
              </div>

              <div className="flex items-center gap-2">
                <button
                  onClick={handleCopyPath}
                  className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium text-neutral-600 hover:bg-neutral-200/60 cursor-pointer"
                  title="Copy path"
                >
                  {copiedPath ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
                  <span>{copiedPath ? 'Copied' : 'Copy Path'}</span>
                </button>

                {onAskAiAboutFile && fileData.is_text && (
                  <button
                    onClick={() => onAskAiAboutFile(selectedFile, editedContent)}
                    className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium text-blue-700 bg-blue-50 hover:bg-blue-100 cursor-pointer"
                    title="Ask AI about this file"
                  >
                    <MessageSquare className="w-3.5 h-3.5" />
                    <span>Ask AI</span>
                  </button>
                )}

                {onOpenInTerminal && (
                  <button
                    onClick={() => onOpenInTerminal(currentPath)}
                    className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium text-neutral-600 hover:bg-neutral-200/60 cursor-pointer"
                    title="Open terminal here"
                  >
                    <ExternalLink className="w-3.5 h-3.5" />
                    <span>Terminal</span>
                  </button>
                )}

                {fileData.is_text && (
                  <button
                    onClick={handleSaveFile}
                    disabled={isSaving}
                    className="flex items-center gap-1 px-3 py-1 rounded-lg text-xs font-medium text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-50 cursor-pointer shadow-xs"
                  >
                    {saveSuccess ? <Check className="w-3.5 h-3.5" /> : <Save className="w-3.5 h-3.5" />}
                    <span>{saveSuccess ? 'Saved' : isSaving ? 'Saving...' : 'Save File'}</span>
                  </button>
                )}
              </div>
            </div>

            {/* File Content Area */}
            <div className="flex-1 overflow-auto p-4 font-mono text-xs">
              {fileData.is_text ? (
                <textarea
                  value={editedContent}
                  onChange={(e) => setEditedContent(e.target.value)}
                  className="w-full h-full resize-none outline-none font-mono text-xs text-neutral-800 leading-relaxed bg-transparent"
                  spellCheck={false}
                />
              ) : fileData.base64 ? (
                <div className="flex flex-col items-center justify-center h-full p-8">
                  <img
                    src={`data:image/png;base64,${fileData.base64}`}
                    alt={fileData.name}
                    className="max-h-[80%] max-w-[80%] rounded-xl shadow-md border border-neutral-200 object-contain"
                  />
                  <div className="mt-4 text-neutral-400 text-xs">Image Preview</div>
                </div>
              ) : (
                <div className="p-8 text-neutral-400 text-center">Binary file cannot be displayed</div>
              )}
            </div>
          </>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center text-neutral-400 p-8 text-center space-y-2">
            <FolderOpen className="w-12 h-12 text-neutral-300 stroke-[1.5]" />
            <div className="text-sm font-medium text-neutral-600">Select a file from the explorer to preview or edit</div>
            <div className="text-xs text-neutral-400 max-w-sm">
              Browse workspace directories, inspect source code, edit text files, and seamlessly ask the AI agent about any file.
            </div>
          </div>
        )}
      </div>

      {/* ───────────────────────────────────────────────────────────
          NEW FILE / NEW FOLDER MODAL
      ─────────────────────────────────────────────────────────── */}
      {createModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-neutral-200/70 backdrop-blur-2xs">
          <div className="w-full max-w-sm bg-white rounded-2xl shadow-xl border border-neutral-200 p-4 space-y-3">
            <div className="flex items-center justify-between font-bold text-sm text-neutral-900">
              <span>{createModal === 'file' ? 'Create New File' : 'Create New Folder'}</span>
              <button onClick={() => setCreateModal(null)} className="text-neutral-400 hover:text-neutral-700">
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="text-xs text-neutral-500 font-mono">Location: {currentPath}</div>

            <input
              type="text"
              value={newItemName}
              onChange={(e) => setNewItemName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleCreateItem()}
              placeholder={createModal === 'file' ? 'filename.ts' : 'folder-name'}
              className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 text-xs font-mono outline-none focus:border-blue-500"
              autoFocus
            />

            <div className="flex justify-end gap-2 pt-2">
              <button
                onClick={() => setCreateModal(null)}
                className="px-3 py-1.5 rounded-lg text-xs font-medium text-neutral-600 hover:bg-neutral-100 cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={handleCreateItem}
                disabled={!newItemName.trim()}
                className="px-3 py-1.5 rounded-lg text-xs font-medium text-white bg-blue-600 hover:bg-blue-500 disabled:opacity-40 cursor-pointer shadow-xs transition-colors"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

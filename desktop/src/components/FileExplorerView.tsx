import React, { useState, useEffect, useRef } from 'react';
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
  ArrowLeft,
  ArrowRight,
  AlertCircle,
} from 'lucide-react';
import { api, FsEntryItem, FsReadResult } from '../api';
import { CodeEditor } from './CodeEditor';

interface FileExplorerViewProps {
  /** Workspace selected in the composer; the explorer starts under it. */
  workspaceRoot?: string;
  onAskAiAboutFile?: (path: string, content: string) => void;
  onOpenInTerminal?: (path: string) => void;
}

const joinPath = (base: string, child: string): string => {
  if (!base || base === '.') return child;
  const separator = base.includes('\\') ? '\\' : '/';
  return `${base.replace(/[\\/]+$/, '')}${separator}${child}`;
};

const parentPath = (path: string): string | null => {
  const trimmed = path.replace(/[\\/]+$/, '');
  if (!trimmed || trimmed === '.') return null;
  const index = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (index < 0) return null;
  if (index === 0) return trimmed.slice(0, 1);
  const parent = trimmed.slice(0, index);
  // `C:` is a drive-relative spelling, not a directory; keep the root separator.
  if (/^[a-zA-Z]:$/.test(parent)) return `${parent}\\`;
  return parent;
};

const pathKey = (path: string): string =>
  path.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();

const samePath = (a: string, b: string): boolean => pathKey(a) === pathKey(b);

const isInside = (root: string, path: string): boolean => {
  const rootKey = pathKey(root);
  return pathKey(path) === rootKey || pathKey(path).startsWith(`${rootKey}/`);
};

const relativeTo = (root: string, path: string): string => {
  const trimmedRoot = root.replace(/[\\/]+$/, '');
  if (samePath(trimmedRoot, path)) return '.';
  if (pathKey(path).startsWith(`${pathKey(trimmedRoot)}/`)) {
    return path.slice(trimmedRoot.length + 1);
  }
  return path;
};

export const FileExplorerView: React.FC<FileExplorerViewProps> = ({
  workspaceRoot,
  onAskAiAboutFile,
  onOpenInTerminal,
}) => {
  const rootPath = workspaceRoot && workspaceRoot.trim() ? workspaceRoot : '.';
  const [currentPath, setCurrentPath] = useState<string>(rootPath);
  const [history, setHistory] = useState<string[]>([rootPath]);
  const [historyIndex, setHistoryIndex] = useState<number>(0);
  const [entries, setEntries] = useState<FsEntryItem[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState<string>('');

  // Selected file and its content
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileData, setFileData] = useState<FsReadResult | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [editedContent, setEditedContent] = useState<string>('');
  const [isSaving, setIsSaving] = useState<boolean>(false);
  const [saveSuccess, setSaveSuccess] = useState<boolean>(false);
  const [copiedPath, setCopiedPath] = useState<boolean>(false);

  // Modal for new file / new folder
  const [createModal, setCreateModal] = useState<'file' | 'folder' | null>(null);
  const [newItemName, setNewItemName] = useState<string>('');
  // A slow listing for a previous workspace must not overwrite the folder the
  // operator is now looking at.
  const loadRequestRef = useRef(0);
  // Same guard for file reads: a slow read of file A landing after file B was
  // opened used to leave B selected with A's text in the editor, and Save then
  // overwrote B with A's content.
  const openRequestRef = useRef(0);
  // Line ending of the file as it is on disk. A controlled <textarea> reports
  // LF-only values after the first edit, so saving blindly rewrote every
  // CRLF-terminated line (breaking .bat files and repos with .gitattributes).
  const fileEolRef = useRef<'lf' | 'crlf'>('lf');

  const loadDirectory = async (path: string) => {
    const requestId = ++loadRequestRef.current;
    setIsLoading(true);
    setLoadError(null);
    try {
      const res = await api.fsList(path);
      if (requestId !== loadRequestRef.current) return;
      if (res.error) {
        setEntries([]);
        setLoadError(res.error);
        return;
      }
      setEntries(res.entries || []);
      setCurrentPath(path);
    } catch (err: any) {
      if (requestId !== loadRequestRef.current) return;
      setEntries([]);
      setLoadError(err?.message || 'Failed to load directory');
    } finally {
      if (requestId === loadRequestRef.current) setIsLoading(false);
    }
  };

  // Follow the workspace selected in the composer. Switching workspaces starts
  // a new history: back/forward must not walk into a folder that is no longer
  // allowed.
  useEffect(() => {
    // Dirty edits in the previous workspace would be silently lost.
    if (selectedFile && isDirty && !window.confirm('This file has unsaved changes. Discard them?')) {
      return;
    }
    setHistory([rootPath]);
    setHistoryIndex(0);
    setSelectedFile(null);
    setFileData(null);
    setFileError(null);
    setEditedContent('');
    setSearchQuery('');
    void loadDirectory(rootPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rootPath]);

  const navigateTo = (path: string) => {
    if (!samePath(path, currentPath)) {
      const nextHistory = history.slice(0, historyIndex + 1);
      if (!nextHistory.length || !samePath(nextHistory[nextHistory.length - 1], path)) {
        nextHistory.push(path);
        setHistory(nextHistory);
        setHistoryIndex(nextHistory.length - 1);
      }
    }
    void loadDirectory(path);
  };

  const canGoBack = historyIndex > 0;
  const canGoForward = historyIndex < history.length - 1;
  const parent = parentPath(currentPath);
  const canGoUp = !!parent && (rootPath === '.' || isInside(rootPath, parent));

  const goBack = () => {
    if (!canGoBack) return;
    const nextIndex = historyIndex - 1;
    setHistoryIndex(nextIndex);
    void loadDirectory(history[nextIndex]);
  };

  const goForward = () => {
    if (!canGoForward) return;
    const nextIndex = historyIndex + 1;
    setHistoryIndex(nextIndex);
    void loadDirectory(history[nextIndex]);
  };

  const handleGoUp = () => {
    if (!canGoUp || !parent) return;
    navigateTo(parent);
  };

  const isDirty =
    !!fileData && editedContent !== (fileData.content || '').replace(/\r\n/g, '\n');

  const hasUnsavedChanges = () => isDirty && !window.confirm('This file has unsaved changes. Discard them?');

  const handleOpenFile = async (entry: FsEntryItem) => {
    const fullPath = joinPath(currentPath, entry.name);
    if (entry.is_dir) {
      navigateTo(fullPath);
      return;
    }
    if (selectedFile && !samePath(selectedFile, fullPath) && hasUnsavedChanges()) return;
    const requestId = ++openRequestRef.current;
    setSelectedFile(fullPath);
    setFileError(null);
    try {
      const res = await api.fsRead(fullPath);
      if (requestId !== openRequestRef.current) return;
      if (res.error) {
        setFileData(null);
        setFileError(res.error);
        return;
      }
      const raw = res.content || '';
      fileEolRef.current = raw.includes('\r\n') ? 'crlf' : 'lf';
      setFileData(res);
      setEditedContent(raw.replace(/\r\n/g, '\n'));
    } catch (err: any) {
      if (requestId !== openRequestRef.current) return;
      setFileData(null);
      setFileError(err?.message || 'Failed to read file');
    }
  };

  const handleSaveFile = async () => {
    if (!selectedFile || !isDirty) return;
    setIsSaving(true);
    try {
      // Restore the on-disk line ending before writing.
      const content =
        fileEolRef.current === 'crlf' ? editedContent.replace(/\n/g, '\r\n') : editedContent;
      const res = await api.fsWrite(selectedFile, content);
      if (res.success) {
        setFileError(null);
        setSaveSuccess(true);
        setTimeout(() => setSaveSuccess(false), 2000);
      } else {
        setFileError(res.error || 'Failed to save file');
      }
    } catch (err: any) {
      setFileError(err?.message || 'Failed to save file');
    } finally {
      setIsSaving(false);
    }
  };

  const handleDeleteEntry = async (entry: FsEntryItem, e: React.MouseEvent) => {
    e.stopPropagation();
    if (!confirm(`Are you sure you want to delete ${entry.name}?`)) return;
    const fullPath = joinPath(currentPath, entry.name);
    try {
      const res = await api.fsDelete(fullPath);
      // The daemon answers HTTP 200 with { success: false } when an approval is
      // rejected; treating that as success cleared the UI as if it worked.
      if (res && res.success === false) {
        setLoadError(res.error || 'Delete was not permitted');
        return;
      }
      if (selectedFile && samePath(selectedFile, fullPath)) {
        setSelectedFile(null);
        setFileData(null);
        setFileError(null);
      }
      loadDirectory(currentPath);
    } catch (err) {
      console.error('Failed to delete path:', err);
      setLoadError('Failed to delete path');
    }
  };

  const handleCreateItem = async () => {
    if (!newItemName.trim() || !createModal) return;
    const target = joinPath(currentPath, newItemName.trim());
    try {
      const res =
        createModal === 'file' ? await api.fsWrite(target, '') : await api.fsMkdir(target);
      if (res && res.success === false) {
        setLoadError(res.error || 'Create was not permitted');
        return;
      }
      setCreateModal(null);
      setNewItemName('');
      loadDirectory(currentPath);
    } catch (err) {
      console.error('Failed to create item:', err);
      setLoadError('Failed to create item');
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

  const rootLabel =
    rootPath === '.'
      ? 'Workspace'
      : rootPath.replace(/[\\/]+$/, '').split(/[\\/]/).filter(Boolean).pop() || rootPath;
  const relativePath = relativeTo(rootPath, currentPath);
  const breadcrumb = [
    rootLabel,
    ...(relativePath === '.' ? [] : relativePath.split(/[\\/]/).filter(Boolean)),
  ].join(' / ');

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

          {/* Navigation: back / forward / up + breadcrumb */}
          <div className="flex items-center gap-1 min-w-0 pt-1">
            <button
              onClick={goBack}
              disabled={!canGoBack}
              className="p-1 rounded-md text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/70 disabled:opacity-30 disabled:cursor-default cursor-pointer"
              title="Back"
              aria-label="Back"
            >
              <ArrowLeft className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={goForward}
              disabled={!canGoForward}
              className="p-1 rounded-md text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/70 disabled:opacity-30 disabled:cursor-default cursor-pointer"
              title="Forward"
              aria-label="Forward"
            >
              <ArrowRight className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={handleGoUp}
              disabled={!canGoUp}
              className="p-1 rounded-md text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/70 disabled:opacity-30 disabled:cursor-default cursor-pointer"
              title="Up one folder"
              aria-label="Up one folder"
            >
              <ArrowUp className="w-3.5 h-3.5" />
            </button>
            <span
              className="text-[11px] text-neutral-500 font-mono truncate min-w-0"
              title={currentPath}
            >
              {breadcrumb}
            </span>
          </div>
        </div>

        {/* Entries List */}
        <div className="flex-1 overflow-y-auto p-1.5 space-y-0.5">
          {loadError ? (
            <div className="p-4 text-xs text-red-600 flex items-start gap-1.5">
              <AlertCircle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
              <span>{loadError}</span>
            </div>
          ) : filteredEntries.length === 0 ? (
            <div className="p-6 text-center text-xs text-neutral-400">
              {isLoading ? 'Loading...' : 'Empty folder'}
            </div>
          ) : (
            filteredEntries.map((entry) => {
              const entryPath = joinPath(currentPath, entry.name);
              const isSelected = !!selectedFile && samePath(selectedFile, entryPath);
              return (
                <div
                  key={entry.name}
                  onClick={() => handleOpenFile(entry)}
                  className={`group flex items-center justify-between px-2.5 py-1.5 rounded-lg text-xs cursor-pointer transition-colors ${
                    isSelected
                      ? 'bg-blue-50 text-blue-700 font-medium'
                      : 'text-neutral-700 hover:bg-neutral-200/50'
                  }`}
                  title={entryPath}
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
            <div className="flex-1 min-h-0 overflow-hidden">
              {fileData.is_text ? (
                <CodeEditor
                  value={editedContent}
                  onChange={setEditedContent}
                  language={fileData.name.split('.').pop() || 'text'}
                  onSave={handleSaveFile}
                />
              ) : fileData.base64 ? (
                <div className="flex h-full flex-col items-center justify-center overflow-auto p-8">
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
        ) : selectedFile && fileError ? (
          <div className="flex-1 flex flex-col items-center justify-center gap-2 p-8 text-center">
            <AlertCircle className="w-8 h-8 text-red-400" />
            <div className="text-sm font-medium text-neutral-700">Cannot open file</div>
            <div className="text-xs text-neutral-500 max-w-md break-words">{fileError}</div>
            <div className="text-[11px] text-neutral-400 font-mono break-all">{selectedFile}</div>
          </div>
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

            <div className="text-xs text-neutral-500 font-mono truncate" title={currentPath}>
              Location: {currentPath}
            </div>

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
                className="px-3 py-1.5 rounded-lg text-xs font-medium text-neutral-600 hover:bg-neutral-200 cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={handleCreateItem}
                disabled={!newItemName.trim()}
                className="px-3 py-1.5 rounded-lg text-xs font-medium text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-40 cursor-pointer shadow-xs transition-colors"
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

import React, { useEffect, useRef, useState } from 'react';
import { Copy, Check, FileCode, Play, Save } from 'lucide-react';
import { DiffView } from './DiffView';
import { CodeEditor } from './CodeEditor';
import { api } from '../api';

interface CodeFile {
  name: string;
  path: string;
  language: string;
  content: string;
}

// The editor is mounted in two places (the full-page Code view and the right
// panel) and switching views unmounts one of them. Keeping the buffers in a
// module-level store means unsaved edits survive the unmount instead of being
// silently discarded, and both mounts see the same draft.
const sharedBuffers: Record<string, string> = {};
const sharedSaved: Record<string, string> = {};


interface CodeEditorViewProps {
  onRunPreview?: () => void;
  onOpenTerminal?: () => void;
  files?: CodeFile[];
  /** Unified diff to render instead of the file body (patch artifacts). */
  diff?: string;
  /**
   * Identity of the artifact the files come from. When it changes (a new or
   * regenerated artifact), the drafts are reset so a fresh generation is not
   * hidden behind a stale edit buffer.
   */
  filesKey?: string;
}

export function CodeEditorView({ onRunPreview, onOpenTerminal, files, diff, filesKey }: CodeEditorViewProps) {
  const fileList = files && files.length > 0 ? files : [];
  const [activeFileIndex, setActiveFileIndex] = useState(0);
  const [copied, setCopied] = useState(false);
  // Edited buffers survive tab switches and are written back to the workspace
  // on save; until then the artifact's own content stays the baseline.
  const [buffers, setBuffers] = useState<Record<string, string>>(() => ({ ...sharedBuffers }));
  // Content as of the last successful write. Without this baseline the dirty
  // indicator could never clear: the buffer differs from the artifact content
  // forever after the first save.
  const [savedContent, setSavedContent] = useState<Record<string, string>>(() => ({ ...sharedSaved }));
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle');
  const [saveError, setSaveError] = useState('');
  const savingRef = useRef(false);

  // A new (or regenerated) artifact resets the drafts and tab selection.
  useEffect(() => {
    for (const key of Object.keys(sharedBuffers)) delete sharedBuffers[key];
    for (const key of Object.keys(sharedSaved)) delete sharedSaved[key];
    setBuffers({});
    setSavedContent({});
    setActiveFileIndex(0);
  }, [filesKey]);

  const currentFile = fileList[activeFileIndex] || fileList[0];
  const currentContent = currentFile
    ? buffers[currentFile.path] ?? currentFile.content
    : '';
  const baselineContent = currentFile
    ? savedContent[currentFile.path] ?? currentFile.content
    : '';
  const isDirty = currentContent !== baselineContent;

  if (diff && diff.trim()) {
    return (
      <div className="h-full flex flex-col bg-sand-50 text-neutral-800 rounded-xl overflow-hidden border border-neutral-200 select-text">
        <div className="flex items-center justify-between bg-white px-3 py-2 border-b border-neutral-200 shrink-0">
          <span className="text-xs font-semibold text-neutral-900">Applied patch</span>
          <button
            onClick={() => {
              navigator.clipboard.writeText(diff);
              setCopied(true);
              setTimeout(() => setCopied(false), 2000);
            }}
            className="flex items-center gap-1 px-2.5 py-1 rounded hover:bg-neutral-200 text-neutral-600 text-xs font-sans transition-colors cursor-pointer"
          >
            {copied ? <Check className="w-3.5 h-3.5 text-emerald-600" /> : <Copy className="w-3.5 h-3.5" />}
            <span>{copied ? 'Copied' : 'Copy diff'}</span>
          </button>
        </div>
        <div className="flex-1 overflow-auto p-3">
          <DiffView diff={diff} collapsible={false} />
        </div>
      </div>
    );
  }

  const handleCopy = () => {
    if (!currentFile) return;
    navigator.clipboard.writeText(currentContent);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleSave = async () => {
    if (!currentFile || !isDirty) return;
    // One lock shared by the button and Ctrl+S so a write in flight cannot be
    // issued twice concurrently for the same path.
    if (savingRef.current) return;
    savingRef.current = true;
    setSaveState('saving');
    setSaveError('');
    try {
      const result = await api.fsWrite(currentFile.path, currentContent);
      if (!result.success) {
        throw new Error(result.error || 'The daemon did not confirm the write.');
      }
      // Advance the baseline so the dirty dot clears.
      sharedSaved[currentFile.path] = currentContent;
      setSavedContent((previous) => ({ ...previous, [currentFile.path]: currentContent }));
      setSaveState('saved');
      setTimeout(() => setSaveState('idle'), 2000);
    } catch (error: any) {
      setSaveState('error');
      setSaveError(error?.message || 'Failed to save file');
    } finally {
      savingRef.current = false;
    }
  };

  if (!currentFile) {
    return (
      <div className="h-full flex flex-col items-center justify-center gap-2 bg-slate-50/70 text-slate-500 rounded-xl border border-slate-200 p-8 text-center font-sans">
        <FileCode className="w-8 h-8 text-slate-300" />
        <div className="text-sm font-semibold text-slate-700">No generated code yet</div>
        <p className="text-xs max-w-sm leading-relaxed">
          Ask the agent to build a project or module, or open a workspace file from the File Explorer.
        </p>
      </div>
    );
  }

  const language = currentFile.path.split('.').pop() || currentFile.language;

  return (
    <div className="h-full flex flex-col bg-slate-50/70 text-slate-800 rounded-xl overflow-hidden border border-slate-200 font-mono text-[13px] select-text">
      {/* File Tabs & Actions */}
      <div className="flex items-center justify-between bg-white px-2 border-b border-slate-200 shrink-0">
        <div className="flex items-center overflow-x-auto scrollbar-none">
          {fileList.map((file, idx) => {
            const isActive = idx === activeFileIndex;
            const tabDirty = (buffers[file.path] ?? file.content) !== (savedContent[file.path] ?? file.content);
            return (
              <button
                key={file.path}
                onClick={() => setActiveFileIndex(idx)}
                className={`flex items-center gap-1.5 px-3 py-2 text-xs border-b-2 transition-colors whitespace-nowrap cursor-pointer ${
                  isActive
                    ? 'border-blue-600 bg-blue-50/50 text-blue-700 font-semibold'
                    : 'border-transparent text-slate-500 hover:text-slate-800 hover:bg-slate-50'
                }`}
              >
                <FileCode className={`w-3.5 h-3.5 ${isActive ? 'text-blue-600' : 'text-slate-400'}`} />
                {file.name}
                {tabDirty && <span className="text-amber-500" title="Unsaved changes">●</span>}
              </button>
            );
          })}
        </div>

        <div className="flex items-center gap-1.5 py-1 pl-2 shrink-0">
          {onOpenTerminal && (
            <button
              onClick={onOpenTerminal}
              className="flex items-center gap-1 px-2.5 py-1 rounded bg-slate-100 hover:bg-slate-200 text-slate-700 text-xs font-sans font-medium transition-colors cursor-pointer"
              title="Open Terminal"
            >
              Terminal
            </button>
          )}

          {onRunPreview && (
            <button
              onClick={onRunPreview}
              className="flex items-center gap-1.5 px-2.5 py-1 rounded bg-blue-50 hover:bg-blue-100 text-blue-700 text-xs font-sans font-medium transition-colors border border-blue-200 cursor-pointer"
              title="Preview in Browser"
            >
              <Play className="w-3 h-3 fill-current text-blue-600" />
              Preview
            </button>
          )}

          <button
            onClick={handleSave}
            disabled={saveState === 'saving'}
            className="flex items-center gap-1 px-2.5 py-1 rounded bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white text-xs font-sans font-medium transition-colors cursor-pointer"
            title="Save file to the workspace (Ctrl+S)"
          >
            {saveState === 'saved' ? <Check className="w-3.5 h-3.5" /> : <Save className="w-3.5 h-3.5" />}
            <span>
              {saveState === 'saving' ? 'Saving…' : saveState === 'saved' ? 'Saved' : 'Save'}
            </span>
          </button>

          <button
            onClick={handleCopy}
            className="flex items-center gap-1 px-2.5 py-1 rounded hover:bg-slate-100 text-slate-600 text-xs font-sans transition-colors cursor-pointer"
            title="Copy code"
          >
            {copied ? (
              <>
                <Check className="w-3.5 h-3.5 text-emerald-600" />
                <span className="text-emerald-600 text-xs font-medium">Copied</span>
              </>
            ) : (
              <>
                <Copy className="w-3.5 h-3.5" />
                <span>Copy</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Editable, syntax-highlighted code area. Keyed by path so switching
          tabs starts with a clean caret/scroll instead of the previous file's
          numeric offset. */}
      <div className="flex-1 min-h-0">
        <CodeEditor
          key={currentFile.path}
          value={currentContent}
          onChange={(next) => {
            sharedBuffers[currentFile.path] = next;
            setBuffers((previous) => ({ ...previous, [currentFile.path]: next }));
          }}
          language={language}
          onSave={handleSave}
        />
      </div>

      {/* Editor Footer */}
      <div className="h-6 bg-white border-t border-slate-200 px-3 flex items-center justify-between text-[11px] text-slate-500 shrink-0 font-sans">
        <span className="truncate">{currentFile.path}</span>
        <div className="flex items-center gap-3">
          <span className="uppercase">{language}</span>
          {saveState === 'error' ? (
            <span className="text-red-600 truncate max-w-[260px]" title={saveError}>
              {saveError}
            </span>
          ) : (
            <span>UTF-8</span>
          )}
        </div>
      </div>
    </div>
  );
}

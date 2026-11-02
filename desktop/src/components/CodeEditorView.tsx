import React, { useState } from 'react';
import { Copy, Check, FileCode, Play } from 'lucide-react';

interface CodeFile {
  name: string;
  path: string;
  language: string;
  content: string;
}

const SAMPLE_FILES: CodeFile[] = [
  {
    name: 'App.tsx',
    path: 'src/App.tsx',
    language: 'typescript',
    content: `import React, { useState, useEffect } from 'react';
import { NoteList } from './features/notes/NoteList';
import { Note } from './types';
import { Search, Plus } from 'lucide-react';

export default function App() {
  const [notes, setNotes] = useState<Note[]>(() => {
    const saved = localStorage.getItem('odon_notes');
    return saved ? JSON.parse(saved) : [
      { id: '1', title: 'React 19 Architecture', body: 'Exploring useActionState, useOptimistic, and Server Actions.', tag: 'architecture', createdAt: '10:15 AM' },
      { id: '2', title: 'ODON Runtime Integration', body: 'Autonomous tool loops with risk-tier policy gating.', tag: 'odon', createdAt: '10:22 AM' },
      { id: '3', title: 'Knowledge Retrieval', body: 'Local vector search with zero latency SQLite persistence.', tag: 'research', createdAt: '10:24 AM' },
    ];
  });

  const [search, setSearch] = useState('');

  useEffect(() => {
    localStorage.setItem('odon_notes', JSON.stringify(notes));
  }, [notes]);

  return (
    <div className="min-h-screen bg-white text-neutral-900 p-8 antialiased">
      <header className="flex items-center justify-between pb-6 border-b border-neutral-200">
        <h1 className="font-semibold text-lg tracking-tight">Personal Notes</h1>
      </header>
      <main className="mt-6">
        <NoteList notes={notes.filter(n => n.title.toLowerCase().includes(search.toLowerCase()))} />
      </main>
    </div>
  );
}`,
  },
  {
    name: 'NoteList.tsx',
    path: 'src/features/notes/NoteList.tsx',
    language: 'typescript',
    content: `import React from 'react';
import { Note } from '../../types';

interface NoteListProps {
  notes: Note[];
}

export function NoteList({ notes }: NoteListProps) {
  if (notes.length === 0) {
    return (
      <div className="text-center py-12 text-neutral-400 text-sm">
        No notes yet. Add your first note above.
      </div>
    );
  }

  return (
    <div className="divide-y divide-neutral-100">
      {notes.map((note) => (
        <article key={note.id} className="py-4 group">
          <div className="flex items-baseline justify-between mb-1">
            <h3 className="font-medium text-sm text-neutral-900">{note.title}</h3>
            <time className="text-xs text-neutral-400 font-mono">{note.createdAt}</time>
          </div>
          <p className="text-sm text-neutral-600 leading-relaxed">{note.body}</p>
          <div className="mt-2">
            <span className="inline-block text-[11px] font-mono text-neutral-500 bg-neutral-100 px-2 py-0.5 rounded">
              #{note.tag}
            </span>
          </div>
        </article>
      ))}
    </div>
  );
}`,
  },
  {
    name: 'types.ts',
    path: 'src/types.ts',
    language: 'typescript',
    content: `export interface Note {
  id: string;
  title: string;
  body: string;
  tag: string;
  createdAt: string;
}
`,
  },
  {
    name: 'package.json',
    path: 'package.json',
    language: 'json',
    content: `{
  "name": "personal-notes",
  "private": true,
  "version": "1.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build"
  },
  "dependencies": {
    "lucide-react": "^0.475.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  }
}`,
  },
];

interface CodeEditorViewProps {
  onRunPreview?: () => void;
  onOpenTerminal?: () => void;
  files?: CodeFile[];
}

export function CodeEditorView({ onRunPreview, onOpenTerminal, files }: CodeEditorViewProps) {
  const [showSample, setShowSample] = useState(false);
  const fileList = files && files.length > 0 ? files : showSample ? SAMPLE_FILES : [];
  const [activeFileIndex, setActiveFileIndex] = useState(0);
  const [copied, setCopied] = useState(false);
  const currentFile = fileList[activeFileIndex] || fileList[0];

  const handleCopy = () => {
    if (!currentFile) return;
    navigator.clipboard.writeText(currentFile.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const lines = currentFile ? currentFile.content.split('\n') : [];

  if (!currentFile) {
    return (
      <div className="h-full flex flex-col items-center justify-center gap-2 bg-slate-50/70 text-slate-500 rounded-xl border border-slate-200 p-8 text-center font-sans">
        <FileCode className="w-8 h-8 text-slate-300" />
        <div className="text-sm font-semibold text-slate-700">No generated code yet</div>
        <p className="text-xs max-w-sm leading-relaxed">
          Ask the agent to build a project or module, or open a workspace file from the File Explorer.
        </p>
        <button
          type="button"
          onClick={() => setShowSample(true)}
          className="mt-2 px-3 py-1.5 rounded-lg text-xs font-medium text-slate-600 bg-white border border-slate-200 hover:bg-slate-100 transition-colors"
        >
          Load example project (demo)
        </button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-slate-50/70 text-slate-800 rounded-xl overflow-hidden border border-slate-200 font-mono text-[13px] select-text">
      {/* File Tabs & Actions */}
      <div className="flex items-center justify-between bg-white px-2 border-b border-slate-200 shrink-0">
        <div className="flex items-center overflow-x-auto scrollbar-none">
          {fileList.map((file, idx) => {
            const isActive = idx === activeFileIndex;
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
            onClick={handleCopy}
            className="flex items-center gap-1 px-2.5 py-1 rounded hover:bg-slate-100 text-slate-600 text-xs font-sans transition-colors"
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

      {/* Code Area */}
      <div className="flex-1 overflow-auto bg-slate-50/50 p-3 leading-relaxed">
        <table className="w-full border-collapse">
          <tbody>
            {lines.map((line, i) => (
              <tr key={i} className="hover:bg-blue-50/40">
                <td className="w-9 text-right pr-4 select-none text-slate-400 text-xs align-top">
                  {i + 1}
                </td>
                <td className="whitespace-pre overflow-x-visible align-top font-mono">
                  {renderSyntaxLine(line)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Editor Footer */}
      <div className="h-6 bg-white border-t border-slate-200 px-3 flex items-center justify-between text-[11px] text-slate-500 shrink-0 font-sans">
        <span className="truncate">{currentFile.path}</span>
        <div className="flex items-center gap-3">
          <span className="uppercase">{currentFile.language}</span>
          <span>UTF-8</span>
        </div>
      </div>
    </div>
  );
}

function renderSyntaxLine(line: string) {
  if (line.trim().startsWith('//')) {
    return <span className="text-emerald-600 italic">{line}</span>;
  }

  const tokens = line.split(/(\b(?:import|from|export|default|function|const|let|return|interface|type)\b|'[^']*'|"[^"]*"|`[^`]*`)/g);

  return (
    <span>
      {tokens.map((token, idx) => {
        if (/^(import|from|export|default|function|const|let|return|interface|type)$/.test(token)) {
          return <span key={idx} className="text-blue-700 font-semibold">{token}</span>;
        }
        if (/^['"`]/.test(token)) {
          return <span key={idx} className="text-amber-700">{token}</span>;
        }
        if (/^[A-Z][a-zA-Z0-9]+/.test(token)) {
          return <span key={idx} className="text-teal-700 font-medium">{token}</span>;
        }
        return <span key={idx} className="text-slate-700">{token}</span>;
      })}
    </span>
  );
}

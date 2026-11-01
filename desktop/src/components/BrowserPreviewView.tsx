import React, { useState } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  RefreshCw,
  Lock,
  ExternalLink,
  Monitor,
  Smartphone,
  Search,
  Plus,
  BookOpen,
  Tag,
  Clock,
  Trash2,
  SlidersHorizontal,
} from 'lucide-react';

interface NoteItem {
  id: string;
  title: string;
  body: string;
  tag: 'architecture' | 'odon' | 'research';
  createdAt: string;
}

export function BrowserPreviewView() {
  const [url, setUrl] = useState('http://localhost:3000/preview/notes');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [viewportMode, setViewportMode] = useState<'desktop' | 'mobile'>('desktop');
  
  // Interactive notes state
  const [search, setSearch] = useState('');
  const [selectedTag, setSelectedTag] = useState<string>('all');
  const [notes, setNotes] = useState<NoteItem[]>([
    {
      id: '1',
      title: 'React 19 Architecture',
      body: 'Exploring useActionState, useOptimistic, and Server Actions for responsive asynchronous UI state.',
      tag: 'architecture',
      createdAt: '10:15 AM',
    },
    {
      id: '2',
      title: 'ODON Runtime Integration',
      body: 'Autonomous tool loops with risk-tier policy gating and streaming action chips.',
      tag: 'odon',
      createdAt: '10:22 AM',
    },
    {
      id: '3',
      title: 'Local Knowledge Retrieval',
      body: 'High-speed local vector storage backed by SQLite and BM25 indexers.',
      tag: 'research',
      createdAt: '10:24 AM',
    },
  ]);

  const [newTitle, setNewTitle] = useState('');
  const [newBody, setNewBody] = useState('');
  const [newTag, setNewTag] = useState<'architecture' | 'odon' | 'research'>('odon');
  const [isAdding, setIsAdding] = useState(false);

  const handleRefresh = () => {
    setIsRefreshing(true);
    setTimeout(() => setIsRefreshing(false), 500);
  };

  const handleAddNote = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTitle.trim()) return;
    const newNote: NoteItem = {
      id: Date.now().toString(),
      title: newTitle.trim(),
      body: newBody.trim() || 'No description provided.',
      tag: newTag,
      createdAt: 'Just now',
    };
    setNotes([newNote, ...notes]);
    setNewTitle('');
    setNewBody('');
    setIsAdding(false);
  };

  const handleDeleteNote = (id: string) => {
    setNotes(notes.filter((n) => n.id !== id));
  };

  const filteredNotes = notes.filter((n) => {
    const matchesSearch = n.title.toLowerCase().includes(search.toLowerCase()) || n.body.toLowerCase().includes(search.toLowerCase());
    const matchesTag = selectedTag === 'all' || n.tag === selectedTag;
    return matchesSearch && matchesTag;
  });

  return (
    <div className="h-full flex flex-col bg-white rounded-lg overflow-hidden border border-neutral-200 shadow-xs">
      {/* ─────────────────────────────────────────────────────────────
          MINIMALIST BROWSER BAR
      ───────────────────────────────────────────────────────────── */}
      <div className="bg-[#f9f9f9] border-b border-neutral-200 px-3 py-2 flex items-center justify-between gap-2 shrink-0">
        <div className="flex items-center gap-1">
          <button className="p-1 rounded text-neutral-400 hover:text-neutral-700 hover:bg-neutral-200/60 transition-colors" title="Back">
            <ArrowLeft className="w-3.5 h-3.5" />
          </button>
          <button className="p-1 rounded text-neutral-400 hover:text-neutral-700 hover:bg-neutral-200/60 transition-colors" title="Forward">
            <ArrowRight className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleRefresh}
            className={`p-1 rounded text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/60 transition-colors ${
              isRefreshing ? 'animate-spin text-neutral-900' : ''
            }`}
            title="Reload preview"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* URL Bar */}
        <div className="flex-1 max-w-sm bg-white border border-neutral-300 rounded-md px-2.5 py-1 flex items-center gap-1.5 shadow-2xs">
          <Lock className="w-3 h-3 text-neutral-500 shrink-0" />
          <input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            className="flex-1 text-xs font-mono text-neutral-700 outline-none bg-transparent"
          />
          <span className="text-[10px] font-mono text-neutral-500 px-1 py-0.2 rounded bg-neutral-100 shrink-0">
            200 OK
          </span>
        </div>

        {/* Viewport switch & Popout */}
        <div className="flex items-center gap-1">
          <div className="flex items-center bg-neutral-200/70 p-0.5 rounded">
            <button
              onClick={() => setViewportMode('desktop')}
              className={`p-1 rounded transition-all ${viewportMode === 'desktop' ? 'bg-white text-neutral-900 shadow-2xs' : 'text-neutral-500'}`}
              title="Desktop view"
            >
              <Monitor className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => setViewportMode('mobile')}
              className={`p-1 rounded transition-all ${viewportMode === 'mobile' ? 'bg-white text-neutral-900 shadow-2xs' : 'text-neutral-500'}`}
              title="Mobile view"
            >
              <Smartphone className="w-3.5 h-3.5" />
            </button>
          </div>

          <a
            href="http://localhost:3000"
            target="_blank"
            rel="noreferrer"
            className="p-1.5 text-neutral-500 hover:text-neutral-900 hover:bg-neutral-100 rounded transition-colors"
            title="Open in new window"
          >
            <ExternalLink className="w-3.5 h-3.5" />
          </a>
        </div>
      </div>

      {/* ─────────────────────────────────────────────────────────────
          CANVAS VIEWPORT
      ───────────────────────────────────────────────────────────── */}
      <div className={`flex-1 overflow-auto bg-[#f4f4f4] p-4 flex justify-center items-start ${isRefreshing ? 'opacity-40 transition-opacity' : ''}`}>
        <div
          className={`bg-white text-neutral-900 rounded-lg shadow-sm border border-neutral-200 flex flex-col transition-all overflow-hidden ${
            viewportMode === 'mobile' ? 'w-[360px] min-h-[540px]' : 'w-full min-h-full'
          }`}
        >
          {/* Minimalist App Header */}
          <div className="px-5 py-4 border-b border-neutral-100 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="w-7 h-7 rounded-md bg-neutral-900 flex items-center justify-center text-white">
                <BookOpen className="w-3.5 h-3.5" />
              </div>
              <div>
                <h1 className="text-sm font-semibold text-neutral-900 leading-none">Personal Notes</h1>
                <span className="text-[11px] text-neutral-400">React 19 • Local Storage</span>
              </div>
            </div>

            <button
              onClick={() => setIsAdding(!isAdding)}
              className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-neutral-900 hover:bg-neutral-800 text-white text-xs font-medium transition-colors"
            >
              <Plus className="w-3.5 h-3.5" />
              New Note
            </button>
          </div>

          {/* Search & Tags */}
          <div className="p-4 border-b border-neutral-100 bg-neutral-50/50 space-y-3">
            <div className="relative">
              <Search className="w-3.5 h-3.5 text-neutral-400 absolute left-3 top-2.5" />
              <input
                type="text"
                placeholder="Search notes..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="w-full pl-9 pr-3 py-1.5 rounded-md bg-white border border-neutral-200 text-xs text-neutral-900 placeholder-neutral-400 outline-none focus:border-neutral-900 transition-colors shadow-2xs"
              />
            </div>

            <div className="flex items-center gap-1.5 overflow-x-auto scrollbar-none">
              {[
                { id: 'all', label: 'All' },
                { id: 'architecture', label: 'Architecture' },
                { id: 'odon', label: 'ODON' },
                { id: 'research', label: 'Research' },
              ].map((tag) => (
                <button
                  key={tag.id}
                  onClick={() => setSelectedTag(tag.id)}
                  className={`px-2.5 py-1 rounded text-xs transition-colors shrink-0 ${
                    selectedTag === tag.id
                      ? 'bg-neutral-900 text-white font-medium'
                      : 'bg-white text-neutral-600 hover:text-neutral-900 border border-neutral-200'
                  }`}
                >
                  {tag.label}
                </button>
              ))}
            </div>
          </div>

          {/* Add Form */}
          {isAdding && (
            <form onSubmit={handleAddNote} className="p-4 bg-neutral-50 border-b border-neutral-200 space-y-2.5">
              <div className="text-xs font-semibold text-neutral-800">New Note</div>
              <input
                type="text"
                placeholder="Title"
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                required
                className="w-full px-3 py-1.5 rounded-md bg-white border border-neutral-300 text-xs text-neutral-900 outline-none focus:border-neutral-900"
              />
              <textarea
                placeholder="Note body..."
                value={newBody}
                onChange={(e) => setNewBody(e.target.value)}
                rows={2}
                className="w-full px-3 py-1.5 rounded-md bg-white border border-neutral-300 text-xs text-neutral-900 outline-none focus:border-neutral-900 resize-none"
              />
              <div className="flex items-center justify-between pt-1">
                <select
                  value={newTag}
                  onChange={(e) => setNewTag(e.target.value as any)}
                  className="bg-white border border-neutral-300 text-xs text-neutral-700 rounded px-2 py-1 outline-none"
                >
                  <option value="architecture">Architecture</option>
                  <option value="odon">ODON</option>
                  <option value="research">Research</option>
                </select>
                <div className="flex items-center gap-1.5">
                  <button
                    type="button"
                    onClick={() => setIsAdding(false)}
                    className="px-2.5 py-1 rounded text-xs text-neutral-500 hover:text-neutral-800"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="px-3 py-1 rounded bg-neutral-900 hover:bg-neutral-800 text-white text-xs font-medium"
                  >
                    Save
                  </button>
                </div>
              </div>
            </form>
          )}

          {/* Note List */}
          <div className="flex-1 p-4 overflow-y-auto divide-y divide-neutral-100">
            {filteredNotes.length === 0 ? (
              <div className="text-center py-12 text-neutral-400 text-xs">
                No notes found.
              </div>
            ) : (
              filteredNotes.map((note) => (
                <div key={note.id} className="py-3 first:pt-0 last:pb-0 group">
                  <div className="flex items-baseline justify-between mb-1">
                    <h3 className="font-medium text-xs text-neutral-900">{note.title}</h3>
                    <div className="flex items-center gap-2">
                      <span className="text-[10px] text-neutral-400 font-mono">{note.createdAt}</span>
                      <button
                        onClick={() => handleDeleteNote(note.id)}
                        className="text-neutral-300 hover:text-neutral-700 opacity-0 group-hover:opacity-100 transition-opacity"
                        title="Delete note"
                      >
                        <Trash2 className="w-3 h-3" />
                      </button>
                    </div>
                  </div>
                  <p className="text-xs text-neutral-600 leading-relaxed mb-2">{note.body}</p>
                  <span className="inline-block text-[10px] font-mono text-neutral-500 bg-neutral-100 px-2 py-0.5 rounded">
                    #{note.tag}
                  </span>
                </div>
              ))
            )}
          </div>

          {/* Footer status */}
          <div className="px-4 py-2 border-t border-neutral-100 bg-neutral-50 flex items-center justify-between text-[11px] text-neutral-400 font-mono">
            <span>{notes.length} notes stored</span>
            <span>Ready</span>
          </div>
        </div>
      </div>
    </div>
  );
}

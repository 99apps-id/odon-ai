import { useCallback, useEffect, useRef, useState } from 'react';
import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Check,
  FileCode2,
  FilePlus2,
  FolderPlus,
  Folder,
  Loader2,
  Pencil,
  RefreshCw,
  Save,
  Trash2,
  X,
} from 'lucide-react';
import { api, type SftpEntry } from '../api';

interface Props {
  profileId: string;
  /** Shown in the header so the pane says which host it belongs to. */
  hostLabel: string;
  onClose: () => void;
  /** Called after a write so the terminal can be told a file changed. */
  onError?: (message: string) => void;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatMode(mode: string): string {
  return mode || '----';
}

/**
 * Remote file browser with the navigation WinSCP users expect: back, forward,
 * up, a typed path, and an inline editor.
 *
 * History is an explicit stack instead of `window.history` so it never fights
 * the app router; every navigation pushes the path it is leaving.
 */
export default function SftpExplorer({ profileId, hostLabel, onClose, onError }: Props) {
  const [path, setPath] = useState('/');
  const [parent, setParent] = useState('/');
  const [entries, setEntries] = useState<SftpEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [back, setBack] = useState<string[]>([]);
  const [forward, setForward] = useState<string[]>([]);
  const [typedPath, setTypedPath] = useState('/');

  // Editor state: `editing` is the open file, `draft` its editable text.
  const [editing, setEditing] = useState<{ path: string; name: string } | null>(null);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const report = useCallback(
    (message: string) => {
      setError(message);
      onError?.(message);
    },
    [onError]
  );

  const load = useCallback(
    async (target: string) => {
      setLoading(true);
      setError(null);
      try {
        const dir = target || '/';
        const result = await api.sftpList(profileId, dir);
        setEntries(result.entries);
        setPath(result.path);
        setParent(result.parent);
        setTypedPath(result.path);
      } catch (e) {
        report(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    },
    [profileId, report]
  );

  // Open the home directory once per profile.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      try {
        const home = await api.sftpHome(profileId);
        if (cancelled) return;
        setPath(home);
        setTypedPath(home);
        const result = await api.sftpList(profileId, home);
        if (cancelled) return;
        setEntries(result.entries);
        setParent(result.parent);
      } catch (e) {
        if (!cancelled) report(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [profileId, report]);

  const navigate = (target: string) => {
    setBack((b) => [...b, path]);
    setForward([]);
    void load(target);
  };

  const goBack = () => {
    if (!back.length) return;
    const target = back[back.length - 1];
    setBack((b) => b.slice(0, -1));
    setForward((f) => [path, ...f]);
    void load(target);
  };

  const goForward = () => {
    if (!forward.length) return;
    const target = forward[0];
    setForward((f) => f.slice(1));
    setBack((b) => [...b, path]);
    void load(target);
  };

  const goUp = () => {
    if (path === '/') return;
    navigate(parent);
  };

  const openEntry = async (entry: SftpEntry) => {
    if (entry.kind === 'dir') {
      navigate(entry.path);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const file = await api.sftpReadFile(profileId, entry.path);
      setEditing({ path: entry.path, name: entry.name });
      setDraft(file.content);
      setNotice(file.truncated ? 'Opened a truncated copy (file is larger than the editor limit)' : null);
    } catch (e) {
      report(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const saveFile = async () => {
    if (!editing) return;
    setSaving(true);
    setError(null);
    try {
      await api.sftpWriteFile(profileId, editing.path, draft);
      setNotice(`Saved ${editing.name}`);
    } catch (e) {
      report(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const createEntry = async (kind: 'file' | 'dir') => {
    const label = kind === 'dir' ? 'folder' : 'file';
    const name = window.prompt(`New ${label} name in ${path}`);
    if (!name) return;
    try {
      await api.sftpCreate(profileId, path, name, kind);
      await load(path);
    } catch (e) {
      report(e instanceof Error ? e.message : String(e));
    }
  };

  const renameEntry = async (entry: SftpEntry) => {
    const next = window.prompt(`Rename ${entry.name} to`, entry.name);
    if (!next || next === entry.name) return;
    const target = path === '/' ? `/${next}` : `${path}/${next}`;
    try {
      await api.sftpRename(profileId, entry.path, target);
      await load(path);
    } catch (e) {
      report(e instanceof Error ? e.message : String(e));
    }
  };

  const deleteEntry = async (entry: SftpEntry) => {
    if (!window.confirm(`Delete ${entry.name} from the server?`)) return;
    try {
      await api.sftpDelete(profileId, entry.path, entry.kind === 'dir');
      await load(path);
    } catch (e) {
      report(e instanceof Error ? e.message : String(e));
    }
  };

  const editorRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    editorRef.current?.focus();
  }, [editing]);

  return (
    <div className="h-full flex flex-col bg-white overflow-hidden">
      {/* Header: host + close */}
      <div className="px-3 py-2 border-b border-neutral-200 flex items-center justify-between gap-2 shrink-0">
        <div className="min-w-0">
          <div className="text-[11px] font-bold tracking-wide text-neutral-500 uppercase">SFTP</div>
          <div className="text-xs font-semibold text-neutral-900 truncate" title={hostLabel}>
            {hostLabel}
          </div>
        </div>
        <button
          onClick={onClose}
          title="Close the file panel"
          className="p-1.5 rounded-lg text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200 transition-colors cursor-pointer shrink-0"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Navigation: back / forward / up / typed path */}
      <div className="px-2 py-1.5 border-b border-neutral-200 flex items-center gap-1 shrink-0">
        <button
          onClick={goBack}
          disabled={!back.length || loading}
          title="Back"
          className="p-1.5 rounded-md text-neutral-600 hover:text-neutral-900 hover:bg-neutral-200 disabled:opacity-30 disabled:hover:bg-transparent transition-colors cursor-pointer"
        >
          <ArrowLeft className="w-4 h-4" />
        </button>
        <button
          onClick={goForward}
          disabled={!forward.length || loading}
          title="Forward"
          className="p-1.5 rounded-md text-neutral-600 hover:text-neutral-900 hover:bg-neutral-200 disabled:opacity-30 disabled:hover:bg-transparent transition-colors cursor-pointer"
        >
          <ArrowRight className="w-4 h-4" />
        </button>
        <button
          onClick={goUp}
          disabled={path === '/' || loading}
          title="Up one level"
          className="p-1.5 rounded-md text-neutral-600 hover:text-neutral-900 hover:bg-neutral-200 disabled:opacity-30 disabled:hover:bg-transparent transition-colors cursor-pointer"
        >
          <ArrowUp className="w-4 h-4" />
        </button>
        <form
          className="flex-1 min-w-0"
          onSubmit={(e) => {
            e.preventDefault();
            navigate(typedPath);
          }}
        >
          <input
            value={typedPath}
            onChange={(e) => setTypedPath(e.target.value)}
            spellCheck={false}
            className="w-full px-2 py-1 rounded-md border border-neutral-200 bg-neutral-50 font-mono text-[11px] text-neutral-800 focus:outline-none focus:ring-2 focus:ring-blue-500/40 focus:border-blue-400"
          />
        </form>
        <button
          onClick={() => void load(path)}
          disabled={loading}
          title="Refresh"
          className="p-1.5 rounded-md text-neutral-600 hover:text-neutral-900 hover:bg-neutral-200 transition-colors cursor-pointer"
        >
          {loading ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />}
        </button>
      </div>

      {/* Toolbar */}
      <div className="px-2 py-1.5 border-b border-neutral-200 flex items-center gap-1 shrink-0">
        <button
          onClick={() => void createEntry('dir')}
          className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-semibold text-neutral-700 hover:bg-neutral-200 transition-colors cursor-pointer"
        >
          <FolderPlus className="w-3.5 h-3.5" /> Folder
        </button>
        <button
          onClick={() => void createEntry('file')}
          className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-semibold text-neutral-700 hover:bg-neutral-200 transition-colors cursor-pointer"
        >
          <FilePlus2 className="w-3.5 h-3.5" /> File
        </button>
      </div>

      {error && (
        <div className="mx-2 mt-2 px-2 py-1.5 rounded-md bg-clay-50 border border-clay-300 text-clay-800 text-[11px] flex items-start gap-1.5 shrink-0">
          <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
          <span className="break-words">{error}</span>
        </div>
      )}

      {/* Listing */}
      <div className="flex-1 overflow-auto">
        {entries.length === 0 && !loading ? (
          <div className="p-4 text-center text-[11px] text-neutral-500">This folder is empty.</div>
        ) : (
          <ul className="py-1">
            {entries.map((entry) => (
              <li
                key={entry.path}
                className="group px-2 py-1 flex items-center gap-2 hover:bg-neutral-200/70 transition-colors"
              >
                <button
                  onClick={() => void openEntry(entry)}
                  className="flex-1 min-w-0 flex items-center gap-2 text-left cursor-pointer"
                  title={entry.path}
                >
                  {entry.kind === 'dir' ? (
                    <Folder className="w-4 h-4 text-blue-700 shrink-0" />
                  ) : (
                    <FileCode2 className="w-4 h-4 text-neutral-500 shrink-0" />
                  )}
                  <span className="flex-1 truncate text-[11.5px] text-neutral-900">{entry.name}</span>
                  <span className="text-[10px] text-neutral-500 font-mono shrink-0 hidden sm:inline">
                    {formatMode(entry.mode)}
                  </span>
                  <span className="text-[10px] text-neutral-500 font-mono shrink-0 w-14 text-right">
                    {entry.kind === 'dir' ? '—' : formatSize(entry.size)}
                  </span>
                </button>
                <button
                  onClick={() => void renameEntry(entry)}
                  title="Rename"
                  className="opacity-0 group-hover:opacity-100 p-1 rounded text-neutral-500 hover:text-neutral-900 hover:bg-neutral-300 transition-all cursor-pointer"
                >
                  <Pencil className="w-3 h-3" />
                </button>
                <button
                  onClick={() => void deleteEntry(entry)}
                  title="Delete"
                  className="opacity-0 group-hover:opacity-100 p-1 rounded text-clay-600 hover:text-clay-800 hover:bg-clay-100 transition-all cursor-pointer"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Editor */}
      {editing && (
        <div className="border-t border-neutral-200 flex flex-col shrink-0" style={{ height: '45%' }}>
          <div className="px-2 py-1.5 flex items-center justify-between gap-2 bg-neutral-50 border-b border-neutral-200">
            <span className="font-mono text-[11px] text-neutral-700 truncate" title={editing.path}>
              {editing.path}
            </span>
            <div className="flex items-center gap-1 shrink-0">
              {notice && (
                <span className="text-[10px] text-emerald-700 flex items-center gap-1">
                  <Check className="w-3 h-3" /> {notice}
                </span>
              )}
              <button
                onClick={() => void saveFile()}
                disabled={saving}
                className="flex items-center gap-1 px-2 py-1 rounded-md bg-blue-600 hover:bg-blue-700 disabled:opacity-40 text-white text-[11px] font-semibold transition-colors cursor-pointer"
              >
                {saving ? <Loader2 className="w-3 h-3 animate-spin" /> : <Save className="w-3 h-3" />}
                Save
              </button>
              <button
                onClick={() => {
                  setEditing(null);
                  setNotice(null);
                }}
                className="p-1 rounded-md text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200 transition-colors cursor-pointer"
                title="Close the editor"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
          <textarea
            ref={editorRef}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            spellCheck={false}
            className="flex-1 w-full p-2 font-mono text-[11.5px] leading-relaxed text-neutral-900 bg-white resize-none focus:outline-none"
          />
        </div>
      )}
    </div>
  );
}

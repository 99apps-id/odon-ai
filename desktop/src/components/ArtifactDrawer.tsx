import React, { useEffect, useState } from 'react';
import {
  Code2,
  FileText,
  Image as ImageIcon,
  Check,
  Copy,
  Download,
  FolderOpen,
  Bot,
  Zap,
  Radio,
  ExternalLink,
  ChevronRight,
  Database,
  Trash2,
} from 'lucide-react';
import { Artifact, ArtifactPayload } from '../types';

const isDocumentPayload = (payload: ArtifactPayload): payload is Extract<ArtifactPayload, { type: 'document' }> =>
  payload.type === 'document';
import { api } from '../api';
import { MarkdownView } from './MarkdownView';

interface ArtifactDrawerProps {
  artifacts: Artifact[];
  activeArtifactId: string | null;
  onSelectArtifact: (id: string) => void;
  onClose: () => void;
}

export const ArtifactDrawer: React.FC<ArtifactDrawerProps> = ({
  artifacts,
  activeArtifactId,
  onSelectArtifact,
  onClose,
}) => {
  const [activeTab, setActiveTab] = useState<'artifacts' | 'memory' | 'relays'>('artifacts');
  const [selectedFileIdx, setSelectedFileIdx] = useState(0);
  const [copied, setCopied] = useState(false);
  const [memories, setMemories] = useState<any[]>([]);
  const [memoryLoading, setMemoryLoading] = useState(false);
  const [relayStatus, setRelayStatus] = useState<any>(null);

  const selectedArtifact =
    artifacts.find((a) => a.id === activeArtifactId) || artifacts[0];

  useEffect(() => {
    if (activeTab !== 'memory') return;
    let cancelled = false;
    setMemoryLoading(true);
    api
      .getMemories()
      .then((items) => {
        if (!cancelled) setMemories(Array.isArray(items) ? items : []);
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setMemoryLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeTab]);

  useEffect(() => {
    if (activeTab !== 'relays') return;
    let cancelled = false;
    api
      .getStatus()
      .then((status) => {
        if (!cancelled) setRelayStatus(status);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [activeTab]);

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleForgetMemory = async (key: string) => {
    try {
      await api.deleteMemory(key);
      setMemories((prev) => prev.filter((item) => item.key !== key));
    } catch {
      // Keep the row when the daemon refuses the delete.
    }
  };

  return (
    <aside className="w-[420px] shrink-0 border-l border-neutral-200 dark:border-neutral-800 bg-white dark:bg-[#181818] flex flex-col h-full overflow-hidden select-text">
      {/* Top Header & Tabs */}
      <div className="border-b border-neutral-200 dark:border-neutral-800 px-4 py-3 flex items-center justify-between">
        <div className="flex items-center gap-1 bg-neutral-100 dark:bg-neutral-800 p-0.5 rounded-lg text-[12px] font-medium text-neutral-600 dark:text-neutral-400">
          <button
            onClick={() => setActiveTab('artifacts')}
            className={`px-3 py-1 rounded-md transition-colors ${
              activeTab === 'artifacts'
                ? 'bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 font-semibold shadow-2xs'
                : 'hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            Artifacts ({artifacts.length})
          </button>
          <button
            onClick={() => setActiveTab('memory')}
            className={`px-3 py-1 rounded-md transition-colors ${
              activeTab === 'memory'
                ? 'bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 font-semibold shadow-2xs'
                : 'hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            Memory
          </button>
          <button
            onClick={() => setActiveTab('relays')}
            className={`px-3 py-1 rounded-md transition-colors ${
              activeTab === 'relays'
                ? 'bg-white dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 font-semibold shadow-2xs'
                : 'hover:text-neutral-900 dark:hover:text-neutral-200'
            }`}
          >
            Relays
          </button>
        </div>

        <button
          onClick={onClose}
          className="text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 p-1 rounded-md"
          title="Close panel"
        >
          <ChevronRight className="w-4 h-4" />
        </button>
      </div>

      {/* Content Body */}
      <div className="flex-1 overflow-y-auto p-4">
        {activeTab === 'artifacts' && (
          <div className="space-y-4">
            {/* Artifacts Pill List */}
            {artifacts.length > 1 && (
              <div className="flex items-center gap-1.5 overflow-x-auto pb-1">
                {artifacts.map((art) => {
                  const isSelected = selectedArtifact?.id === art.id;
                  const Icon =
                    art.kind === 'code' ? Code2 : art.kind === 'document' ? FileText : ImageIcon;
                  return (
                    <button
                      key={art.id}
                      onClick={() => {
                        onSelectArtifact(art.id);
                        setSelectedFileIdx(0);
                      }}
                      className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[12px] font-medium shrink-0 border transition-all ${
                        isSelected
                          ? 'border-blue-600 bg-blue-600 text-white shadow-2xs'
                          : 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-neutral-700 dark:text-neutral-300 hover:bg-neutral-50 dark:hover:bg-neutral-800'
                      }`}
                    >
                      <Icon className="w-3.5 h-3.5" />
                      <span className="truncate max-w-[120px]">{art.title}</span>
                    </button>
                  );
                })}
              </div>
            )}

            {/* Active Artifact Content */}
            {selectedArtifact ? (
              <div className="border border-neutral-200 dark:border-neutral-800 rounded-xl bg-neutral-50/50 dark:bg-neutral-900/50 p-4 space-y-4">
                <div>
                  <div className="flex items-center justify-between">
                    <span className="text-[11px] font-semibold text-neutral-500 uppercase tracking-wider">
                      {selectedArtifact.kind} artifact
                    </span>
                    <span className="text-[11px] text-neutral-400">
                      {new Date(selectedArtifact.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                    </span>
                  </div>
                  <h3 className="text-[15px] font-semibold text-neutral-900 dark:text-neutral-100 mt-1">
                    {selectedArtifact.title}
                  </h3>
                  <p className="text-[12.5px] text-neutral-600 dark:text-neutral-400 mt-0.5">
                    {selectedArtifact.summary}
                  </p>
                </div>

                {/* Code Artifact Payload */}
                {selectedArtifact.payload.type === 'code' && (() => {
                  const codeData = selectedArtifact.payload.data;
                  const currentFile = codeData.files[selectedFileIdx];
                  return (
                    <div className="space-y-3">
                      {/* File Tabs */}
                      {codeData.files.length > 0 && (
                        <div className="flex items-center gap-1 border-b border-neutral-200 dark:border-neutral-800 pb-1.5 overflow-x-auto">
                          {codeData.files.map((f, i) => (
                            <button
                              key={i}
                              onClick={() => setSelectedFileIdx(i)}
                              className={`px-2.5 py-1 rounded text-[11.5px] font-mono transition-colors ${
                                selectedFileIdx === i
                                  ? 'bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-semibold'
                                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200'
                              }`}
                            >
                              {f.path}
                            </button>
                          ))}
                        </div>
                      )}

                      {/* File Code Viewer */}
                      {currentFile && (
                        <div className="rounded-lg overflow-hidden border border-neutral-200 dark:border-neutral-800 bg-[#1e1e1e]">
                          <div className="flex items-center justify-between px-3 py-1.5 bg-[#252526] border-b border-neutral-800 text-[11px] font-mono text-neutral-400">
                            <span>{currentFile.path}</span>
                            <button
                              onClick={() => handleCopy(currentFile.content)}
                              className="flex items-center gap-1 hover:text-white"
                            >
                              {copied ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
                              <span>{copied ? 'Copied' : 'Copy'}</span>
                            </button>
                          </div>
                          <pre className="p-3 text-[12px] font-mono text-neutral-200 overflow-x-auto max-h-[360px]">
                            <code>{currentFile.content}</code>
                          </pre>
                        </div>
                      )}
                    </div>
                  );
                })()}

                {/* Document Artifact Payload */}
                {selectedArtifact.payload.type === 'document' && (() => {
                  const markdown = selectedArtifact.payload.data.markdown;
                  return (
                    <div className="space-y-2">
                      <div className="border border-neutral-200 dark:border-neutral-800 rounded-lg p-3 bg-white dark:bg-neutral-900 max-h-[420px] overflow-y-auto">
                        <MarkdownView content={markdown} />
                      </div>
                      <button
                        onClick={() =>
                          window.dispatchEvent(
                            new CustomEvent('odon:open-browser', {
                              detail: { markdown },
                            }),
                          )
                        }
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium transition-colors shadow-2xs cursor-pointer"
                      >
                        <FileText className="w-3 h-3" />
                        Preview in Browser
                      </button>
                    </div>
                  );
                })()}

                {/* Image Artifact Payload */}
                {selectedArtifact.payload.type === 'image' && (() => {
                  const imgData = selectedArtifact.payload.data;
                  const rawUrl = imgData.preview_url || '';
                  const fullSrc = rawUrl.startsWith('/api/') ? api.authorizeUrl(rawUrl) : rawUrl;
                  const fileName = selectedArtifact.title.endsWith('.png') ? selectedArtifact.title : `${selectedArtifact.title}.png`;

                  return (
                    <div className="space-y-3">
                      <div className="rounded-xl overflow-hidden border border-neutral-200 dark:border-neutral-800 bg-neutral-950 aspect-video flex items-center justify-center">
                        {fullSrc ? (
                          <img
                            src={fullSrc}
                            alt={imgData.prompt}
                            referrerPolicy="no-referrer"
                            className="w-full h-full object-contain"
                          />
                        ) : (
                          <ImageIcon className="w-8 h-8 text-neutral-600" />
                        )}
                      </div>
                      <div className="text-[11.5px] text-neutral-500 flex items-center justify-between">
                        <span>{imgData.width} Ã— {imgData.height}</span>
                        <span>Model: {imgData.model}</span>
                      </div>
                      {imgData.file_path && (
                        <div className="text-[11px] font-mono text-neutral-500 bg-neutral-100 dark:bg-neutral-800/80 p-2 rounded-lg truncate border border-neutral-200/60 dark:border-neutral-700/60" title={imgData.file_path}>
                          ðŸ“ {imgData.file_path}
                        </div>
                      )}
                      <div className="flex items-center gap-2 pt-1">
                        <button
                          onClick={async () => {
                            if (!fullSrc) return;
                            try {
                              const resp = await api.fetchApi(fullSrc);
                              const blob = await resp.blob();
                              const url = window.URL.createObjectURL(blob);
                              const a = document.createElement('a');
                              a.href = url;
                              a.download = fileName;
                              document.body.appendChild(a);
                              a.click();
                              window.URL.revokeObjectURL(url);
                              document.body.removeChild(a);
                            } catch {
                              window.open(fullSrc, '_blank');
                            }
                          }}
                          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium transition-colors shadow-2xs cursor-pointer"
                        >
                          <Download className="w-3.5 h-3.5" />
                          Save Image
                        </button>
                        <button
                          onClick={() => fullSrc && window.open(fullSrc, '_blank')}
                          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-700 dark:text-neutral-200 hover:bg-neutral-50 text-xs font-medium transition-colors cursor-pointer"
                        >
                          <ExternalLink className="w-3.5 h-3.5 text-neutral-500" />
                          Open Full
                        </button>
                      </div>
                    </div>
                  );
                })()}
              </div>
            ) : (
              <div className="text-center py-12 text-neutral-400 text-[13px]">
                No artifacts created yet for this conversation.
              </div>
            )}
          </div>
        )}

        {/* Memory Tab */}
        {activeTab === 'memory' && (
          <div className="space-y-4">
            <div className="flex items-center gap-2">
              <Database className="w-4 h-4 text-neutral-500" />
              <h4 className="text-[13px] font-semibold text-neutral-900 dark:text-neutral-100">
                Agent Long-Term Memory
              </h4>
            </div>
            <p className="text-[12px] text-neutral-500 leading-relaxed">
              Live entries from the agent's SQLite memory. These are the facts and preferences it recalls across sessions.
            </p>
            {memoryLoading ? (
              <div className="text-center py-8 text-neutral-400 text-[12px]">Loading memoriesâ€¦</div>
            ) : memories.length === 0 ? (
              <div className="text-center py-8 text-neutral-400 text-[12px]">
                No long-term memories yet. Ask the agent to remember something, or use the memory_store tool.
              </div>
            ) : (
              <div className="space-y-2 pt-2">
                {memories.map((memory) => (
                  <div key={memory.key} className="p-2.5 rounded-lg border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 text-[12px]">
                    <div className="flex items-center justify-between gap-2 font-mono text-[11px] text-neutral-500 mb-1">
                      <span className="truncate">{memory.key}</span>
                      <div className="flex items-center gap-1.5 shrink-0">
                        <span className="px-1.5 py-0.2 rounded bg-neutral-200 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300">
                          {memory.category || 'general'}
                        </span>
                        <button
                          type="button"
                          onClick={() => handleForgetMemory(memory.key)}
                          title="Forget this memory"
                          className="p-0.5 rounded text-neutral-400 hover:text-clay-500"
                        >
                          <Trash2 className="w-3 h-3" />
                        </button>
                      </div>
                    </div>
                    <div className="text-neutral-800 dark:text-neutral-200 whitespace-pre-wrap break-words">{memory.value}</div>
                    {memory.updated_at && (
                      <div className="text-[10px] text-neutral-400 mt-1">
                        {new Date(memory.updated_at).toLocaleString()}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Relays Tab */}
        {activeTab === 'relays' && (
          <div className="space-y-4">
            <div className="flex items-center gap-2">
              <Radio className="w-4 h-4 text-emerald-500" />
              <h4 className="text-[13px] font-semibold text-neutral-900 dark:text-neutral-100">
                Universal Relays & Connectors
              </h4>
            </div>
            <p className="text-[12px] text-neutral-500 leading-relaxed">
              Live status from the daemon. Configure tokens, allowlists, and QR pairing in Settings â†’ Relays.
            </p>
            <div className="space-y-2 pt-2">
              {[
                {
                  name: 'Desktop IPC & HTTP Server',
                  port: '127.0.0.1:4096',
                  active: relayStatus ? !!relayStatus.relays?.desktop_api : true,
                  desc: 'Daemon serving REST and real-time SSE',
                },
                {
                  name: 'CLI Channel',
                  port: 'odon chat',
                  active: relayStatus ? !!relayStatus.relays?.cli : true,
                  desc: 'Terminal interactive REPL and headless runs',
                },
                {
                  name: 'Telegram Bot Relay',
                  port: 'Long-polling',
                  active: !!relayStatus?.relays?.telegram,
                  desc: relayStatus?.relays?.telegram
                    ? 'Connected and polling for messages'
                    : 'Not connected â€” configure the bot token in Settings â†’ Relays',
                },
                {
                  name: 'WhatsApp Relay',
                  port: 'Web bridge / Cloud API',
                  active: !!relayStatus?.relays?.whatsapp,
                  desc: relayStatus?.relays?.whatsapp
                    ? 'Connected â€” QR/pairing session or Cloud API webhook is live'
                    : 'Not connected â€” pair a device or configure Cloud API in Settings â†’ Relays',
                },
              ].map((relay, index) => (
                <div key={index} className="p-3 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 space-y-1">
                  <div className="flex items-center justify-between">
                    <span className="text-[12.5px] font-semibold text-neutral-900 dark:text-neutral-100">{relay.name}</span>
                    <span className={`inline-flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded-full ${
                      relay.active
                        ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950 dark:text-emerald-400'
                        : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
                    }`}>
                      <span className={`w-1.5 h-1.5 rounded-full ${relay.active ? 'bg-emerald-500' : 'bg-neutral-400'}`}></span>
                      {relay.active ? 'Active' : 'Standby'}
                    </span>
                  </div>
                  <div className="text-[11.5px] text-neutral-500">{relay.desc}</div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </aside>
  );
};

import React, { useState, useEffect } from 'react';
import {
  Layers,
  Plus,
  RefreshCw,
  Trash2,
  ToggleLeft,
  ToggleRight,
  ExternalLink,
  ChevronDown,
  ChevronRight,
  Terminal,
  Server,
  Sparkles,
  Check,
  XCircle,
} from 'lucide-react';
import { api } from '../api';

interface McpTool {
  name: string;
  description: string;
  input_schema?: Record<string, unknown>;
}

interface McpServer {
  id: string;
  name: string;
  transport: string;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  enabled: boolean;
  tools?: McpTool[];
}

export const McpView: React.FC = () => {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [expandedServerId, setExpandedServerId] = useState<string | null>(null);
  const [isAddOpen, setIsAddOpen] = useState<boolean>(false);

  // Form states
  const [newId, setNewId] = useState<string>('');
  const [newName, setNewName] = useState<string>('');
  const [newTransport, setNewTransport] = useState<'stdio' | 'sse'>('stdio');
  const [newCommand, setNewCommand] = useState<string>('npx');
  const [newArgs, setNewArgs] = useState<string>('');
  const [newUrl, setNewUrl] = useState<string>('');

  const loadServers = async () => {
    setIsLoading(true);
    try {
      const data = await api.getMcpServers();
      setServers(Array.isArray(data) ? data : []);
    } catch (err) {
      console.error('Failed to load MCP servers:', err);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadServers();
  }, []);

  const handleToggle = async (id: string) => {
    try {
      await api.toggleMcpServer(id);
      loadServers();
    } catch (err) {
      console.error('Failed to toggle MCP server:', err);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await api.deleteMcpServer(id);
      setServers((prev) => prev.filter((s) => s.id !== id));
    } catch (err) {
      console.error('Failed to delete MCP server:', err);
    }
  };

  const handleAddSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newId.trim() || !newName.trim()) return;

    const payload: McpServer = {
      id: newId.trim().toLowerCase(),
      name: newName.trim(),
      transport: newTransport,
      command: newTransport === 'stdio' ? newCommand : undefined,
      args: newTransport === 'stdio' && newArgs ? newArgs.split(' ') : undefined,
      url: newTransport === 'sse' ? newUrl : undefined,
      enabled: true,
      tools: [],
    };

    try {
      await api.saveMcpServer(payload);
      setIsAddOpen(false);
      setNewId('');
      setNewName('');
      setNewArgs('');
      setNewUrl('');
      loadServers();
    } catch (err) {
      console.error('Failed to save MCP server:', err);
    }
  };

  const handleAddPreset = async (preset: { id: string; name: string; cmd: string; args: string[] }) => {
    const payload: McpServer = {
      id: preset.id,
      name: preset.name,
      transport: 'stdio',
      command: preset.cmd,
      args: preset.args,
      enabled: true,
      tools: [],
    };
    try {
      await api.saveMcpServer(payload);
      loadServers();
    } catch (err) {
      console.error('Failed to install preset:', err);
    }
  };

  return (
    <div className="flex flex-col h-full bg-white dark:bg-neutral-900 overflow-hidden font-sans">
      {/* Top Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/40 shrink-0">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-2xl bg-blue-100 dark:bg-blue-950/80 text-blue-600 dark:text-blue-400 flex items-center justify-center shadow-xs">
            <Layers className="w-5 h-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-base font-bold text-neutral-900 dark:text-neutral-100">
                Model Context Protocol (MCP) Hub
              </h2>
              <span className="text-xs px-2.5 py-0.5 rounded-full bg-blue-100 text-blue-700 dark:bg-blue-950 dark:text-blue-300 font-semibold font-mono">
                {servers.filter((s) => s.enabled).length} Connected
              </span>
            </div>
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Discover external tool servers, stdio/SSE connectors, and custom schema wiring
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={loadServers}
            title="Refresh MCP servers"
            className="p-2 rounded-xl text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors"
          >
            <RefreshCw className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`} />
          </button>
          <button
            type="button"
            onClick={() => setIsAddOpen(true)}
            className="inline-flex items-center gap-1.5 px-4 py-2 rounded-xl text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 transition-all shadow-2xs"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Add Server</span>
          </button>
        </div>
      </div>

      {/* Main Content */}
      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        {/* Preset Catalog Section */}
        <div className="space-y-3">
          <div className="text-xs font-bold text-neutral-800 dark:text-neutral-200 uppercase tracking-wider">
            Popular 1-Click MCP Presets
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            {[
              { id: 'github', name: 'GitHub MCP', cmd: 'npx', args: ['-y', '@modelcontextprotocol/server-github'] },
              { id: 'memory', name: 'Memory Graph MCP', cmd: 'npx', args: ['-y', '@modelcontextprotocol/server-memory'] },
              { id: 'postgres', name: 'PostgreSQL MCP', cmd: 'npx', args: ['-y', '@modelcontextprotocol/server-postgres'] },
              { id: 'filesystem', name: 'Filesystem MCP', cmd: 'npx', args: ['-y', '@modelcontextprotocol/server-filesystem', '.'] },
              { id: 'puppeteer', name: 'Puppeteer Browser MCP', cmd: 'npx', args: ['-y', '@modelcontextprotocol/server-puppeteer'] },
              { id: 'brave', name: 'Brave Search MCP', cmd: 'npx', args: ['-y', '@modelcontextprotocol/server-brave-search'] },
            ].map((preset) => {
              const isConfigured = servers.some((s) => s.id === preset.id);
              return (
                <div
                  key={preset.id}
                  className="p-3.5 rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-950/30 flex items-center justify-between gap-3 shadow-2xs"
                >
                  <div>
                    <div className="text-xs font-bold text-neutral-900 dark:text-neutral-100">{preset.name}</div>
                    <div className="text-[10px] text-neutral-400 font-mono">stdio: {preset.cmd}</div>
                  </div>
                  <button
                    type="button"
                    onClick={() => handleAddPreset(preset)}
                    disabled={isConfigured}
                    className={`px-3 py-1 rounded-xl text-xs font-semibold transition-all ${
                      isConfigured
                        ? 'bg-neutral-100 dark:bg-neutral-800 text-neutral-400 cursor-default'
                        : 'bg-blue-600 text-white hover:bg-blue-500 shadow-2xs'
                    }`}
                  >
                    {isConfigured ? 'Installed' : '+ Add'}
                  </button>
                </div>
              );
            })}
          </div>
        </div>

        {/* Configured Servers Section */}
        <div className="space-y-3 pt-4 border-t border-neutral-200 dark:border-neutral-800">
          <div className="text-xs font-bold text-neutral-800 dark:text-neutral-200 uppercase tracking-wider">
            Active MCP Servers ({servers.length})
          </div>

          <div className="space-y-3">
            {servers.map((server) => {
              const isExpanded = expandedServerId === server.id;
              const toolsCount = server.tools?.length || 0;
              return (
                <div
                  key={server.id}
                  className={`p-4 rounded-2xl border transition-all shadow-2xs space-y-3 ${
                    server.enabled
                      ? 'border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900'
                      : 'border-neutral-200/60 dark:border-neutral-800/60 bg-neutral-50/50 dark:bg-neutral-950/40 opacity-70'
                  }`}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div
                      onClick={() => setExpandedServerId(isExpanded ? null : server.id)}
                      className="flex items-center gap-3 cursor-pointer flex-1"
                    >
                      {isExpanded ? <ChevronDown className="w-4 h-4 text-neutral-400" /> : <ChevronRight className="w-4 h-4 text-neutral-400" />}
                      <div className="w-8 h-8 rounded-xl bg-blue-100 dark:bg-blue-950/80 text-blue-600 flex items-center justify-center shrink-0">
                        <Server className="w-4 h-4" />
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="text-xs font-bold text-neutral-900 dark:text-neutral-100">{server.name}</span>
                          <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300">
                            {server.transport}
                          </span>
                        </div>
                        <div className="text-[11px] text-neutral-500 font-mono mt-0.5">
                          {toolsCount} discovered tools
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() => handleToggle(server.id)}
                        className="text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
                      >
                        {server.enabled ? (
                          <ToggleRight className="w-6 h-6 text-blue-600 dark:text-blue-400" />
                        ) : (
                          <ToggleLeft className="w-6 h-6 text-neutral-300 dark:text-neutral-600" />
                        )}
                      </button>
                      <button
                        type="button"
                        onClick={() => handleDelete(server.id)}
                        title="Delete server"
                        className="p-1.5 rounded-lg text-neutral-400 hover:text-rose-500 hover:bg-rose-50 dark:hover:bg-rose-950/40 transition-colors"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>

                  {isExpanded && (
                    <div className="pt-3 border-t border-neutral-100 dark:border-neutral-800 space-y-2">
                      <div className="text-[11px] font-bold text-neutral-700 dark:text-neutral-300">Exposed Tools:</div>
                      {toolsCount === 0 ? (
                        <p className="text-xs text-neutral-400 italic">No tools discovered for this server yet.</p>
                      ) : (
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
                          {server.tools?.map((tool) => (
                            <div
                              key={tool.name}
                              className="p-2.5 rounded-xl border border-neutral-200/70 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-950 font-mono text-xs space-y-1"
                            >
                              <div className="font-bold text-blue-600 dark:text-blue-400">{tool.name}</div>
                              <p className="text-[11px] text-neutral-500 font-sans line-clamp-2">{tool.description}</p>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Add Custom Modal */}
      {isAddOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-xs">
          <form
            onSubmit={handleAddSubmit}
            className="flex flex-col w-full max-w-lg bg-white dark:bg-neutral-900 rounded-3xl shadow-2xl border border-neutral-200 dark:border-neutral-800 p-6 space-y-4"
          >
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-bold text-neutral-900 dark:text-neutral-100">Register Custom MCP Server</h3>
              <button
                type="button"
                onClick={() => setIsAddOpen(false)}
                className="p-1 rounded-lg text-neutral-400 hover:text-neutral-700"
              >
                <XCircle className="w-4 h-4" />
              </button>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Server ID</label>
                <input
                  type="text"
                  value={newId}
                  onChange={(e) => setNewId(e.target.value)}
                  placeholder="e.g. custom-db"
                  className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 font-mono"
                  required
                />
              </div>
              <div>
                <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Display Name</label>
                <input
                  type="text"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder="e.g. My Database Server"
                  className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100"
                  required
                />
              </div>
            </div>

            <div>
              <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Transport</label>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setNewTransport('stdio')}
                  className={`flex-1 py-1.5 rounded-xl text-xs font-semibold border ${
                    newTransport === 'stdio'
                      ? 'bg-blue-600 text-white border-transparent shadow-xs'
                      : 'bg-neutral-50 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 border-neutral-200 dark:border-neutral-700'
                  }`}
                >
                  stdio (Local CLI Process)
                </button>
                <button
                  type="button"
                  onClick={() => setNewTransport('sse')}
                  className={`flex-1 py-1.5 rounded-xl text-xs font-semibold border ${
                    newTransport === 'sse'
                      ? 'bg-blue-600 text-white border-transparent shadow-xs'
                      : 'bg-neutral-50 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 border-neutral-200 dark:border-neutral-700'
                  }`}
                >
                  SSE (Remote Web Endpoint)
                </button>
              </div>
            </div>

            {newTransport === 'stdio' ? (
              <>
                <div>
                  <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Command</label>
                  <input
                    type="text"
                    value={newCommand}
                    onChange={(e) => setNewCommand(e.target.value)}
                    placeholder="e.g. npx, python, uvx"
                    className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 font-mono"
                    required
                  />
                </div>
                <div>
                  <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">Arguments (space separated)</label>
                  <input
                    type="text"
                    value={newArgs}
                    onChange={(e) => setNewArgs(e.target.value)}
                    placeholder="e.g. -y @modelcontextprotocol/server-github"
                    className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 font-mono"
                  />
                </div>
              </>
            ) : (
              <div>
                <label className="text-xs font-semibold text-neutral-700 dark:text-neutral-300 block mb-1">SSE Endpoint URL</label>
                <input
                  type="url"
                  value={newUrl}
                  onChange={(e) => setNewUrl(e.target.value)}
                  placeholder="https://my-mcp-server.com/sse"
                  className="w-full px-3 py-2 text-xs rounded-xl border border-neutral-300 dark:border-neutral-700 bg-neutral-50 dark:bg-neutral-950 text-neutral-900 dark:text-neutral-100 font-mono"
                  required
                />
              </div>
            )}

            <div className="flex items-center justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setIsAddOpen(false)}
                className="px-4 py-2 text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 rounded-xl"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="px-4 py-2 text-xs font-bold text-white bg-blue-600 hover:bg-blue-500 rounded-xl transition-colors shadow-2xs"
              >
                Register Server
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
};


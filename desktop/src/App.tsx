import React, { useState, useEffect, useRef } from 'react';
import {
  MessageSquare,
  Code2,
  Image as ImageIcon,
  FileText,
  Folder,
  Bot,
  Share2,
  Settings,
  Plus,
  Search,
  Bell,
  PanelRightClose,
  PanelRightOpen,
  ChevronDown,
  Paperclip,
  Globe,
  ArrowUp,
  ExternalLink,
  Radio,
  Activity,
  ChevronRight,
  Video,
  Minus,
  Square,
  X,
  Sparkles,
  Layers,
  ArrowLeft,
  CheckCircle2,
  Check,
  Download,
  Copy,
  Terminal,
  Loader2,
  Trash2,
  GitPullRequest,
  Cpu,
  ShieldCheck,
  ShieldAlert,
  FolderTree,
  Server,
} from 'lucide-react';
import { CatLogo } from './components/CatLogo';
import { CodeEditorView } from './components/CodeEditorView';
import { BrowserPreviewView } from './components/BrowserPreviewView';
import { TerminalView } from './components/TerminalView';
import { FileExplorerView } from './components/FileExplorerView';
import { SshTerminalView } from './components/SshTerminalView';
import { SettingsModal } from './components/SettingsModal';
import { MarkdownView } from './components/MarkdownView';
import { ApprovalCard } from './components/ApprovalCard';
import { ApprovalCenterModal } from './components/ApprovalCenterModal';
import { GitHubView } from './components/GitHubView';
import { AutomationsView } from './components/AutomationsView';
import { McpView } from './components/McpView';
import { api } from './api';
import { Session, Message, Artifact, SystemStatus, ProviderItem, ModelItem, FALLBACK_PROVIDERS, getCustomOpenAiModels, saveCustomOpenAiModels } from './types';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  showAppCard?: boolean;
  thought?: string;
  toolCalls?: string[];
}

// Model lists are now dynamically populated from the active provider chosen in Settings.

const REGISTERED_TOOLS = [
  // File System & Termigo Standard
  { name: 'read_file', category: 'File System', tier: 'Tier 0', desc: 'Read file contents inside workspace' },
  { name: 'write_file', category: 'File System', tier: 'Tier 1', desc: 'Create or overwrite workspace file' },
  { name: 'edit', category: 'File System', tier: 'Tier 1', desc: 'Surgical single-string replacement' },
  { name: 'multi_edit', category: 'File System', tier: 'Tier 1', desc: 'Atomic multi-replacement file edit' },
  { name: 'list_directory', category: 'File System', tier: 'Tier 0', desc: 'List immediate directory entries' },
  { name: 'create_directory', category: 'File System', tier: 'Tier 1', desc: 'Create directory recursively' },
  { name: 'delete_file', category: 'File System', tier: 'Tier 2', desc: 'Delete workspace file (irreversible)' },
  { name: 'move_file', category: 'File System', tier: 'Tier 1', desc: 'Move or rename file/folder' },
  { name: 'copy_file', category: 'File System', tier: 'Tier 1', desc: 'Copy file within workspace' },
  { name: 'grep', category: 'File System', tier: 'Tier 0', desc: 'Search file contents using pattern' },
  { name: 'glob', category: 'File System', tier: 'Tier 0', desc: 'Find files matching pattern' },
  { name: 'fs_read', category: 'File System', tier: 'Tier 0', desc: 'Read file path' },
  { name: 'fs_write', category: 'File System', tier: 'Tier 1', desc: 'Write content to path' },
  { name: 'fs_edit', category: 'File System', tier: 'Tier 1', desc: 'Find and replace target' },
  { name: 'fs_list', category: 'File System', tier: 'Tier 0', desc: 'List directory entries' },
  { name: 'fs_search', category: 'File System', tier: 'Tier 0', desc: 'Recursive text search' },
  { name: 'fs_delete', category: 'File System', tier: 'Tier 2', desc: 'Remove file from disk' },

  // Planning & Harness
  { name: 'todo_write', category: 'Harness', tier: 'Tier 0', desc: 'Set task breakdown & milestones' },
  { name: 'todo_read', category: 'Harness', tier: 'Tier 0', desc: 'Read task status and progress' },
  { name: 'todo_update', category: 'Harness', tier: 'Tier 0', desc: 'Update specific todo status' },
  { name: 'think', category: 'Harness', tier: 'Tier 0', desc: 'Private reasoning scratchpad' },
  { name: 'plan_mode', category: 'Harness', tier: 'Tier 0', desc: 'Toggle read-only plan mode' },
  { name: 'remember', category: 'Harness', tier: 'Tier 1', desc: 'Save durable project fact/gotcha' },

  // Git Tools
  { name: 'git_status', category: 'Git', tier: 'Tier 0', desc: 'Working tree status' },
  { name: 'git_diff', category: 'Git', tier: 'Tier 0', desc: 'Inspect uncommitted git diff' },
  { name: 'git_commit', category: 'Git', tier: 'Tier 1', desc: 'Stage and commit changes' },
  { name: 'git_log', category: 'Git', tier: 'Tier 0', desc: 'View commit history log' },
  { name: 'git_branch', category: 'Git', tier: 'Tier 1', desc: 'List or create git branches' },
  { name: 'git_add', category: 'Git', tier: 'Tier 1', desc: 'Stage specific files or all changes' },
  { name: 'git_checkout', category: 'Git', tier: 'Tier 1', desc: 'Switch branch or restore path' },
  { name: 'git_show', category: 'Git', tier: 'Tier 0', desc: 'Show commit details and diff' },
  { name: 'git_stash', category: 'Git', tier: 'Tier 1', desc: 'Stash dirty working tree state' },
  { name: 'git_stash_pop', category: 'Git', tier: 'Tier 1', desc: 'Apply and drop latest stash' },
  { name: 'git_stash_list', category: 'Git', tier: 'Tier 0', desc: 'List all repository stashes' },
  { name: 'git_pull', category: 'Git', tier: 'Tier 2', desc: 'Pull remote commits' },
  { name: 'git_push', category: 'Git', tier: 'Tier 2', desc: 'Push local commits upstream' },
  { name: 'git_reset', category: 'Git', tier: 'Tier 2', desc: 'Reset HEAD to specified commit' },

  // Shell & Execution
  { name: 'shell_exec', category: 'Execution', tier: 'Tier 2', desc: 'Run PowerShell / sh command' },
  { name: 'bash_run', category: 'Execution', tier: 'Tier 2', desc: 'Foreground command with timeout' },
  { name: 'terminal_exec', category: 'Execution', tier: 'Tier 2', desc: 'Direct terminal command run' },

  // Web & Security Pentest
  { name: 'web_search', category: 'Web & Security', tier: 'Tier 0', desc: 'Web search for docs and CVEs' },
  { name: 'web_fetch', category: 'Web & Security', tier: 'Tier 0', desc: 'Fetch web page text content' },
  { name: 'probe_url', category: 'Web & Security', tier: 'Tier 0', desc: 'Probe HTTP headers and latency' },
  { name: 'http_get', category: 'Web & Security', tier: 'Tier 0', desc: 'Send raw HTTP GET request' },
  { name: 'http_post', category: 'Web & Security', tier: 'Tier 1', desc: 'Send HTTP POST with JSON body' },
  { name: 'dns_lookup', category: 'Web & Security', tier: 'Tier 0', desc: 'Resolve domain hostnames to IP' },
  { name: 'port_scan', category: 'Web & Security', tier: 'Tier 1', desc: 'TCP connect port scan on target' },

  // Browser Control
  { name: 'browser_open', category: 'Browser', tier: 'Tier 1', desc: 'Open URL in system browser or preview' },
  { name: 'browser_navigate', category: 'Browser', tier: 'Tier 0', desc: 'Navigate to URL and extract page content' },
  { name: 'browser_screenshot', category: 'Browser', tier: 'Tier 1', desc: 'Capture screenshot of target web page' },
  { name: 'browser_click', category: 'Browser', tier: 'Tier 1', desc: 'Click web element or link' },

  // Skills System (.odon/skills)
  { name: 'skill_list', category: 'Skills', tier: 'Tier 0', desc: 'List reusable autonomous skills' },
  { name: 'skill_create', category: 'Skills', tier: 'Tier 1', desc: 'Record a new skill procedure in markdown' },
  { name: 'skill_run', category: 'Skills', tier: 'Tier 0', desc: 'Execute an autonomous stored skill' },

  // System & Environment
  { name: 'system_info', category: 'System', tier: 'Tier 0', desc: 'Host OS, CPU, memory details' },
  { name: 'calc_eval', category: 'System', tier: 'Tier 0', desc: 'Safe math expression evaluator' },
  { name: 'env_get', category: 'System', tier: 'Tier 0', desc: 'Read environment variable safely' },
  { name: 'env_list', category: 'System', tier: 'Tier 0', desc: 'List visible environment keys' },
  { name: 'process_list', category: 'System', tier: 'Tier 0', desc: 'List running host processes' },
  { name: 'disk_usage', category: 'System', tier: 'Tier 0', desc: 'Workspace disk usage metrics' },

  // GitHub & MCP
  { name: 'github_get_repo', category: 'Integrations', tier: 'Tier 0', desc: 'GitHub repository metadata' },
  { name: 'github_list_issues', category: 'Integrations', tier: 'Tier 0', desc: 'List repository issues' },
  { name: 'github_create_issue', category: 'Integrations', tier: 'Tier 1', desc: 'Open a new GitHub issue' },
  { name: 'github_list_prs', category: 'Integrations', tier: 'Tier 0', desc: 'List repository pull requests' },
  { name: 'mcp_list_tools', category: 'Integrations', tier: 'Tier 0', desc: 'List connected MCP tools' },
  { name: 'mcp_call', category: 'Integrations', tier: 'Tier 1', desc: 'Call tool on MCP server' },

  // Memory & Artifacts
  { name: 'memory_store', category: 'Memory', tier: 'Tier 0', desc: 'Save long-term note/fact' },
  { name: 'memory_recall', category: 'Memory', tier: 'Tier 0', desc: 'Recall persistent memories' },
  { name: 'artifact_create_code', category: 'Artifacts', tier: 'Tier 0', desc: 'Generate multi-file code app' },
  { name: 'artifact_create_document', category: 'Artifacts', tier: 'Tier 0', desc: 'Generate PRD or report' },
  { name: 'artifact_create_image', category: 'Artifacts', tier: 'Tier 0', desc: 'Synthesize preview image' },
  { name: 'artifact_update', category: 'Artifacts', tier: 'Tier 1', desc: 'Update existing artifact content' },
];

export default function App() {
  // Navigation & View States
  const [activeNav, setActiveNav] = useState('chat');
  const [approvalCenterOpen, setApprovalCenterOpen] = useState(false);
  const [dynamicTools, setDynamicTools] = useState<any[]>([]);
  const [isRightPanelOpen, setIsRightPanelOpen] = useState(true);
  const [rightPanelView, setRightPanelView] = useState<'artifacts' | 'editor' | 'browser' | 'terminal'>('artifacts');
  // Provider & Model States (Synchronized with Settings)
  const [providers, setProviders] = useState<ProviderItem[]>(FALLBACK_PROVIDERS);
  const [activeProviderId, setActiveProviderId] = useState<string>(() => {
    if (typeof window !== 'undefined') {
      return localStorage.getItem('odon_active_provider') || 'openai';
    }
    return 'openai';
  });
  const [selectedModel, setSelectedModel] = useState<string>(() => {
    if (typeof window !== 'undefined') {
      return localStorage.getItem('odon_selected_model') || 'gpt-4o';
    }
    return 'gpt-4o';
  });
  const [inputMessage, setInputMessage] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Menus & Modals
  const [isModelMenuOpen, setIsModelMenuOpen] = useState(false);
  const [isToolsMenuOpen, setIsToolsMenuOpen] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [isBellOpen, setIsBellOpen] = useState(false);
  const [showAllRecent, setShowAllRecent] = useState(false);
  const [relayModalOpen, setRelayModalOpen] = useState<'telegram' | 'whatsapp' | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  interface AttachedFile {
    name: string;
    type: string;
    size: number;
    dataUrl?: string;
    textContent?: string;
    isImage: boolean;
  }
  const [attachedFiles, setAttachedFiles] = useState<AttachedFile[]>([]);

  const [customOpenAiModels, setCustomOpenAiModels] = useState<ModelItem[]>(() => getCustomOpenAiModels());
  const [quickModelInput, setQuickModelInput] = useState('');

  // Active Provider & Model resolution
  const activeProvider = providers.find((p) => p.id === activeProviderId) || providers[0] || FALLBACK_PROVIDERS[0];
  const isOpenAiCompatible = activeProvider?.id === 'openai-compatible' || activeProvider?.id === 'openai_compatible';
  const activeModels = isOpenAiCompatible ? customOpenAiModels : (activeProvider?.models || []);
  const currentModelObj = activeModels.find((m) => m.id === selectedModel || `${activeProvider?.id}::${m.id}` === selectedModel || m.name === selectedModel) || activeModels[0];
  const displayModelName = currentModelObj ? (currentModelObj.name || currentModelObj.id) : (selectedModel || 'Select Model');

  const handleSelectProvider = (providerId: string) => {
    setActiveProviderId(providerId);
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_active_provider', providerId);
    }
    const isComp = providerId === 'openai-compatible' || providerId === 'openai_compatible';
    const prov = providers.find((p) => p.id === providerId);
    const pModels = isComp ? getCustomOpenAiModels() : (prov?.models || []);
    if (pModels.length > 0) {
      const rec = pModels.find((m) => m.recommended) || pModels[0];
      setSelectedModel(rec.id);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_selected_model', rec.id);
      }
    } else if (isComp) {
      setSelectedModel('');
    }
  };

  const handleAddQuickCustomModel = () => {
    const trimmed = quickModelInput.trim();
    if (!trimmed) return;
    const existing = getCustomOpenAiModels();
    let updated = existing;
    if (!existing.some((m) => m.id === trimmed)) {
      updated = [...existing, { id: trimmed, name: trimmed, context: 128000 }];
      saveCustomOpenAiModels(updated);
      setCustomOpenAiModels(updated);
    }
    setQuickModelInput('');
    handleSelectModel(trimmed);
  };

  const handleDeleteQuickCustomModel = (modelId: string) => {
    const existing = getCustomOpenAiModels();
    const updated = existing.filter((m) => m.id !== modelId);
    saveCustomOpenAiModels(updated);
    setCustomOpenAiModels(updated);
    if (selectedModel === modelId) {
      if (updated.length > 0) {
        handleSelectModel(updated[0].id);
      } else {
        setSelectedModel('');
      }
    }
  };

  const handleSelectModel = (modelId: string) => {
    if (modelId.includes('::')) {
      const [pId, mId] = modelId.split('::');
      setActiveProviderId(pId);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_active_provider', pId);
      }
      setSelectedModel(mId);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_selected_model', mId);
      }
    } else {
      setSelectedModel(modelId);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_selected_model', modelId);
      }
    }
    setIsModelMenuOpen(false);
  };

  // Resizable Right Panel State
  const [rightPanelWidth, setRightPanelWidth] = useState<number>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('odon_right_panel_width');
      if (saved) {
        const parsed = parseInt(saved, 10);
        if (!isNaN(parsed) && parsed >= 280 && parsed <= 900) return parsed;
      }
    }
    return 340;
  });
  const [isResizingRight, setIsResizingRight] = useState(false);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!isResizingRight) return;
      const newWidth = window.innerWidth - e.clientX - 16;
      if (newWidth >= 280 && newWidth <= Math.min(850, window.innerWidth * 0.65)) {
        setRightPanelWidth(newWidth);
      }
    };

    const handleMouseUp = () => {
      if (isResizingRight) {
        setIsResizingRight(false);
        localStorage.setItem('odon_right_panel_width', rightPanelWidth.toString());
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
      }
    };

    if (isResizingRight) {
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      window.addEventListener('mousemove', handleMouseMove);
      window.addEventListener('mouseup', handleMouseUp);
    }

    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizingRight, rightPanelWidth]);

  // Backend & Data States
  const [daemonOnline, setDaemonOnline] = useState(false);
  const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);

  // Chat & Stream State
  const [inChatMode, setInChatMode] = useState(false);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isGenerating, setIsGenerating] = useState(false);
  const [streamingText, setStreamingText] = useState('');
  const [streamingThought, setStreamingThought] = useState('');
  const [streamingTool, setStreamingTool] = useState<string | null>(null);
  const [copiedMsgId, setCopiedMsgId] = useState<string | null>(null);

  const handleCopyMessage = (id: string, content: string) => {
    navigator.clipboard.writeText(content);
    setCopiedMsgId(id);
    setTimeout(() => setCopiedMsgId(null), 2000);
  };
  const [pendingApproval, setPendingApproval] = useState<{
    id: string;
    toolName: string;
    risk: string;
    arguments: Record<string, unknown>;
  } | null>(null);

  const chatEndRef = useRef<HTMLDivElement>(null);
  const currentSessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    currentSessionIdRef.current = currentSessionId;
  }, [currentSessionId]);

  // Check daemon status and load sessions
  const checkDaemonStatus = async () => {
    try {
      const status = await api.getStatus();
      setDaemonOnline(true);
      setSystemStatus(status);
    } catch {
      setDaemonOnline(false);
    }
  };

  const loadSessions = async () => {
    try {
      const list = await api.listSessions();
      setSessions(list);
    } catch {
      // Local fallback
    }
  };

  const loadArtifacts = async (sessionId: string) => {
    try {
      const list = await api.listArtifacts(sessionId);
      if (list && list.length > 0) {
        setArtifacts(list);
      }
    } catch { }
  };

  // Initial setup & SSE subscription
  useEffect(() => {
    checkDaemonStatus();
    loadSessions();
    api.getTools().then(setDynamicTools).catch(() => {});
    api.getModels().then((data) => {
      if (data?.providers?.length) {
        setProviders(data.providers);
        const storedProvider = localStorage.getItem('odon_active_provider');
        if (!storedProvider || storedProvider === 'openai') {
          const readyProv = data.providers.find((p: any) => p.connected || p.configured);
          if (readyProv) {
            setActiveProviderId(readyProv.id);
            localStorage.setItem('odon_active_provider', readyProv.id);
            const rec = readyProv.models.find((m: any) => m.recommended) || readyProv.models[0];
            if (rec) {
              setSelectedModel(rec.id);
              localStorage.setItem('odon_selected_model', rec.id);
            }
          }
        }
      }
    }).catch(() => {
      // Use fallback
    });

    const statusInterval = setInterval(checkDaemonStatus, 5000);

    const unsubscribe = api.subscribeEvents((event) => {
      if (event.type === 'text_delta') {
        setStreamingText((prev) => prev + event.payload.delta);
      } else if (event.type === 'thought_delta') {
        setStreamingThought((prev) => prev + event.payload.delta);
      } else if (event.type === 'approval_requested') {
        setPendingApproval({
          id: event.payload.approval_id,
          toolName: event.payload.tool_name,
          risk: `Tier ${event.payload.risk_tier}`,
          arguments: { description: event.payload.description },
        });
      } else if (event.type === 'tool_call_started') {
        const toolName = event.payload.tool_name;
        setStreamingTool(toolName);
        if (['terminal_exec', 'bash_run', 'shell_exec', 'exec_background', 'exec_stream_stdout'].includes(toolName)) {
          window.dispatchEvent(new CustomEvent('odon:agent-terminal-event', { detail: event.payload }));
        } else if (['browser_open', 'url_preview', 'probe_url', 'web_fetch'].includes(toolName)) {
          const u = event.payload.arguments?.url;
          if (u) {
            window.dispatchEvent(new CustomEvent('odon:open-browser', { detail: { url: u } }));
          }
        }
      } else if (event.type === 'tool_call_finished') {
        setStreamingTool(null);
      } else if (event.type === 'artifact_created') {
        setArtifacts((prev) => [event.payload.artifact, ...prev.filter((a) => a.id !== event.payload.artifact.id)]);
      } else if (event.type === 'state_changed') {
        if (event.payload.state === 'completed') {
          setIsGenerating(false);
          setStreamingTool(null);
          if (currentSessionId) {
            loadArtifacts(currentSessionId);
            loadSessions();
          }
        }
      }
    });

    return () => {
      clearInterval(statusInterval);
      unsubscribe();
    };
  }, [currentSessionId]);

  // Cross-panel integration event listeners (Terminal, Browser, Agent interaction)
  useEffect(() => {
    const handleOpenBrowser = (e: any) => {
      setRightPanelView('browser');
      setIsRightPanelOpen(true);
    };

    const handleRunTerminal = (e: any) => {
      setRightPanelView('terminal');
      setIsRightPanelOpen(true);
    };

    const handleSendToAgent = (e: any) => {
      if (e.detail?.text) {
        setActiveNav('chat');
        setInputMessage(e.detail.text);
      }
    };

    window.addEventListener('odon:open-browser', handleOpenBrowser);
    window.addEventListener('odon:run-terminal', handleRunTerminal);
    window.addEventListener('odon:send-to-agent', handleSendToAgent);

    return () => {
      window.removeEventListener('odon:open-browser', handleOpenBrowser);
      window.removeEventListener('odon:run-terminal', handleRunTerminal);
      window.removeEventListener('odon:send-to-agent', handleSendToAgent);
    };
  }, []);

  // Auto-scroll chat
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, streamingText, streamingThought, isGenerating]);

  const decideApproval = async (decision: 'approve_once' | 'approve_always' | 'deny') => {
    if (!pendingApproval) return;
    try {
      await api.resolveApproval(
        pendingApproval.id,
        decision,
        decision === 'deny' ? 'Rejected by user' : undefined,
      );
      setPendingApproval(null);
    } catch (error) {
      console.error('Failed to submit approval decision:', error);
    }
  };

  // Start a new chat (return to hero)
  const handleNewChat = () => {
    setInChatMode(false);
    setCurrentSessionId(null);
    setMessages([]);
    setInputMessage('');
    setStreamingText('');
    setStreamingThought('');
    setStreamingTool(null);
  };

  // Select an existing session from recent history
  const loadSessionChat = async (id: string, title: string) => {
    setCurrentSessionId(id);
    setInChatMode(true);
    setStreamingText('');
    setStreamingThought('');
    try {
      const data = await api.getSession(id);
      if (data.messages && data.messages.length > 0) {
        setMessages(
          data.messages.map((m) => ({
            id: m.id,
            role: m.role as 'user' | 'assistant',
            content: m.content.text,
            timestamp: new Date(m.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
            showAppCard: m.content.artifact_ids?.length > 0 || m.content.text.includes('```'),
            thought: m.content.thought,
            toolCalls: m.content.tool_calls?.map((t) => t.tool_name),
          }))
        );
      } else {
        setMessages([
          {
            id: 'init-ast',
            role: 'assistant',
            content: `Session "${title}" loaded. How can I help you continue this work?`,
            timestamp: 'Just now',
          },
        ]);
      }
      if (data.artifacts && data.artifacts.length > 0) {
        setArtifacts(data.artifacts);
      }
    } catch {
      setMessages([
        {
          id: 'fb-user',
          role: 'user',
          content: title,
          timestamp: 'Just now',
        },
        {
          id: 'fb-ast',
          role: 'assistant',
          content: `Here is the progress and artifacts generated for "${title}". All components are compiled and verified.`,
          timestamp: 'Just now',
          showAppCard: true,
        },
      ]);
    }
  };

  // Submit prompt from input box or action cards
  const handleSendMessage = async (customText?: string, mode: string = 'general') => {
    let textToSend = (customText || inputMessage).trim();
    if (!textToSend && attachedFiles.length === 0) return;
    if (isGenerating) return;

    if (attachedFiles.length > 0) {
      const parts: string[] = [];
      for (const file of attachedFiles) {
        if (file.isImage && file.dataUrl) {
          parts.push(`![${file.name}](${file.dataUrl})`);
        } else if (file.textContent) {
          const preview = file.textContent.length > 4000 ? file.textContent.slice(0, 4000) + '\n... (truncated)' : file.textContent;
          parts.push(`[Attached Document: ${file.name}]\n\`\`\`\n${preview}\n\`\`\``);
        }
      }
      if (parts.length > 0) {
        textToSend = `${textToSend ? textToSend + '\n\n' : ''}${parts.join('\n\n')}`;
      }
      setAttachedFiles([]);
    }

    const userMsg: ChatMessage = {
      id: `usr_${Date.now()}`,
      role: 'user',
      content: textToSend,
      timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
    };

    setMessages((prev) => [...prev, userMsg]);
    setInputMessage('');
    setInChatMode(true);
    setIsGenerating(true);
    setStreamingText('');
    setStreamingThought('');
    setStreamingTool(null);

    let sid = currentSessionId;

    if (daemonOnline) {
      try {
        if (!sid) {
          const title = textToSend.slice(0, 32) + (textToSend.length > 32 ? '...' : '');
          const newSession = await api.createSession(title, mode, selectedModel);
          sid = newSession.id;
          setCurrentSessionId(sid);
          setSessions((prev) => [newSession, ...prev]);
        }

        const modelPayload = selectedModel.includes('::') ? selectedModel : `${activeProviderId}::${selectedModel}`;
        const resMsg = await api.sendMessage(sid, textToSend, modelPayload);

        const assistantMsg: ChatMessage = {
          id: resMsg.id,
          role: 'assistant',
          content: resMsg.content.text || streamingText,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
          showAppCard: resMsg.content.artifact_ids?.length > 0 || resMsg.content.text.includes('```'),
          thought: resMsg.content.thought,
          toolCalls: resMsg.content.tool_calls?.map((t) => t.tool_name),
        };

        setMessages((prev) => [...prev, assistantMsg]);
        loadArtifacts(sid);
        loadSessions();
      } catch (err: any) {
        console.error('Daemon message send error:', err);
        const errorMsg: ChatMessage = {
          id: `ast_err_${Date.now()}`,
          role: 'assistant',
          content: `⚠️ **Gagal menghubungi model AI**: ${err.message || 'Terjadi kesalahan saat memproses jawaban.'}

Periksa konfigurasi API key atau koneksi akun provider pada menu **Settings**.`,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
        };
        setMessages((prev) => [...prev, errorMsg]);
      } finally {
        setIsGenerating(false);
        setStreamingText('');
        setStreamingThought('');
        setStreamingTool(null);
      }
    } else {
      const offlineMsg: ChatMessage = {
        id: `ast_off_${Date.now()}`,
        role: 'assistant',
        content: `⚠️ **Daemon ODON (odond) sedang offline**.

Pastikan service daemon aktif di \`http://127.0.0.1:4096\` untuk mendapatkan jawaban langsung dari AI model.`,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
      };
      setMessages((prev) => [...prev, offlineMsg]);
      setIsGenerating(false);
    }
  };

  // Trigger prompt from hero action cards
  const handleActionCard = (title: string, promptText: string, mode: string) => {
    setActiveNav(mode);
    handleSendMessage(promptText, mode);
  };

  // File attachment handler (supports images and documents)
  const handleFileAttach = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files || files.length === 0) return;

    Array.from(files).forEach((file) => {
      const isImg = file.type.startsWith('image/');
      const reader = new FileReader();
      if (isImg) {
        reader.onload = () => {
          setAttachedFiles((prev) => [
            ...prev,
            {
              name: file.name,
              type: file.type,
              size: file.size,
              dataUrl: typeof reader.result === 'string' ? reader.result : undefined,
              isImage: true,
            },
          ]);
        };
        reader.readAsDataURL(file);
      } else {
        reader.onload = () => {
          const text = typeof reader.result === 'string' ? reader.result : '';
          setAttachedFiles((prev) => [
            ...prev,
            {
              name: file.name,
              type: file.type,
              size: file.size,
              textContent: text,
              isImage: false,
            },
          ]);
        };
        reader.readAsText(file);
      }
    });

    e.target.value = '';
  };

  const removeAttachedFile = (index: number) => {
    setAttachedFiles((prev) => prev.filter((_, i) => i !== index));
  };

  // Dynamic input placeholder based on active navigation tab
  const getPlaceholder = () => {
    switch (activeNav) {
      case 'code':
        return 'Describe code to generate, refactor, or test...';
      case 'create':
        return 'Describe an image or visual creative asset to generate...';
      case 'documents':
        return 'Draft a PRD, technical specification, or document...';
      case 'files':
        return 'Ask ODON to search, inspect, or edit workspace files...';
      case 'agents':
        return 'Define an autonomous multi-step agent mission...';
      case 'automations':
        return 'Configure a background automation or scheduled workflow...';
      case 'terminal':
        return 'Ask ODON to run terminal commands or script in workspace...';
      default:
        return 'Ask ODON anything...';
    }
  };

  // Window control actions
  const handleMinimize = () => {
    setIsRightPanelOpen(false);
  };

  const handleMaximize = () => {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen().catch(() => { });
    } else {
      document.exitFullscreen().catch(() => { });
    }
  };

  const handleClose = () => {
    handleNewChat();
  };

  // Extract generated code files for CodeEditorView
  const latestCodeArtifact = artifacts.find((a) => a.kind === 'code');
  const codeFiles = latestCodeArtifact && latestCodeArtifact.payload && 'data' in latestCodeArtifact.payload && 'files' in (latestCodeArtifact.payload as any).data
    ? ((latestCodeArtifact.payload as any).data.files as any[]).map((f) => ({
      name: f.path.split('/').pop() || f.path,
      path: f.path,
      language: (latestCodeArtifact.payload as any).data.language || 'typescript',
      content: f.content,
    }))
    : undefined;

  // Session list to render
  const recentItems = sessions.length > 0
    ? (showAllRecent ? sessions : sessions.slice(0, 5)).map((s) => ({
      id: s.id,
      title: s.title,
      time: new Date(s.updated_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
    }))
    : [
      { id: 'mock-1', title: 'Rencana arsitektur ODON', time: '2h ago' },
      { id: 'mock-2', title: 'Refactor modul agent (Rust)', time: '4h ago' },
      { id: 'mock-3', title: 'Buat gambar ilustrasi produk', time: 'Yesterday' },
      { id: 'mock-4', title: 'Konversi PDF ke DOCX', time: 'Yesterday' },
    ];

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-[#f4f5f8] text-neutral-800 font-sans">
      {/* Hidden File Picker */}
      <input
        type="file"
        ref={fileInputRef}
        onChange={handleFileAttach}
        multiple
        accept="image/*,.pdf,.txt,.md,.json,.csv,.py,.rs,.ts,.js,.html,.css,.yaml,.yml,.toml"
        className="hidden"
      />

      {/* ─────────────────────────────────────────────────────────────
          1. WINDOW TITLE BAR (DESKTOP CHROME)
      ───────────────────────────────────────────────────────────── */}
      <header className="h-8 shrink-0 flex items-center justify-between px-3 text-xs text-neutral-600 bg-[#f4f5f8]">
        {/* Left: App Logo & Name + Backend Status */}
        <div className="flex items-center gap-2">
          <div
            onClick={handleNewChat}
            className="flex items-center gap-1.5 font-medium text-neutral-800 tracking-tight cursor-pointer hover:opacity-80 transition-opacity"
            title="Return to Home"
          >
            <CatLogo className="w-4 h-4 text-neutral-800" strokeWidth={2.4} />
            <span className="font-semibold text-[13px] tracking-wide">ODON</span>
          </div>

          <div
            onClick={() => setSettingsOpen(true)}
            className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-white/90 border border-neutral-200/80 text-[10px] text-neutral-600 shadow-2xs cursor-pointer hover:bg-neutral-50 transition-colors"
            title="Click to configure Daemon connection"
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${daemonOnline ? 'bg-emerald-500 animate-pulse' : 'bg-amber-400'
                }`}
            />
            <span>{daemonOnline ? 'Daemon Online (127.0.0.1:4096)' : 'Local Standby'}</span>
          </div>
        </div>

        {/* Center: Draggable Spacer */}
        <div className="flex-1 h-full mx-4" />

        {/* Right: Window Controls */}
        <div className="flex items-center gap-1">
          <button
            onClick={handleMinimize}
            className="w-7 h-6 flex items-center justify-center rounded hover:bg-neutral-200/70 text-neutral-600 transition-colors cursor-pointer"
            title="Collapse Panel"
          >
            <Minus className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleMaximize}
            className="w-7 h-6 flex items-center justify-center rounded hover:bg-neutral-200/70 text-neutral-600 transition-colors cursor-pointer"
            title="Toggle Fullscreen"
          >
            <Square className="w-3 h-3" />
          </button>
          <button
            onClick={handleClose}
            className="w-7 h-6 flex items-center justify-center rounded hover:bg-red-500 hover:text-white text-neutral-600 transition-colors cursor-pointer"
            title="Reset to Home Screen"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </header>

      {/* ─────────────────────────────────────────────────────────────
          2. MAIN 3-COLUMN WORKSPACE (ROUNDED WHITE CARDS)
      ───────────────────────────────────────────────────────────── */}
      <div className="flex-1 flex overflow-hidden p-2.5 pt-0 gap-2.5">

        {/* ───────────────────────────────────────────────────────────
            LEFT SIDEBAR CARD
        ─────────────────────────────────────────────────────────── */}
        <aside className="w-[250px] shrink-0 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col justify-between p-3.5 overflow-hidden">
          {/* Top Section */}
          <div className="flex flex-col overflow-y-auto">
            {/* Header: Cat Logo + ODON + Subtitle */}
            <div className="flex items-center gap-2.5 px-1 py-1">
              <CatLogo className="w-8 h-8 text-neutral-900 shrink-0" strokeWidth={2.2} />
              <div className="leading-tight">
                <div className="font-bold text-base text-neutral-900 tracking-tight">ODON</div>
                <div className="text-[10px] text-neutral-500 font-normal leading-tight">
                  Open Digital Orchestration Network
                </div>
              </div>
            </div>

            {/* + New Chat Button */}
            <button
              onClick={handleNewChat}
              className="mt-3.5 w-full flex items-center justify-between px-3 py-2 rounded-xl border border-neutral-200 hover:border-neutral-300 hover:bg-neutral-50 transition-all text-xs font-medium text-neutral-800 shadow-2xs group"
            >
              <div className="flex items-center gap-1.5 font-semibold">
                <Plus className="w-3.5 h-3.5 text-neutral-700 group-hover:scale-110 transition-transform" />
                <span>New Chat</span>
              </div>
              <span className="text-[11px] font-mono text-neutral-400 bg-neutral-100 px-1.5 py-0.5 rounded">
                Ctrl N
              </span>
            </button>

            {/* Navigation Menu */}
            <nav className="mt-3.5 space-y-0.5">
              <button
                onClick={() => { setActiveNav('chat'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'chat'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <MessageSquare className={`w-4 h-4 ${activeNav === 'chat' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>AI Chat</span>
              </button>

              <button
                onClick={() => { setActiveNav('code'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'code'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Code2 className={`w-4 h-4 ${activeNav === 'code' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Code</span>
              </button>

              <button
                onClick={() => { setActiveNav('terminal'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'terminal'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Terminal className={`w-4 h-4 ${activeNav === 'terminal' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Terminal</span>
              </button>

              <button
                onClick={() => { setActiveNav('ssh'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'ssh'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Server className={`w-4 h-4 ${activeNav === 'ssh' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>SSH Remote</span>
              </button>

              <button
                onClick={() => { setActiveNav('github'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'github'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <GitPullRequest className={`w-4 h-4 ${activeNav === 'github' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>GitHub</span>
              </button>

              <button
                onClick={() => { setActiveNav('automations'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'automations'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Share2 className={`w-4 h-4 ${activeNav === 'automations' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Automations</span>
              </button>

              <button
                onClick={() => { setActiveNav('mcp'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'mcp'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Cpu className={`w-4 h-4 ${activeNav === 'mcp' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>MCP Hub</span>
              </button>

              <button
                onClick={() => setActiveNav('create')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'create'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <ImageIcon className={`w-4 h-4 ${activeNav === 'create' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Create</span>
              </button>

              <button
                onClick={() => setActiveNav('documents')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'documents'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <FileText className={`w-4 h-4 ${activeNav === 'documents' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Documents</span>
              </button>

              <button
                onClick={() => setActiveNav('files')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${(activeNav === 'files' || activeNav === 'explorer')
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Folder className={`w-4 h-4 ${(activeNav === 'files' || activeNav === 'explorer') ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Files</span>
              </button>

              <button
                onClick={() => setActiveNav('agents')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'agents'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Bot className={`w-4 h-4 ${activeNav === 'agents' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>Agents</span>
              </button>
            </nav>

            {/* Recent Section (Bound to SQLite Sessions) */}
            <div className="mt-5 pt-3 border-t border-neutral-100">
              <div className="text-[11px] font-medium text-neutral-400 px-1 mb-2">Recent</div>
              <div className="space-y-1">
                {recentItems.map((item) => (
                  <button
                    key={item.id}
                    onClick={() => loadSessionChat(item.id, item.title)}
                    className={`w-full flex items-center justify-between px-2 py-1.5 rounded-lg text-xs transition-colors group text-left ${currentSessionId === item.id
                        ? 'bg-neutral-100 font-medium text-neutral-900'
                        : 'text-neutral-700 hover:bg-neutral-100/80'
                      }`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <MessageSquare className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-600 shrink-0" />
                      <span className="truncate">{item.title}</span>
                    </div>
                    <span className="text-[10px] text-neutral-400 shrink-0 ml-1">{item.time}</span>
                  </button>
                ))}
              </div>
              <div className="flex items-center justify-between px-1 mt-1 text-[11px] text-neutral-400">
                <button
                  onClick={() => setShowAllRecent(!showAllRecent)}
                  className="hover:text-neutral-700 py-0.5 transition-colors cursor-pointer"
                >
                  {showAllRecent ? 'Show less' : 'Show more...'}
                </button>
                <button
                  onClick={loadSessions}
                  className="hover:text-neutral-700 py-0.5 transition-colors cursor-pointer"
                  title="Reload from SQLite"
                >
                  Refresh
                </button>
              </div>
            </div>
          </div>

          {/* User Profile at Bottom */}
          <div className="pt-3 border-t border-neutral-100 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="w-7 h-7 rounded-full bg-blue-600 text-white flex items-center justify-center font-semibold text-xs tracking-tight shadow-2xs">
                OD
              </div>
              <div className="leading-tight">
                <div className="font-semibold text-xs text-neutral-900">Odon User</div>
                <div className="text-[10px] text-neutral-400">Pro • Local</div>
              </div>
            </div>
            <button
              onClick={() => setSettingsOpen(true)}
              className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-lg transition-colors cursor-pointer"
              title="Settings & Preferences"
            >
              <Settings className="w-4 h-4" />
            </button>
          </div>
        </aside>

        {/* ───────────────────────────────────────────────────────────
            CENTER MAIN CHAT / HERO CARD
        ─────────────────────────────────────────────────────────── */}
        <main className="flex-1 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col justify-between overflow-hidden relative min-w-0">

          {/* Top Bar inside Center Card */}
          <div className="h-12 border-b border-neutral-100 px-4 flex items-center justify-between shrink-0">
            {activeNav !== 'chat' && activeNav !== 'create' && activeNav !== 'documents' && activeNav !== 'agents' ? (
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setActiveNav('chat')}
                  className="text-xs text-blue-600 hover:text-blue-800 font-medium px-2.5 py-1 rounded-lg hover:bg-blue-50 transition-colors cursor-pointer flex items-center gap-1"
                >
                  <ArrowLeft className="w-3.5 h-3.5" />
                  <span>AI Chat</span>
                </button>
                <span className="text-neutral-300">/</span>
                <span className="text-xs font-bold text-neutral-900 capitalize tracking-tight flex items-center gap-1.5">
                  {activeNav === 'github' && <GitPullRequest className="w-3.5 h-3.5 text-blue-600" />}
                  {activeNav === 'automations' && <Share2 className="w-3.5 h-3.5 text-blue-600" />}
                  {activeNav === 'mcp' && <Cpu className="w-3.5 h-3.5 text-blue-600" />}
                  {activeNav === 'terminal' && <Terminal className="w-3.5 h-3.5 text-blue-600" />}
                  {activeNav === 'code' && <Code2 className="w-3.5 h-3.5 text-blue-600" />}
                  {(activeNav === 'files' || activeNav === 'explorer') && <Folder className="w-3.5 h-3.5 text-blue-600" />}
                  <span>{activeNav === 'files' || activeNav === 'explorer' ? 'File Explorer' : `${activeNav} Workspace`}</span>
                </span>
              </div>
            ) : (
              /* Model Selector Pill */
              <div className="flex items-center gap-2">
                <div
                  onClick={() => setIsModelMenuOpen(!isModelMenuOpen)}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-neutral-200/90 text-xs font-medium text-neutral-800 hover:bg-neutral-50 transition-colors shadow-2xs cursor-pointer"
                  title={`Active Provider: ${activeProvider?.name} (Click to change)`}
                >
                  <Sparkles className="w-3.5 h-3.5 text-blue-600" />
                  <span className="text-[11px] font-semibold text-neutral-500">{activeProvider?.name.split(' (')[0]}:</span>
                  <span className="font-semibold text-neutral-900 truncate max-w-[150px]">{displayModelName}</span>
                  <ChevronDown className="w-3 h-3 text-neutral-400 ml-0.5" />
                </div>

                {inChatMode && (
                  <button
                    onClick={handleNewChat}
                    className="text-xs text-blue-600 hover:text-blue-800 font-medium px-2 py-1 rounded-md hover:bg-blue-50 transition-colors cursor-pointer"
                  >
                    ← Back to Home
                  </button>
                )}
              </div>
            )}

            {/* Action Buttons on Right */}
            <div className="flex items-center gap-1">
              <button
                onClick={() => setApprovalCenterOpen(true)}
                className={`p-2 rounded-lg transition-colors cursor-pointer relative ${
                  pendingApproval
                    ? 'text-amber-600 bg-amber-50 hover:bg-amber-100 ring-1 ring-amber-300'
                    : 'text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100'
                }`}
                title={pendingApproval ? "Approval Required: Action paused" : "Security Approval & Audit Center"}
              >
                {pendingApproval ? <ShieldAlert className="w-4 h-4 text-amber-600" /> : <ShieldCheck className="w-4 h-4" />}
                {pendingApproval && (
                  <span className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-red-500 animate-pulse" />
                )}
              </button>
              <button
                onClick={() => setIsSearchOpen(true)}
                className="p-2 text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-lg transition-colors cursor-pointer"
                title="Search Sessions & Artifacts"
              >
                <Search className="w-4 h-4" />
              </button>
              <button
                onClick={() => setIsBellOpen(true)}
                className="p-2 text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-lg transition-colors cursor-pointer"
                title="System Notifications & Activity"
              >
                <Bell className="w-4 h-4" />
              </button>
              <button
                onClick={() => setIsRightPanelOpen(!isRightPanelOpen)}
                className={`p-2 rounded-lg transition-colors cursor-pointer ${isRightPanelOpen
                    ? 'text-neutral-700 hover:bg-neutral-100'
                    : 'text-neutral-400 hover:text-neutral-800 hover:bg-neutral-100'
                  }`}
                title={isRightPanelOpen ? 'Collapse panel' : 'Open panel'}
              >
                {isRightPanelOpen ? <PanelRightClose className="w-4 h-4" /> : <PanelRightOpen className="w-4 h-4" />}
              </button>
            </div>
          </div>

          {/* Workspace Views */}
          {activeNav === 'github' && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white">
              <GitHubView
                onSendMessage={(prompt) => {
                  setActiveNav('chat');
                  handleSendMessage(prompt, 'chat');
                }}
                onOpenTerminal={() => {
                  setActiveNav('terminal');
                }}
              />
            </div>
          )}

          {activeNav === 'automations' && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white">
              <AutomationsView />
            </div>
          )}

          {activeNav === 'mcp' && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white">
              <McpView />
            </div>
          )}

          {(activeNav === 'files' || activeNav === 'explorer') && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white min-w-0">
              <FileExplorerView
                onAskAiAboutFile={(path, content) => {
                  setActiveNav('chat');
                  setInputMessage(`Tolong analisa file ${path}:\n\`\`\`\n${content.slice(0, 4000)}\n\`\`\``);
                }}
                onOpenInTerminal={() => {
                  setActiveNav('terminal');
                }}
              />
            </div>
          )}

          {activeNav === 'ssh' && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white">
              <SshTerminalView />
            </div>
          )}

          {activeNav === 'terminal' && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white">
              <TerminalView />
            </div>
          )}

          {activeNav === 'code' && (
            <div className="flex-1 overflow-hidden flex flex-col p-4 bg-white">
              <CodeEditorView
                files={codeFiles}
                onRunPreview={() => {
                  setRightPanelView('browser');
                  setIsRightPanelOpen(true);
                }}
                onOpenTerminal={() => {
                  setActiveNav('terminal');
                }}
              />
            </div>
          )}

          {/* Center Content Area for Chat / Documents / Creative Tasks */}
          {activeNav !== 'github' && activeNav !== 'automations' && activeNav !== 'mcp' && activeNav !== 'terminal' && activeNav !== 'code' && activeNav !== 'files' && activeNav !== 'explorer' && activeNav !== 'ssh' && (
            <>
              <div className="flex-1 overflow-y-auto overflow-x-hidden px-6 py-6 flex flex-col justify-start select-text min-w-0 w-full">

            {/* VIEW A: WELCOME HERO (EXACT MATCH TO DESIGN) */}
            {!inChatMode && (
              <div className={`w-full ${isRightPanelOpen ? 'max-w-3xl' : 'max-w-4xl'} flex flex-col items-center my-auto mx-auto min-w-0`}>
                {/* Cute Cat Logo Illustration */}
                <div className="mb-3 text-neutral-900 hover:scale-105 transition-transform">
                  <CatLogo className="w-20 h-20" strokeWidth={2} />
                </div>

                {/* Main Heading */}
                <h1 className="text-2xl font-bold text-neutral-900 tracking-tight text-center">
                  What can ODON do for you?
                </h1>

                {/* Subheading */}
                <p className="text-xs text-neutral-500 text-center mt-1.5 mb-8">
                  Your universal AI assistant for coding, creating, documents, and more.
                </p>

                {/* 6 Feature Action Cards (3x2 Grid) */}
                <div className="grid grid-cols-3 gap-3.5 w-full">
                  {/* Card 1: Build an app */}
                  <button
                    onClick={() => handleActionCard('Build an app', 'Build a personal notes web application with clean modular React 19 and SQLite storage.', 'code')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Code2 className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Build an app</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Create, refactor, and debug code
                    </div>
                  </button>

                  {/* Card 2: Create an image */}
                  <button
                    onClick={() => handleActionCard('Create an image', 'Generate a modern minimalist product illustration for ODON desktop UI.', 'create')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <ImageIcon className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Create an image</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Generate or edit images
                    </div>
                  </button>

                  {/* Card 3: Make a document */}
                  <button
                    onClick={() => handleActionCard('Make a document', 'Draft a comprehensive product requirements document (PRD) for local AI orchestration.', 'documents')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <FileText className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Make a document</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Write, convert, or analyze files
                    </div>
                  </button>

                  {/* Card 4: Run an agent */}
                  <button
                    onClick={() => handleActionCard('Run an agent', 'Run autonomous security audit agent on the workspace tools and policy engine.', 'agents')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Bot className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Run an agent</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Let AI work on complex tasks
                    </div>
                  </button>

                  {/* Card 5: Automate a workflow */}
                  <button
                    onClick={() => handleActionCard('Automate a workflow', 'Set up automated background scheduler for Telegram and WhatsApp relay sync.', 'automations')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Share2 className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Automate a workflow</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Schedule and orchestrate tasks
                    </div>
                  </button>

                  {/* Card 6: Create a video */}
                  <button
                    onClick={() => handleActionCard('Create a video', 'Generate a product showcase walkthrough storyboard and video script.', 'create')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Video className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Create a video</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Generate video from ideas
                    </div>
                  </button>
                </div>
              </div>
            )}

            {/* VIEW B: CONVERSATION STREAM */}
            {inChatMode && (
              <div className={`w-full ${isRightPanelOpen ? 'max-w-3xl' : 'max-w-4xl'} flex flex-col space-y-6 mx-auto select-text min-w-0 overflow-x-hidden`}>
                {messages.map((msg) => (
                  <div key={msg.id} className="flex flex-col space-y-2 group">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <div className={`w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-bold ${
                          msg.role === 'user'
                            ? 'bg-blue-600 text-white'
                            : 'bg-white border border-neutral-200 text-neutral-900 shadow-2xs'
                        }`}>
                          {msg.role === 'user' ? 'U' : <CatLogo className="w-3.5 h-3.5 text-neutral-900" />}
                        </div>
                        <span className="text-xs font-semibold text-neutral-900">
                          {msg.role === 'user' ? 'You' : 'ODON'}
                        </span>
                        <span className="text-[10px] text-neutral-400">{msg.timestamp}</span>
                      </div>

                      {/* Message Action Bar (Copy Button like ChatGPT / Muse AI) */}
                      <button
                        onClick={() => handleCopyMessage(msg.id, msg.content)}
                        className="opacity-0 group-hover:opacity-100 focus:opacity-100 flex items-center gap-1 px-2 py-0.5 rounded text-[11px] text-neutral-400 hover:text-neutral-800 hover:bg-neutral-100 transition-all cursor-pointer"
                        title="Copy message"
                      >
                        {copiedMsgId === msg.id ? (
                          <>
                            <Check className="w-3 h-3 text-emerald-600" />
                            <span className="text-emerald-600 font-medium">Copied</span>
                          </>
                        ) : (
                          <>
                            <Copy className="w-3 h-3" />
                            <span>Copy</span>
                          </>
                        )}
                      </button>
                    </div>

                    {msg.thought && (
                      <div className="ml-7 my-1.5">
                        <details className="group rounded-xl bg-neutral-50/80 border border-neutral-200/80 overflow-hidden shadow-2xs">
                          <summary className="px-3 py-1.5 cursor-pointer text-xs font-medium text-neutral-600 hover:text-neutral-900 flex items-center justify-between select-none bg-neutral-100/50 hover:bg-neutral-100 transition-colors">
                            <div className="flex items-center gap-1.5">
                              <Sparkles className="w-3 h-3 text-amber-500" />
                              <span className="text-[11.5px] font-semibold text-neutral-700">Reasoning & Thought Process</span>
                            </div>
                            <span className="text-[10px] text-neutral-400 font-mono group-open:hidden">Click to expand</span>
                          </summary>
                          <div className="p-3 border-t border-neutral-200/60 bg-neutral-50/30 text-[11px] leading-snug text-neutral-600 select-text max-w-full min-w-0 overflow-x-hidden break-words">
                            <MarkdownView content={msg.thought} dense />
                          </div>
                        </details>
                      </div>
                    )}

                    {msg.toolCalls && msg.toolCalls.length > 0 && (
                      <div className="ml-7 flex flex-wrap items-center gap-2 py-1">
                        {msg.toolCalls.map((tc, idx) => {
                          const tcLower = tc.toLowerCase();
                          const isTerm = ['terminal', 'bash', 'shell', 'exec', 'kill'].some(k => tcLower.includes(k));
                          const isWeb = ['browser', 'web', 'fetch', 'url', 'http'].some(k => tcLower.includes(k));
                          return (
                            <span
                              key={idx}
                              onClick={() => {
                                if (isTerm) {
                                  setRightPanelView('terminal');
                                  setIsRightPanelOpen(true);
                                } else if (isWeb) {
                                  setRightPanelView('browser');
                                  setIsRightPanelOpen(true);
                                }
                              }}
                              className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg border text-[11px] font-mono transition-all cursor-pointer ${
                                isTerm
                                  ? 'bg-emerald-50 text-emerald-800 border-emerald-200/80 hover:bg-emerald-100'
                                  : isWeb
                                  ? 'bg-blue-50 text-blue-800 border-blue-200/80 hover:bg-blue-100'
                                  : 'bg-neutral-100 text-neutral-700 border-neutral-200 hover:bg-neutral-200'
                              }`}
                              title={isTerm ? "Click to view in Terminal panel" : isWeb ? "Click to view in Browser Preview" : tc}
                            >
                              {isTerm ? (
                                <Terminal className="w-3 h-3 text-emerald-600" />
                              ) : isWeb ? (
                                <Globe className="w-3 h-3 text-blue-600" />
                              ) : (
                                <Terminal className="w-3 h-3 text-neutral-500" />
                              )}
                              <span>{tc}</span>
                            </span>
                          );
                        })}
                      </div>
                    )}

                    <div className="pl-7 select-text">
                      <MarkdownView content={msg.content} />
                    </div>

                    {/* Dynamic Artifacts Card for this turn */}
                    {msg.showAppCard && (
                      <div className="ml-7 mt-2 space-y-2">
                        {artifacts.filter(a => msg.role === 'assistant').length > 0 ? (
                          artifacts.map((art) => {
                            if (art.kind === 'image' && art.payload.type === 'image') {
                              const rawUrl = art.payload.data.preview_url || '';
                              const fullSrc = rawUrl.startsWith('/api/') ? `http://127.0.0.1:4096${rawUrl}` : rawUrl;
                              const fileName = art.title.endsWith('.png') ? art.title : `${art.title}.png`;

                              return (
                                <div key={art.id} className="p-3.5 rounded-2xl border border-neutral-200/90 bg-white shadow-2xs max-w-lg">
                                  <div className="flex items-center justify-between mb-2">
                                    <div className="flex items-center gap-2">
                                      <div className="p-1.5 rounded-lg bg-neutral-100 text-neutral-800 border border-neutral-200">
                                        <ImageIcon className="w-4 h-4" />
                                      </div>
                                      <div>
                                        <div className="font-semibold text-xs text-neutral-900">{art.title}</div>
                                        <div className="text-[10px] text-neutral-500">
                                          {art.payload.data.width} × {art.payload.data.height} • {art.payload.data.model}
                                        </div>
                                      </div>
                                    </div>
                                    <span className="inline-flex items-center gap-1 text-[10px] font-medium text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded-full border border-emerald-200">
                                      <CheckCircle2 className="w-3 h-3 text-emerald-600" />
                                      Saved
                                    </span>
                                  </div>

                                  <div className="relative aspect-video rounded-xl bg-neutral-100 border border-neutral-200 flex items-center justify-center overflow-hidden my-2.5">
                                    {fullSrc ? (
                                      <img src={fullSrc} alt={art.payload.data.prompt} className="w-full h-full object-contain" />
                                    ) : (
                                      <ImageIcon className="w-8 h-8 text-neutral-600" />
                                    )}
                                  </div>

                                  <div className="flex items-center gap-2 mt-3">
                                    <button
                                      onClick={async () => {
                                        if (!fullSrc) return;
                                        try {
                                          const resp = await fetch(fullSrc);
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
                                      <Download className="w-3 h-3" />
                                      Save Image
                                    </button>
                                    <button
                                      onClick={() => {
                                        setRightPanelView('artifacts');
                                        setIsRightPanelOpen(true);
                                      }}
                                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 bg-white hover:bg-neutral-50 text-neutral-700 text-xs font-medium transition-colors cursor-pointer"
                                    >
                                      <Layers className="w-3 h-3 text-neutral-500" />
                                      Artifacts Panel
                                    </button>
                                  </div>
                                </div>
                              );
                            }

                            if (art.kind === 'code' && art.payload.type === 'code') {
                              return (
                                <div key={art.id} className="p-3.5 rounded-2xl border border-neutral-200/90 bg-neutral-50/60 shadow-2xs max-w-lg">
                                  <div className="flex items-center justify-between mb-2">
                                    <div className="flex items-center gap-2">
                                      <div className="p-1.5 rounded-lg bg-blue-50 text-blue-600 border border-blue-100">
                                        <Code2 className="w-4 h-4" />
                                      </div>
                                      <div>
                                        <div className="font-semibold text-xs text-neutral-900">{art.title}</div>
                                        <div className="text-[10px] text-neutral-500">{art.summary}</div>
                                      </div>
                                    </div>
                                    <span className="inline-flex items-center gap-1 text-[10px] font-medium text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded-full border border-emerald-200">
                                      <CheckCircle2 className="w-3 h-3 text-emerald-600" />
                                      Verified
                                    </span>
                                  </div>

                                  <div className="flex items-center gap-2 mt-3">
                                    <button
                                      onClick={() => {
                                        setRightPanelView('browser');
                                        setIsRightPanelOpen(true);
                                      }}
                                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium transition-colors shadow-2xs cursor-pointer"
                                    >
                                      <ExternalLink className="w-3 h-3" />
                                      Preview in Browser
                                    </button>
                                    <button
                                      onClick={() => {
                                        setRightPanelView('editor');
                                        setIsRightPanelOpen(true);
                                      }}
                                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 bg-white hover:bg-neutral-50 text-neutral-700 text-xs font-medium transition-colors cursor-pointer"
                                    >
                                      <Code2 className="w-3 h-3 text-neutral-500" />
                                      Inspect Source
                                    </button>
                                    <button
                                      onClick={() => {
                                        setRightPanelView('terminal');
                                        setIsRightPanelOpen(true);
                                      }}
                                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 bg-white hover:bg-neutral-50 text-neutral-700 text-xs font-medium transition-colors cursor-pointer"
                                    >
                                      <Terminal className="w-3 h-3 text-neutral-500" />
                                      Terminal
                                    </button>
                                  </div>
                                </div>
                              );
                            }

                            return null;
                          })
                        ) : null}
                      </div>
                    )}
                  </div>
                ))}

                {/* Active Generating / Streaming Turn */}
                {pendingApproval && (
                  <div className="flex gap-4 mb-4 max-w-3xl" role="alert" aria-live="assertive">
                    <div className="flex-1">
                      <ApprovalCard
                        id={pendingApproval.id}
                        toolName={pendingApproval.toolName}
                        risk={pendingApproval.risk}
                        description={(pendingApproval.arguments?.description as string) || undefined}
                        parameters={pendingApproval.arguments}
                        onDecide={decideApproval}
                      />
                    </div>
                  </div>
                )}

                {isGenerating && (
                  <div className="flex flex-col space-y-2">
                    <div className="flex items-center gap-2">
                      <div className="w-5 h-5 rounded-full bg-white border border-neutral-200 text-neutral-900 shadow-2xs flex items-center justify-center">
                        <CatLogo className="w-3.5 h-3.5 text-neutral-900" />
                      </div>
                      <span className="text-xs font-semibold text-neutral-900">ODON</span>
                      <span className="text-[10px] text-neutral-400 flex items-center gap-1">
                        <Loader2 className="w-3 h-3 animate-spin text-blue-600" />
                        Thinking...
                      </span>
                    </div>

                    {streamingTool && (
                      <div className="ml-7 inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-blue-50 text-blue-700 border border-blue-200/60 text-[11px] font-medium animate-pulse">
                        <Terminal className="w-3 h-3" />
                        <span>Running tool: {streamingTool}...</span>
                      </div>
                    )}

                    {streamingThought && (
                      <div className="ml-7 my-1.5">
                        <div className="rounded-xl bg-amber-50/40 border border-amber-200/70 overflow-hidden shadow-2xs">
                          <div className="px-3 py-1.5 bg-amber-100/50 border-b border-amber-200/60 text-xs font-semibold text-amber-900 flex items-center justify-between">
                            <div className="flex items-center gap-1.5">
                              <Sparkles className="w-3 h-3 text-amber-600 animate-spin" />
                              <span className="text-[11.5px]">ODON is reasoning...</span>
                            </div>
                            <span className="text-[10px] text-amber-700 font-mono animate-pulse">Live</span>
                          </div>
                          <div className="p-3 text-[11px] leading-snug text-neutral-600 max-h-60 overflow-y-auto select-text bg-white/60 max-w-full min-w-0 overflow-x-hidden break-words">
                            <MarkdownView content={streamingThought} dense />
                          </div>
                        </div>
                      </div>
                    )}

                    <div className="pl-7 select-text">
                      {streamingText ? (
                        <MarkdownView content={streamingText} />
                      ) : (
                        <span className="text-xs text-neutral-400 italic">
                          Orchestrating tools and synthesizing solution...
                        </span>
                      )}
                    </div>
                  </div>
                )}

                <div ref={chatEndRef} />
              </div>
            )}
          </div>

          {/* Bottom Prompt Input Box */}
          <div className="p-4 pt-0 shrink-0">
            <div className={`w-full ${isRightPanelOpen ? 'max-w-3xl' : 'max-w-4xl'} mx-auto rounded-3xl border border-neutral-200/90 bg-white shadow-2xs p-3 focus-within:border-neutral-300 focus-within:shadow-xs transition-all min-w-0`}>
              {/* Attached Files Preview Chips */}
              {attachedFiles.length > 0 && (
                <div className="flex flex-wrap gap-2 mb-2 pb-2 border-b border-neutral-100">
                  {attachedFiles.map((file, idx) => (
                    <div
                      key={idx}
                      className="flex items-center gap-1.5 px-2.5 py-1 rounded-xl bg-neutral-100 border border-neutral-200 text-xs text-neutral-800"
                    >
                      {file.isImage && file.dataUrl ? (
                        <img src={file.dataUrl} alt={file.name} className="w-5 h-5 rounded object-cover" />
                      ) : (
                        <FileText className="w-3.5 h-3.5 text-blue-500" />
                      )}
                      <span className="truncate max-w-[140px] font-medium text-[11px]">{file.name}</span>
                      <span className="text-[10px] text-neutral-400">({(file.size / 1024).toFixed(0)}kb)</span>
                      <button
                        type="button"
                        onClick={() => removeAttachedFile(idx)}
                        className="text-neutral-400 hover:text-neutral-700 p-0.5 rounded-full cursor-pointer"
                        title="Remove attachment"
                      >
                        <X className="w-3 h-3" />
                      </button>
                    </div>
                  ))}
                </div>
              )}

              {/* Input Area */}
              <input
                type="text"
                value={inputMessage}
                onChange={(e) => setInputMessage(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSendMessage()}
                placeholder={getPlaceholder()}
                disabled={isGenerating}
                className="w-full text-xs text-neutral-800 placeholder-neutral-400 outline-none px-1 py-1 bg-transparent"
              />

              {/* Bottom Row Tools inside Input Box */}
              <div className="flex items-center justify-between mt-2 pt-2 border-t border-neutral-100">
                {/* Left Tools */}
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => fileInputRef.current?.click()}
                    className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-full transition-colors cursor-pointer"
                    title="Attach document or image"
                  >
                    <Paperclip className="w-4 h-4" />
                  </button>

                  {/* Tools Dropdown */}
                  <div className="relative">
                    <button
                      onClick={() => setIsToolsMenuOpen(!isToolsMenuOpen)}
                      className="flex items-center gap-1 px-2.5 py-1 text-xs text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-full transition-colors ml-1 border border-neutral-200/60 cursor-pointer"
                    >
                      <Globe className="w-3.5 h-3.5 text-neutral-500" />
                      <span>Tools ({REGISTERED_TOOLS.length})</span>
                      <ChevronDown className="w-3 h-3 text-neutral-400" />
                    </button>
                  </div>
                </div>

                {/* Right Tools */}
                <div className="flex items-center gap-2">
                  {/* Model Selector Pill */}
                  <div className="relative">
                    <button
                      onClick={() => setIsModelMenuOpen(!isModelMenuOpen)}
                      className="flex items-center gap-1.5 px-2.5 py-1 text-xs text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-full transition-colors border border-neutral-200/60 cursor-pointer"
                      title={`Active Provider: ${activeProvider?.name}`}
                    >
                      <Sparkles className="w-3 h-3 text-blue-500" />
                      <span className="text-[10px] text-neutral-400 font-normal">{activeProvider?.name.split(' (')[0]}:</span>
                      <span className="truncate max-w-[130px] font-medium">{displayModelName}</span>
                      <ChevronDown className="w-3 h-3 text-neutral-400" />
                    </button>
                  </div>

                  {/* Send Button */}
                  <button
                    onClick={() => handleSendMessage()}
                    disabled={isGenerating || !inputMessage.trim()}
                    className="w-7 h-7 rounded-full bg-blue-600 hover:bg-blue-500 disabled:opacity-40 text-white flex items-center justify-center transition-transform active:scale-95 shadow-2xs cursor-pointer"
                  >
                    <ArrowUp className="w-3.5 h-3.5 stroke-[2.5]" />
                  </button>
                </div>
              </div>
            </div>
          </div>
            </>
          )}
        </main>

        {/* ───────────────────────────────────────────────────────────
            RIGHT PANEL: ARTIFACTS / RELAYS / CODE / BROWSER / TERMINAL
        ─────────────────────────────────────────────────────────── */}
        {isRightPanelOpen && (
          <>
            {/* Draggable resize handle */}
            <div
              onMouseDown={(e) => {
                e.preventDefault();
                setIsResizingRight(true);
              }}
              className={`w-2 -mr-1 z-20 cursor-col-resize flex items-center justify-center group shrink-0 select-none transition-colors ${isResizingRight ? 'bg-blue-500/20' : 'hover:bg-neutral-300/60'
                }`}
              title="Drag to resize panel"
            >
              <div
                className={`w-0.5 h-8 rounded-full transition-colors ${isResizingRight ? 'bg-blue-600' : 'bg-neutral-300 group-hover:bg-neutral-500'
                  }`}
              />
            </div>

            <aside
              style={{ width: `${rightPanelWidth}px` }}
              className="shrink-0 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col p-3.5 overflow-hidden relative"
            >

              {/* Top View Selector Tabs (Artifacts / Code / Browser / Terminal) */}
              <div className="flex items-center gap-1 p-1 bg-neutral-100/80 rounded-xl mb-3 text-[11px] font-medium text-neutral-600 shrink-0 select-none">
                <button
                  onClick={() => setRightPanelView('artifacts')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer ${rightPanelView === 'artifacts'
                      ? 'bg-white text-neutral-900 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900'
                    }`}
                >
                  Artifacts
                </button>
                <button
                  onClick={() => setRightPanelView('editor')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer ${rightPanelView === 'editor'
                      ? 'bg-white text-neutral-900 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900'
                    }`}
                >
                  Code
                </button>
                <button
                  onClick={() => setRightPanelView('browser')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer ${rightPanelView === 'browser'
                      ? 'bg-white text-neutral-900 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900'
                    }`}
                >
                  Browser
                </button>
                <button
                  onClick={() => setRightPanelView('terminal')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer flex items-center justify-center gap-1 ${rightPanelView === 'terminal'
                      ? 'bg-white text-blue-600 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900 text-neutral-600'
                    }`}
                >
                  <Terminal className="w-3 h-3 text-blue-600" />
                  <span>Terminal</span>
                </button>
              </div>

              {/* Right Panel Sub-View: Code Editor */}
              {rightPanelView === 'editor' && (
                <div className="h-full flex flex-col">
                  <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 mb-2.5 shrink-0">
                    <div className="flex items-center gap-1.5">
                      <button
                        onClick={() => setRightPanelView('artifacts')}
                        className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors cursor-pointer"
                        title="Back to Artifacts"
                      >
                        <ArrowLeft className="w-3.5 h-3.5" />
                      </button>
                      <span className="font-bold text-xs text-neutral-900">Code Editor</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setRightPanelView('terminal')}
                        className="text-[11px] text-neutral-600 hover:text-neutral-900 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <Terminal className="w-3 h-3 text-neutral-500" />
                        <span>Terminal</span>
                      </button>
                      <button
                        onClick={() => setRightPanelView('browser')}
                        className="text-[11px] text-blue-600 hover:text-blue-800 font-medium cursor-pointer"
                      >
                        Browser →
                      </button>
                    </div>
                  </div>
                  <div className="flex-1 overflow-hidden">
                    <CodeEditorView
                      files={codeFiles}
                      onRunPreview={() => setRightPanelView('browser')}
                      onOpenTerminal={() => setRightPanelView('terminal')}
                    />
                  </div>
                </div>
              )}

              {/* Right Panel Sub-View: Browser Preview */}
              {rightPanelView === 'browser' && (
                <div className="h-full flex flex-col">
                  <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 mb-2.5 shrink-0">
                    <div className="flex items-center gap-1.5">
                      <button
                        onClick={() => setRightPanelView('artifacts')}
                        className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors cursor-pointer"
                        title="Back to Artifacts"
                      >
                        <ArrowLeft className="w-3.5 h-3.5" />
                      </button>
                      <span className="font-bold text-xs text-neutral-900">Browser Preview</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setRightPanelView('terminal')}
                        className="text-[11px] text-neutral-600 hover:text-neutral-900 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <Terminal className="w-3 h-3 text-neutral-500" />
                        <span>Terminal</span>
                      </button>
                      <button
                        onClick={() => setRightPanelView('editor')}
                        className="text-[11px] text-blue-600 hover:text-blue-800 font-medium cursor-pointer"
                      >
                        View Code →
                      </button>
                    </div>
                  </div>
                  <div className="flex-1 overflow-hidden">
                    <BrowserPreviewView />
                  </div>
                </div>
              )}

              {/* Right Panel Sub-View: Integrated Terminal */}
              {rightPanelView === 'terminal' && (
                <div className="h-full flex flex-col">
                  <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 mb-2.5 shrink-0">
                    <div className="flex items-center gap-1.5">
                      <button
                        onClick={() => setRightPanelView('artifacts')}
                        className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors cursor-pointer"
                        title="Back to Artifacts"
                      >
                        <ArrowLeft className="w-3.5 h-3.5" />
                      </button>
                      <span className="font-bold text-xs text-neutral-900">Integrated Terminal</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setRightPanelView('editor')}
                        className="text-[11px] text-neutral-600 hover:text-neutral-900 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <Code2 className="w-3 h-3 text-neutral-500" />
                        <span>Code</span>
                      </button>
                      <button
                        onClick={() => setRightPanelView('browser')}
                        className="text-[11px] text-blue-600 hover:text-blue-800 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <ExternalLink className="w-3 h-3 text-blue-600" />
                        <span>Browser</span>
                      </button>
                    </div>
                  </div>
                  <div className="flex-1 overflow-hidden">
                    <TerminalView />
                  </div>
                </div>
              )}

              {/* Right Panel Default View: Artifacts & Relays */}
              {rightPanelView === 'artifacts' && (
                <div className="flex-1 flex flex-col justify-between overflow-y-auto">
                  <div className="space-y-4">
                    {/* 1. Artifacts Section */}
                    <div>
                      <div className="flex items-center gap-2 mb-0.5">
                        <Layers className="w-4 h-4 text-neutral-900" />
                        <h2 className="font-bold text-sm text-neutral-900">Artifacts</h2>
                      </div>
                      <p className="text-[11px] text-neutral-500 mb-3">
                        View and manage results from your tasks.
                      </p>

                      {/* Artifacts List (Bound to real or sample artifacts) */}
                      <div className="space-y-2">
                        {artifacts.length > 0 ? (
                          artifacts.map((art) => (
                            <div
                              key={art.id}
                              onClick={() => {
                                if (art.kind === 'code' || art.kind === 'patch') {
                                  setRightPanelView('editor');
                                } else {
                                  setRightPanelView('browser');
                                }
                              }}
                              className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group cursor-pointer"
                            >
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                                  {art.kind === 'code' ? (
                                    <Code2 className="w-4 h-4" />
                                  ) : art.kind === 'image' ? (
                                    <ImageIcon className="w-4 h-4" />
                                  ) : (
                                    <FileText className="w-4 h-4" />
                                  )}
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    {art.title}
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5 capitalize">
                                    {art.kind} • {new Date(art.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          ))
                        ) : (
                          <>
                            {/* Sample Item 1: odon-architecture.png */}
                            <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                                  <ImageIcon className="w-4 h-4" />
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    odon-architecture.png
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5">
                                    Image • 2.4 MB • 2h ago
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>

                            {/* Sample Item 2: agent.rs */}
                            <div
                              onClick={() => setRightPanelView('editor')}
                              className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group cursor-pointer"
                            >
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-neutral-100 text-neutral-700">
                                  <Code2 className="w-4 h-4" />
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    agent.rs
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5">
                                    Code • 12 KB • 4h ago
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>

                            {/* Sample Item 3: rencana-produk.pdf */}
                            <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-red-50 text-red-500">
                                  <FileText className="w-4 h-4" />
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    rencana-produk.pdf
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5">
                                    PDF • 1.1 MB • Yesterday
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          </>
                        )}
                      </div>

                      {/* View all artifacts link */}
                      <button
                        onClick={() => setRightPanelView('browser')}
                        className="text-xs font-medium text-neutral-700 hover:text-neutral-900 flex items-center gap-1 mt-3 px-1 cursor-pointer"
                      >
                        <span>View all artifacts</span>
                        <span>→</span>
                      </button>
                    </div>

                    {/* 1.5 Host Terminal Quick Launch Card */}
                    <div
                      onClick={() => setRightPanelView('terminal')}
                      className="flex items-center justify-between p-2.5 rounded-xl border border-neutral-200/90 bg-neutral-50/70 hover:bg-neutral-100/70 transition-all cursor-pointer group shadow-2xs"
                    >
                      <div className="flex items-center gap-2.5">
                        <div className="p-2 rounded-xl bg-neutral-900 text-white group-hover:scale-105 transition-transform">
                          <Terminal className="w-4 h-4" />
                        </div>
                        <div>
                          <div className="text-xs font-semibold text-neutral-900 leading-tight flex items-center gap-1.5">
                            <span>Host Terminal</span>
                            <span className="text-[9px] px-1.5 py-0.2 bg-emerald-50 text-emerald-600 border border-emerald-200 rounded font-normal">Active</span>
                          </div>
                          <div className="text-[10px] text-neutral-500 mt-0.5">
                            PowerShell / sh • Sandbox CLI • 18 Tools
                          </div>
                        </div>
                      </div>
                      <ChevronRight className="w-4 h-4 text-neutral-400 group-hover:text-neutral-700 transition-colors" />
                    </div>

                    {/* Divider */}
                    <div className="border-t border-neutral-100 my-3" />

                    {/* 2. Relays Section */}
                    <div>
                      <div className="flex items-center gap-2 mb-0.5">
                        <Radio className="w-4 h-4 text-neutral-900" />
                        <h2 className="font-bold text-sm text-neutral-900">Relays</h2>
                      </div>
                      <p className="text-[11px] text-neutral-500 mb-3">
                        Connect and control ODON from your chat apps.
                      </p>

                      <div className="space-y-2">
                        {/* Telegram */}
                        <div
                          onClick={() => setRelayModalOpen('telegram')}
                          className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors cursor-pointer group"
                          title="Configure Telegram Bot Relay"
                        >
                          <div className="flex items-center gap-2.5">
                            <div className="w-7 h-7 rounded-full bg-[#0088cc] text-white flex items-center justify-center">
                              <svg viewBox="0 0 24 24" fill="currentColor" className="w-3.5 h-3.5">
                                <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm4.64 6.8c-.15 1.58-.8 5.42-1.13 7.19-.14.75-.42 1-.68 1.03-.58.05-1.02-.38-1.58-.75-.88-.58-1.38-.94-2.23-1.5-.99-.65-.35-1.01.22-1.59.15-.15 2.71-2.48 2.76-2.69a.2.2 0 00-.05-.18c-.06-.05-.14-.03-.21-.02-.09.02-1.49.95-4.22 2.79-.4.27-.76.41-1.08.4-.36-.01-1.04-.2-1.55-.37-.63-.2-1.12-.31-1.08-.66.02-.18.27-.36.75-.55 2.92-1.27 4.86-2.11 5.83-2.52 2.77-1.16 3.35-1.36 3.73-1.37.08 0 .27.02.39.12.1.08.13.19.14.27-.01.06.01.24 0 .38z" />
                              </svg>
                            </div>
                            <span className="text-xs font-semibold text-neutral-900">Telegram</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <div className="flex items-center gap-1.5 text-[11px] text-neutral-600">
                              <span
                                className={`w-1.5 h-1.5 rounded-full ${systemStatus?.relays.telegram ? 'bg-emerald-500 animate-pulse' : 'bg-neutral-300'
                                  }`}
                              />
                              <span>{systemStatus?.relays.telegram ? 'Connected' : 'Standby'}</span>
                            </div>
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-700 transition-colors" />
                          </div>
                        </div>

                        {/* WhatsApp */}
                        <div
                          onClick={() => setRelayModalOpen('whatsapp')}
                          className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors cursor-pointer group"
                          title="Configure WhatsApp Bridge Relay"
                        >
                          <div className="flex items-center gap-2.5">
                            <div className="w-7 h-7 rounded-full bg-[#25d366] text-white flex items-center justify-center">
                              <svg viewBox="0 0 24 24" fill="currentColor" className="w-3.5 h-3.5">
                                <path d="M12.04 2c-5.46 0-9.91 4.45-9.91 9.91 0 1.75.46 3.45 1.32 4.95L2.05 22l5.25-1.38c1.45.79 3.08 1.21 4.74 1.21 5.46 0 9.91-4.45 9.91-9.91 0-2.65-1.03-5.14-2.9-7.01A9.816 9.816 0 0012.04 2zm0 18.15c-1.49 0-2.94-.4-4.22-1.16l-.3-.18-3.13.82.83-3.05-.2-.31a8.17 8.17 0 01-1.25-4.36c0-4.52 3.68-8.2 8.2-8.2 2.19 0 4.25.85 5.8 2.4 1.55 1.55 2.4 3.61 2.4 5.8 0 4.52-3.68 8.2-8.13 8.2zm4.49-6.14c-.25-.12-1.47-.72-1.7-.81-.23-.08-.39-.12-.56.12-.17.25-.64.81-.79.97-.14.17-.29.19-.54.06-.25-.12-1.05-.39-2-1.23-.74-.66-1.23-1.47-1.38-1.72-.14-.25-.02-.38.11-.5.11-.11.25-.29.37-.43.12-.14.17-.25.25-.41.08-.17.04-.31-.02-.43s-.56-1.34-.76-1.84c-.2-.48-.41-.42-.56-.43h-.48c-.17 0-.43.06-.66.31-.22.25-.86.84-.86 2.05s.88 2.38 1 2.55c.12.17 1.73 2.65 4.2 3.71.59.25 1.05.41 1.41.52.59.19 1.13.16 1.56.1.48-.07 1.47-.6 1.68-1.18.21-.58.21-1.07.14-1.18-.06-.11-.23-.17-.48-.3z" />
                              </svg>
                            </div>
                            <span className="text-xs font-semibold text-neutral-900">WhatsApp</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <div className="flex items-center gap-1.5 text-[11px] text-neutral-600">
                              <span
                                className={`w-1.5 h-1.5 rounded-full ${systemStatus?.relays.whatsapp ? 'bg-emerald-500 animate-pulse' : 'bg-neutral-300'
                                  }`}
                              />
                              <span>{systemStatus?.relays.whatsapp ? 'Connected' : 'Standby'}</span>
                            </div>
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-700 transition-colors" />
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* 3. Agent Activity Section at Bottom */}
                  <div className="pt-3 border-t border-neutral-100">
                    <div className="flex items-center gap-2 mb-1">
                      <Activity className="w-4 h-4 text-neutral-900" />
                      <h3 className="font-bold text-xs text-neutral-900">Agent Activity</h3>
                    </div>
                    <div className="flex items-center gap-1.5 text-xs text-neutral-500 mt-1">
                      <span
                        className={`w-1.5 h-1.5 rounded-full ${isGenerating ? 'bg-blue-600 animate-ping' : 'bg-emerald-500'
                          }`}
                      />
                      <span>
                        {isGenerating
                          ? streamingTool
                            ? `Executing tool: ${streamingTool}...`
                            : 'Synthesizing response...'
                          : 'Idle • Ready for new tasks'}
                      </span>
                    </div>
                  </div>
                </div>
              )}
            </aside>
          </>
        )}
      </div>

      {/* Approval & Audit Center Modal */}
      <ApprovalCenterModal
        isOpen={approvalCenterOpen}
        onClose={() => setApprovalCenterOpen(false)}
        pendingApproval={pendingApproval as any}
        onDecideApproval={decideApproval}
      />

      {/* Settings Modal */}
      <SettingsModal
        isOpen={settingsOpen}
        onClose={() => {
          setSettingsOpen(false);
          setCustomOpenAiModels(getCustomOpenAiModels());
        }}
        isDark={false}
        onToggleTheme={() => { }}
        activeProviderId={activeProviderId}
        onSelectProvider={handleSelectProvider}
        currentModel={selectedModel}
        onSelectModel={handleSelectModel}
      />

      {/* ─────────────────────────────────────────────────────────────
          FLOATING MODEL SELECTOR POPOVER (FOLLOWS SETTINGS PROVIDER)
      ───────────────────────────────────────────────────────────── */}
      {isModelMenuOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/20 backdrop-blur-2xs"
          onClick={() => setIsModelMenuOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[430px] max-h-[85vh] flex flex-col bg-white rounded-2xl border border-neutral-200 shadow-xl p-3.5 animate-in fade-in zoom-in-95 duration-100"
          >
            {/* Popover Header */}
            <div className="flex items-center justify-between px-1 pb-2.5 border-b border-neutral-100 mb-2">
              <div>
                <div className="font-bold text-xs text-neutral-900">Select AI Model</div>
                <div className="text-[11px] text-neutral-500 flex items-center gap-1.5 mt-0.5">
                  <span>Provider:</span>
                  <span className="font-semibold text-blue-600">{activeProvider?.name}</span>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => {
                    setIsModelMenuOpen(false);
                    setSettingsOpen(true);
                  }}
                  className="text-[11px] text-blue-600 hover:text-blue-800 font-medium underline cursor-pointer"
                  title="Configure API keys and providers in Settings"
                >
                  Configure in Settings →
                </button>
                <button
                  onClick={() => setIsModelMenuOpen(false)}
                  className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>

            {/* Quick Provider Switcher Strip */}
            <div className="flex items-center gap-1 overflow-x-auto pb-2 mb-2 shrink-0 border-b border-neutral-100">
              {providers.map((p) => {
                const isActive = p.id === activeProviderId;
                return (
                  <button
                    key={p.id}
                    onClick={() => handleSelectProvider(p.id)}
                    className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all shrink-0 cursor-pointer flex items-center gap-1 ${
                      isActive
                        ? 'bg-blue-600 text-white shadow-2xs'
                        : 'bg-neutral-100 hover:bg-neutral-200 text-neutral-600'
                    }`}
                  >
                    <span>{p.name.split(' (')[0]}</span>
                    {isActive && <Check className="w-2.5 h-2.5 text-white" />}
                  </button>
                );
              })}
            </div>

            {/* Models List belonging to Active Provider */}
            <div className="space-y-1.5 overflow-y-auto max-h-[360px] pr-1">
              <div className="text-[10px] uppercase font-bold text-neutral-400 tracking-wider px-1 mb-1 flex items-center justify-between">
                <span>{activeProvider?.name} Models ({activeModels.length})</span>
                {isOpenAiCompatible && (
                  <button
                    onClick={() => {
                      setIsModelMenuOpen(false);
                      setSettingsOpen(true);
                    }}
                    className="text-blue-600 hover:underline font-normal capitalize"
                  >
                    Kelola di Settings
                  </button>
                )}
              </div>

              {isOpenAiCompatible && activeModels.length === 0 && (
                <div className="p-3 rounded-xl bg-neutral-50 border border-dashed border-neutral-300 text-center text-xs text-neutral-500 my-2">
                  Belum ada model custom gateway. Isi Model ID manual di bawah atau buka Pengaturan.
                </div>
              )}

              {activeModels.map((m) => {
                const isSelected = selectedModel === m.id || selectedModel === m.name || `${activeProvider.id}::${m.id}` === selectedModel;
                return (
                  <div
                    key={m.id}
                    onClick={() => handleSelectModel(m.id)}
                    className={`w-full text-left p-2.5 rounded-xl transition-all cursor-pointer flex items-center justify-between border group ${
                      isSelected
                        ? 'bg-blue-50/80 border-blue-200 text-blue-900 shadow-2xs'
                        : 'border-transparent hover:border-neutral-200 hover:bg-neutral-50 text-neutral-800'
                    }`}
                  >
                    <div className="flex-1">
                      <div className="flex items-center gap-2 font-semibold text-xs">
                        <span>{m.name || m.id}</span>
                        {m.recommended && (
                          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-emerald-100 text-emerald-700 font-medium">
                            Recommended
                          </span>
                        )}
                      </div>
                      <div className="text-[11px] text-neutral-500 mt-0.5 flex items-center gap-2">
                        <span className="font-mono text-[10px] text-neutral-400">id: {m.id}</span>
                        <span>·</span>
                        <span>{m.context > 0 ? `${(m.context / 1000).toFixed(0)}k context` : 'auto context'}</span>
                      </div>
                    </div>

                    <div className="flex items-center gap-1.5">
                      {isSelected && <Check className="w-4 h-4 text-blue-600 shrink-0" />}
                      {isOpenAiCompatible && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDeleteQuickCustomModel(m.id);
                          }}
                          className="opacity-0 group-hover:opacity-100 p-1 text-neutral-400 hover:text-red-600 rounded transition-all cursor-pointer"
                          title="Hapus model ini"
                        >
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}

              {isOpenAiCompatible && (
                <div className="pt-2.5 mt-2 border-t border-neutral-200">
                  <div className="text-[10px] font-bold text-neutral-500 uppercase tracking-wider mb-1.5 px-0.5">
                    Input Model ID Manual
                  </div>
                  <div className="flex items-center gap-1.5">
                    <input
                      type="text"
                      value={quickModelInput}
                      onChange={(e) => setQuickModelInput(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') handleAddQuickCustomModel();
                      }}
                      placeholder="Ketik Model ID (misal: llama-3.3-70b)..."
                      className="flex-1 px-3 py-1.5 text-xs font-mono rounded-xl border border-neutral-300 bg-white focus:border-blue-500 outline-none"
                    />
                    <button
                      onClick={handleAddQuickCustomModel}
                      className="px-3 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded-xl text-xs font-medium transition-colors cursor-pointer shrink-0 shadow-2xs"
                    >
                      + Tambah
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          FLOATING TOOLS INVENTORY POPOVER
      ───────────────────────────────────────────────────────────── */}
      {isToolsMenuOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/20 backdrop-blur-2xs"
          onClick={() => setIsToolsMenuOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[440px] max-w-[90vw] max-h-[80vh] flex flex-col bg-white rounded-2xl border border-neutral-200 shadow-xl p-4 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 shrink-0">
              <div className="flex items-center gap-2">
                <Globe className="w-4 h-4 text-blue-600" />
                <span className="font-bold text-sm text-neutral-900">
                  Registered Tools ({REGISTERED_TOOLS.length})
                </span>
              </div>
              <button
                onClick={() => setIsToolsMenuOpen(false)}
                className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <p className="text-[11px] text-neutral-500 my-2 shrink-0">
              Autonomous execution harness capabilities available to the agent runtime.
            </p>
            <div className="flex-1 overflow-y-auto space-y-1.5 pr-1">
              {REGISTERED_TOOLS.map((t) => (
                <div
                  key={t.name}
                  className="p-2 rounded-xl border border-neutral-100 bg-neutral-50/60 flex items-center justify-between text-xs"
                >
                  <div>
                    <div className="flex items-center gap-2 font-mono font-semibold text-neutral-900">
                      <span>{t.name}</span>
                      <span className="text-[9px] font-sans px-1.5 py-0.2 rounded bg-neutral-200/80 text-neutral-600">
                        {t.category}
                      </span>
                    </div>
                    <div className="text-[10px] text-neutral-500 font-sans mt-0.5">{t.desc}</div>
                  </div>
                  <span
                    className={`text-[9px] font-medium px-2 py-0.5 rounded-full ${t.tier === 'Tier 0'
                        ? 'bg-emerald-50 text-emerald-700 border border-emerald-200'
                        : t.tier === 'Tier 1'
                          ? 'bg-amber-50 text-amber-700 border border-amber-200'
                          : 'bg-red-50 text-red-700 border border-red-200'
                      }`}
                  >
                    {t.tier}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          SEARCH SESSIONS & ARTIFACTS MODAL
      ───────────────────────────────────────────────────────────── */}
      {isSearchOpen && (
        <div
          className="fixed inset-0 z-50 flex items-start justify-center pt-20 bg-black/25 backdrop-blur-2xs"
          onClick={() => setIsSearchOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[480px] max-w-[90vw] bg-white rounded-2xl border border-neutral-200 shadow-2xl p-4 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center gap-2 pb-3 border-b border-neutral-100">
              <Search className="w-4 h-4 text-neutral-400" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search conversations and artifacts..."
                className="flex-1 text-xs outline-none bg-transparent text-neutral-900 placeholder-neutral-400 font-sans"
                autoFocus
              />
              <button
                onClick={() => setIsSearchOpen(false)}
                className="text-neutral-400 hover:text-neutral-700 p-1 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="mt-3 max-h-60 overflow-y-auto space-y-1">
              {sessions.filter((s) => s.title.toLowerCase().includes(searchQuery.toLowerCase())).length > 0 ? (
                sessions
                  .filter((s) => s.title.toLowerCase().includes(searchQuery.toLowerCase()))
                  .map((s) => (
                    <button
                      key={s.id}
                      onClick={() => {
                        loadSessionChat(s.id, s.title);
                        setIsSearchOpen(false);
                      }}
                      className="w-full text-left p-2 rounded-xl hover:bg-neutral-50 transition-colors flex items-center justify-between text-xs cursor-pointer"
                    >
                      <span className="font-medium text-neutral-800 truncate">{s.title}</span>
                      <span className="text-[10px] text-neutral-400">
                        {new Date(s.updated_at).toLocaleDateString()}
                      </span>
                    </button>
                  ))
              ) : (
                <div className="text-center py-6 text-neutral-400 text-xs">
                  {searchQuery ? 'No matching sessions found.' : 'Type to search previous conversations...'}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          SYSTEM ACTIVITY / NOTIFICATIONS MODAL
      ───────────────────────────────────────────────────────────── */}
      {isBellOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/25 backdrop-blur-2xs"
          onClick={() => setIsBellOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[420px] max-w-[90vw] bg-white rounded-2xl border border-neutral-200 shadow-2xl p-4 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100">
              <div className="flex items-center gap-2">
                <Bell className="w-4 h-4 text-blue-600" />
                <span className="font-bold text-sm text-neutral-900">System Status & Activity</span>
              </div>
              <button
                onClick={() => setIsBellOpen(false)}
                className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="mt-3 space-y-2.5 text-xs">
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Runtime Daemon</span>
                <span className={`font-semibold ${daemonOnline ? 'text-emerald-600' : 'text-amber-600'}`}>
                  {daemonOnline ? 'Online (127.0.0.1:4096)' : 'Offline (Local Standby)'}
                </span>
              </div>
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Registered Tools</span>
                <span className="font-semibold text-neutral-900">18 active tools (All Ready)</span>
              </div>
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Telegram Bot Relay</span>
                <span className="font-semibold text-neutral-700">
                  {systemStatus?.relays.telegram ? 'Connected' : 'Standby (Token not set)'}
                </span>
              </div>
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Persistence Engine</span>
                <span className="font-semibold text-neutral-900">SQLite State (`odon_state.db`)</span>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          RELAY SETUP MODAL (TELEGRAM / WHATSAPP)
      ───────────────────────────────────────────────────────────── */}
      {relayModalOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/25 backdrop-blur-2xs"
          onClick={() => setRelayModalOpen(null)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[450px] max-w-[90vw] bg-white rounded-2xl border border-neutral-200 shadow-2xl p-5 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center justify-between pb-3 border-b border-neutral-100">
              <div className="flex items-center gap-2">
                <Radio className="w-4 h-4 text-blue-600" />
                <span className="font-bold text-sm text-neutral-900">
                  {relayModalOpen === 'telegram' ? 'Telegram Bot Relay' : 'WhatsApp Bridge Relay'}
                </span>
              </div>
              <button
                onClick={() => setRelayModalOpen(null)}
                className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="mt-3.5 space-y-3 text-xs text-neutral-600 leading-relaxed">
              {relayModalOpen === 'telegram' ? (
                <>
                  <p>
                    ODON can be fully controlled directly from your personal or team Telegram chats. Commands sent to your bot execute autonomous agent turns and return rich markdown answers.
                  </p>
                  <div className="p-3 bg-neutral-50 rounded-xl border border-neutral-200/80 font-mono text-[11px] space-y-1">
                    <div className="text-neutral-500 font-sans text-[10px]">Start daemon with your bot token:</div>
                    <div className="text-neutral-800">$env:TELEGRAM_BOT_TOKEN="your_token"</div>
                    <div className="text-blue-600">odond serve --telegram-token "your_token"</div>
                  </div>
                  <div className="text-[11px] text-neutral-500">
                    Status:{' '}
                    <span className="font-semibold text-emerald-600">
                      {systemStatus?.relays.telegram ? 'Connected & Listening' : 'Waiting for Bot Token'}
                    </span>
                  </div>
                </>
              ) : (
                <>
                  <p>
                    WhatsApp relay connects ODON via a local WhatsApp bridge or cloud webhook. Messages sent to your WhatsApp number trigger background agent orchestration.
                  </p>
                  <div className="p-3 bg-neutral-50 rounded-xl border border-neutral-200/80 font-mono text-[11px] space-y-1">
                    <div className="text-neutral-500 font-sans text-[10px]">Webhook Listener Endpoint:</div>
                    <div className="text-neutral-800">http://127.0.0.1:4096/api/relays/whatsapp</div>
                  </div>
                  <div className="text-[11px] text-neutral-500">
                    Status: <span className="font-semibold text-amber-600">Standby (Ready for webhook pairing)</span>
                  </div>
                </>
              )}
            </div>
            <div className="mt-4 pt-3 border-t border-neutral-100 flex justify-end">
              <button
                onClick={() => setRelayModalOpen(null)}
                className="px-3.5 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium cursor-pointer transition-colors shadow-2xs"
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

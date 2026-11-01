export type SessionMode = 'general' | 'code' | 'create' | 'documents' | 'files' | 'agents' | 'automations';

export interface OutputActionChip {
  id: string;
  label: string;
  icon?: string;
  action_type: string;
  payload?: any;
}

export interface ToolCallInfo {
  call_id: string;
  tool_name: string;
  arguments: any;
  output?: any;
  is_error: boolean;
  requires_approval: boolean;
  is_approved?: boolean;
}

export interface MessageContent {
  text: string;
  thought?: string;
  tool_calls: ToolCallInfo[];
  artifact_ids: string[];
  action_chips: OutputActionChip[];
}

export interface Message {
  id: string;
  session_id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: MessageContent;
  model?: string;
  created_at: string;
}

export interface ArtifactFile {
  path: string;
  content: string;
}

export type ArtifactPayload =
  | { type: 'code'; data: { language: string; framework?: string; entry_file?: string; files: ArtifactFile[] } }
  | { type: 'document'; data: { doc_type: string; markdown: string; size_bytes: number } }
  | { type: 'image'; data: { prompt: string; width: number; height: number; file_path?: string; preview_url?: string; size_bytes: number; model: string } }
  | { type: 'patch'; data: { git_target: string; diff: string; files_changed: number; additions: number; deletions: number } }
  | { type: 'automation'; data: { trigger: string; schedule?: string; action: string; is_active: boolean } };

export interface Artifact {
  id: string;
  session_id: string;
  title: string;
  kind: 'code' | 'document' | 'image' | 'patch' | 'automation';
  summary: string;
  payload: ArtifactPayload;
  created_at: string;
}

export interface Session {
  id: string;
  title: string;
  mode: SessionMode;
  status: 'active' | 'archived' | 'completed';
  active_model: string;
  workspace_root?: string;
  created_at: string;
  updated_at: string;
}

export interface ToolSchema {
  name: string;
  description: string;
  parameters: any;
  risk_tier: number;
}

export interface ApprovalRequest {
  id: string;
  session_id: string;
  tool_name: string;
  risk_tier: number;
  description: string;
  parameters: any;
  created_at: string;
}

export interface SystemStatus {
  status: string;
  version: string;
  workspace_root: string;
  tools_count: number;
  execution_mode: string;
  relays: {
    desktop_api: boolean;
    sse_stream: boolean;
    cli: boolean;
    telegram: boolean;
    whatsapp: boolean;
  };
}

export type AgentEvent =
  | { type: 'state_changed'; payload: { session_id: string; state: string } }
  | { type: 'text_delta'; payload: { session_id: string; delta: string } }
  | { type: 'thought_delta'; payload: { session_id: string; delta: string } }
  | { type: 'tool_call_started'; payload: { session_id: string; call_id: string; tool_name: string; arguments: any } }
  | { type: 'tool_call_finished'; payload: { session_id: string; call_id: string; output: any; is_error: boolean } }
  | { type: 'artifact_created'; payload: { session_id: string; artifact: Artifact } }
  | { type: 'approval_requested'; payload: { session_id: string; approval_id: string; tool_name: string; risk_tier: number; description: string } }
  | { type: 'approval_resolved'; payload: { session_id: string; approval_id: string; decision: 'approved' | 'rejected' | 'expired' } }
  | { type: 'error'; payload: { session_id: string; message: string } };

export interface ModelItem {
  id: string;
  name: string;
  context: number;
  recommended?: boolean;
}

export interface ProviderItem {
  id: string;
  name: string;
  category: 'oauth' | 'byok';
  auth_type: 'oauth' | 'api_key';
  description: string;
  default_base_url?: string;
  connected?: boolean;
  configured?: boolean;
  models: ModelItem[];
}

export const FALLBACK_PROVIDERS: ProviderItem[] = [
  {
    id: 'openai',
    name: 'OpenAI (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Official OpenAI platform with GPT-4o, o3-mini, and GPT-4.5',
    models: [
      { id: 'gpt-4o', name: 'GPT-4o (Omnimodal Flagship)', context: 128000, recommended: true },
      { id: 'o3-mini', name: 'o3-mini (Reasoning & Coding)', context: 200000, recommended: true },
      { id: 'gpt-4o-mini', name: 'GPT-4o Mini (Fast & Light)', context: 128000 },
      { id: 'o1', name: 'o1 (Deep Reasoning)', context: 200000 },
      { id: 'gpt-4.5-preview', name: 'GPT-4.5 Preview', context: 128000 },
    ],
  },
  {
    id: 'anthropic',
    name: 'Anthropic Claude',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Anthropic frontier reasoning and coding architecture',
    models: [
      { id: 'claude-3-7-sonnet', name: 'Claude 3.7 Sonnet (Hybrid Reasoning)', context: 200000, recommended: true },
      { id: 'claude-3-5-sonnet', name: 'Claude 3.5 Sonnet v2', context: 200000, recommended: true },
      { id: 'claude-3-5-haiku', name: 'Claude 3.5 Haiku', context: 200000 },
    ],
  },
  {
    id: 'deepseek',
    name: 'DeepSeek',
    category: 'byok',
    auth_type: 'api_key',
    description: 'DeepSeek V3 and R1 reasoning models with transparent chain-of-thought',
    models: [
      { id: 'deepseek-chat', name: 'DeepSeek-V3 (Chat & Coding)', context: 64000, recommended: true },
      { id: 'deepseek-reasoner', name: 'DeepSeek-R1 (Deep Reasoning)', context: 64000, recommended: true },
    ],
  },
  {
    id: 'google',
    name: 'Google Gemini',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Google Gemini 2.0 and 1.5 Pro with large context window',
    models: [
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash', context: 1048576, recommended: true },
      { id: 'gemini-1.5-pro', name: 'Gemini 1.5 Pro', context: 2097152 },
    ],
  },
  {
    id: 'ollama',
    name: 'Ollama (Local Offline)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Run open-weights models 100% locally on localhost:11434',
    models: [
      { id: 'llama3.3:latest', name: 'Llama 3.3 70B Local', context: 32768, recommended: true },
      { id: 'qwen2.5-coder:32b', name: 'Qwen 2.5 Coder 32B Local', context: 32768, recommended: true },
      { id: 'deepseek-r1:14b', name: 'DeepSeek R1 14B Local', context: 32768 },
    ],
  },
  {
    id: 'antigravity',
    name: 'Google Antigravity',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Antigravity developer ecosystem with hybrid reasoning',
    models: [
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash (Antigravity)', context: 1048576, recommended: true },
      { id: 'claude-3-7-sonnet', name: 'Claude 3.7 Sonnet (Antigravity)', context: 200000, recommended: true },
    ],
  },
  {
    id: 'chatgpt',
    name: 'ChatGPT / OpenAI Codex',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Direct ChatGPT and Codex account authorization',
    models: [
      { id: 'gpt-4o', name: 'GPT-4o (Codex)', context: 128000, recommended: true },
      { id: 'o3-mini', name: 'o3-mini (Codex)', context: 200000 },
    ],
  },
];

export interface TerminalInfo {
  shell: string;
  os: string;
  arch: string;
  default_cwd: string;
  user: string;
}

export interface TerminalExecResult {
  exit_code: number;
  stdout: string;
  stderr: string;
  cwd: string;
  success: boolean;
}

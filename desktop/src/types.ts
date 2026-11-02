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

export interface MessageAttachment {
  name: string;
  mime_type: string;
  data_base64: string;
}

export interface MessageContent {
  text: string;
  thought?: string;
  tool_calls: ToolCallInfo[];
  artifact_ids: string[];
  action_chips: OutputActionChip[];
  attachments?: MessageAttachment[];
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
    id: 'github-copilot',
    name: 'GitHub Copilot',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'GitHub Copilot subscription models via the Copilot API',
    models: [
      { id: 'gpt-4o', name: 'GPT-4o (GitHub Copilot)', context: 128000, recommended: true },
      { id: 'gpt-4.1', name: 'GPT-4.1 (GitHub Copilot)', context: 1047576, recommended: true },
      { id: 'claude-3.7-sonnet', name: 'Claude 3.7 Sonnet (GitHub Copilot)', context: 200000 },
      { id: 'o3-mini', name: 'o3-mini (GitHub Copilot)', context: 200000 },
    ],
  },
  {
    id: 'antigravity',
    name: 'Google Antigravity',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Google Cloud Code Assist & Antigravity OAuth (Gemini and Claude)',
    models: [
      { id: 'gemini-2.5-pro', name: 'Gemini 2.5 Pro', context: 1048576, recommended: true },
      { id: 'gemini-2.5-flash', name: 'Gemini 2.5 Flash', context: 1048576, recommended: true },
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash', context: 1048576 },
      { id: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4 (Antigravity)', context: 200000 },
    ],
  },
  {
    id: 'chatgpt',
    name: 'ChatGPT / OpenAI Codex',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Official ChatGPT account authorization through the Codex backend',
    models: [
      { id: 'gpt-5-codex', name: 'GPT-5 Codex', context: 400000, recommended: true },
      { id: 'gpt-5', name: 'GPT-5', context: 400000, recommended: true },
      { id: 'gpt-5-mini', name: 'GPT-5 mini', context: 400000 },
      { id: 'gpt-4.1', name: 'GPT-4.1', context: 1047576 },
    ],
  },
  {
    id: 'claude-oauth',
    name: 'Claude (Anthropic OAuth)',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Direct Anthropic Claude account authorization',
    models: [
      { id: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4', context: 200000, recommended: true },
      { id: 'claude-opus-4-20250514', name: 'Claude Opus 4', context: 200000, recommended: true },
      { id: 'claude-3-7-sonnet-20250219', name: 'Claude 3.7 Sonnet', context: 200000 },
      { id: 'claude-3-5-haiku-20241022', name: 'Claude 3.5 Haiku', context: 200000 },
    ],
  },
  {
    id: 'xai-oauth',
    name: 'xAI Grok',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'xAI Grok device authorization',
    models: [
      { id: 'grok-4', name: 'Grok 4', context: 256000, recommended: true },
      { id: 'grok-3', name: 'Grok 3', context: 131072 },
      { id: 'grok-3-mini', name: 'Grok 3 mini', context: 131072 },
    ],
  },
  {
    id: 'muse',
    name: 'Meta Muse Code',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Meta Muse Code developer device authorization and SSE streaming',
    models: [
      { id: 'muse-spark-2.0', name: 'Muse Spark 2.0 (Flagship Coding)', context: 256000, recommended: true },
      { id: 'muse-code-v3', name: 'Muse Code v3', context: 128000 },
    ],
  },
  {
    id: 'openai',
    name: 'OpenAI (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Official OpenAI API models',
    default_base_url: 'https://api.openai.com/v1',
    models: [
      { id: 'gpt-4o', name: 'GPT-4o', context: 128000, recommended: true },
      { id: 'gpt-4.1', name: 'GPT-4.1', context: 1047576, recommended: true },
      { id: 'gpt-4o-mini', name: 'GPT-4o mini', context: 128000 },
      { id: 'o3', name: 'o3 (Reasoning)', context: 200000 },
      { id: 'o4-mini', name: 'o4-mini (Reasoning)', context: 200000 },
    ],
  },
  {
    id: 'anthropic',
    name: 'Anthropic (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Official Anthropic API with Claude models',
    default_base_url: 'https://api.anthropic.com/v1',
    models: [
      { id: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4', context: 200000, recommended: true },
      { id: 'claude-opus-4-20250514', name: 'Claude Opus 4', context: 200000, recommended: true },
      { id: 'claude-3-7-sonnet-20250219', name: 'Claude 3.7 Sonnet', context: 200000 },
      { id: 'claude-3-5-haiku-20241022', name: 'Claude 3.5 Haiku', context: 200000 },
    ],
  },
  {
    id: 'google',
    name: 'Google Gemini (Official BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Google Gemini API with 1M context and native multimodal reasoning',
    default_base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
    models: [
      { id: 'gemini-2.5-pro', name: 'Gemini 2.5 Pro', context: 1048576, recommended: true },
      { id: 'gemini-2.5-flash', name: 'Gemini 2.5 Flash', context: 1048576, recommended: true },
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash', context: 1048576 },
      { id: 'gemini-1.5-flash', name: 'Gemini 1.5 Flash', context: 1048576 },
    ],
  },
  {
    id: 'deepseek',
    name: 'DeepSeek (BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'DeepSeek chat and reasoning models',
    default_base_url: 'https://api.deepseek.com/v1',
    models: [
      { id: 'deepseek-chat', name: 'DeepSeek Chat', context: 128000, recommended: true },
      { id: 'deepseek-reasoner', name: 'DeepSeek Reasoner', context: 128000, recommended: true },
    ],
  },
  {
    id: 'qwen',
    name: 'Qwen Plan / DashScope Global',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Alibaba Cloud international AI gateway (Qwen Max, Qwen Plus, Qwen Coder)',
    default_base_url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
    models: [
      { id: 'qwen-max', name: 'Qwen Max', context: 131072, recommended: true },
      { id: 'qwen-plus', name: 'Qwen Plus', context: 131072, recommended: true },
      { id: 'qwen2.5-coder-32b-instruct', name: 'Qwen 2.5 Coder 32B', context: 131072, recommended: true },
      { id: 'qwq-32b', name: 'QwQ 32B Reasoning', context: 32768 },
    ],
  },
  {
    id: 'mistral',
    name: 'Mistral AI',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Mistral AI models including Large, Medium, and Codestral',
    default_base_url: 'https://api.mistral.ai/v1',
    models: [
      { id: 'mistral-large-latest', name: 'Mistral Large', context: 128000, recommended: true },
      { id: 'mistral-medium-latest', name: 'Mistral Medium', context: 128000, recommended: true },
      { id: 'mistral-small-latest', name: 'Mistral Small', context: 128000 },
      { id: 'codestral-latest', name: 'Codestral', context: 256000, recommended: true },
      { id: 'ministral-8b-latest', name: 'Ministral 8B', context: 128000 },
    ],
  },
  {
    id: 'groq',
    name: 'Groq (Fast Inference)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Ultra high-speed LPU inference for open-weights models',
    default_base_url: 'https://api.groq.com/openai/v1',
    models: [
      { id: 'llama-3.3-70b-versatile', name: 'Llama 3.3 70B Versatile', context: 131072, recommended: true },
      { id: 'llama-3.1-8b-instant', name: 'Llama 3.1 8B Instant', context: 131072, recommended: true },
      { id: 'qwen/qwen3-32b', name: 'Qwen 3 32B', context: 131072 },
      { id: 'deepseek-r1-distill-llama-70b', name: 'DeepSeek R1 Distill 70B', context: 131072 },
    ],
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Unified gateway across all frontier models and providers',
    default_base_url: 'https://openrouter.ai/api/v1',
    models: [
      { id: 'openai/gpt-4o', name: 'GPT-4o (OpenRouter)', context: 128000, recommended: true },
      { id: 'anthropic/claude-sonnet-4', name: 'Claude Sonnet 4 (OpenRouter)', context: 200000, recommended: true },
      { id: 'google/gemini-2.5-pro', name: 'Gemini 2.5 Pro (OpenRouter)', context: 1048576, recommended: true },
      { id: 'deepseek/deepseek-chat', name: 'DeepSeek Chat (OpenRouter)', context: 128000 },
    ],
  },
  {
    id: 'ollama',
    name: 'Ollama (Local Models)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Run open models entirely locally on your hardware without internet',
    default_base_url: 'http://localhost:11434/v1',
    models: [
      { id: 'llama3.3', name: 'Llama 3.3 70B', context: 131072, recommended: true },
      { id: 'qwen2.5-coder:32b', name: 'Qwen 2.5 Coder 32B', context: 131072, recommended: true },
      { id: 'deepseek-r1:32b', name: 'DeepSeek R1 32B', context: 131072 },
      { id: 'llama3.1:8b', name: 'Llama 3.1 8B', context: 131072 },
    ],
  },
  {
    id: 'openai-compatible',
    name: 'OpenAI Compatible (Custom / Gateway)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Any OpenAI-compatible gateway: vLLM, LM Studio, LiteLLM, or custom proxies',
    default_base_url: 'http://localhost:11434/v1',
    models: [],
  },
];

export const CUSTOM_OPENAI_MODELS_STORAGE_KEY = 'odon_custom_models_openai_compatible';

export function getCustomOpenAiModels(): ModelItem[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = localStorage.getItem(CUSTOM_OPENAI_MODELS_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.length > 0) {
        return parsed.filter((m) => m && typeof m.id === 'string' && m.id.trim().length > 0);
      }
    }
  } catch {}
  return [];
}

export function saveCustomOpenAiModels(models: ModelItem[]): void {
  if (typeof window === 'undefined') return;
  const valid = models.filter((m) => m && typeof m.id === 'string' && m.id.trim().length > 0);
  localStorage.setItem(CUSTOM_OPENAI_MODELS_STORAGE_KEY, JSON.stringify(valid));
}

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

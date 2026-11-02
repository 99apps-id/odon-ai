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
  | { type: 'steer_applied'; payload: { session_id: string; steer_id: string; text: string } }
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
  is_custom?: boolean;
  models: ModelItem[];
}

export const FALLBACK_PROVIDERS: ProviderItem[] = [
  {
    id: 'github-copilot',
    name: 'GitHub Copilot',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'GitHub Copilot subscription models (live model-picker list; retired models removed)',
    // Sources: docs.github.com/copilot/reference/ai-models/supported-models
    // and api.githubcopilot.com/models (model_picker_enabled, type=chat).
    models: [
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol', context: 400000, recommended: true },
      { id: 'claude-sonnet-5.5', name: 'Claude Sonnet 5.5', context: 328000, recommended: true },
      { id: 'gpt-5.3-codex', name: 'GPT-5.3-Codex', context: 400000, recommended: true },
      { id: 'gemini-3.8-flash', name: 'Gemini 3.8 Flash', context: 265536, recommended: true },
      { id: 'gpt-6.1-terra', name: 'GPT-6.1 Terra', context: 400000 },
      { id: 'gpt-6-astra', name: 'GPT-6 Astra', context: 400000 },
      { id: 'gpt-6-sol', name: 'GPT-6 Sol', context: 400000 },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna', context: 400000 },
      { id: 'gpt-5.6-sol', name: 'GPT-5.6 Sol', context: 400000 },
      { id: 'gpt-5.6-terra', name: 'GPT-5.6 Terra', context: 400000 },
      { id: 'gpt-5.6-luna', name: 'GPT-5.6 Luna', context: 328000 },
      { id: 'claude-sonnet-5', name: 'Claude Sonnet 5', context: 264000 },
      { id: 'claude-opus-5.5', name: 'Claude Opus 5.5', context: 328000 },
      { id: 'claude-opus-5', name: 'Claude Opus 5', context: 264000 },
      { id: 'claude-opus-4.8', name: 'Claude Opus 4.8', context: 264000 },
      { id: 'claude-opus-4.8-fast', name: 'Claude Opus 4.8 (fast mode)', context: 264000 },
      { id: 'claude-haiku-5.5', name: 'Claude Haiku 5.5', context: 200000 },
      { id: 'claude-haiku-4.5', name: 'Claude Haiku 4.5', context: 200000 },
      { id: 'claude-fable-5.1', name: 'Claude Fable 5.1', context: 264000 },
      { id: 'claude-fable-5', name: 'Claude Fable 5', context: 264000 },
      { id: 'gemini-4-argon', name: 'Gemini 4 Argon', context: 265536 },
      { id: 'grok-4.7', name: 'Grok 4.7', context: 328000 },
      { id: 'grok-4.6', name: 'Grok 4.6', context: 328000 },
      { id: 'kimi-k3', name: 'Kimi K3', context: 1048576 },
      { id: 'mai-code-1.1-flash', name: 'MAI-Code-1.1-Flash', context: 256000 },
    ],
  },
  {
    id: 'antigravity',
    name: 'Google Antigravity',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Google Cloud Code Assist & Antigravity OAuth (Gemini and Claude)',
    models: [
      { id: 'gemini-4-argon', name: 'Gemini 4 Argon', context: 2000000, recommended: true },
      { id: 'gemini-3.1-pro', name: 'Gemini 3.1 Pro', context: 1048576, recommended: true },
      { id: 'gemini-3.8-flash-medium', name: 'Gemini 3.8 Flash', context: 1048576, recommended: true },
      { id: 'gemini-3.8-flash-high', name: 'Gemini 3.8 Flash High', context: 1048576 },
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', context: 1000000 },
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', context: 1000000 },
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', context: 1000000 },
    ],
  },
  {
    id: 'chatgpt',
    name: 'ChatGPT / OpenAI Codex',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Official ChatGPT account authorization through the Codex backend',
    models: [
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol', context: 500000, recommended: true },
      { id: 'gpt-6-astra', name: 'GPT-6 Astra', context: 272000, recommended: true },
      { id: 'gpt-6.1-terra', name: 'GPT-6.1 Terra', context: 500000, recommended: true },
      { id: 'gpt-6-sol', name: 'GPT-6 Sol', context: 272000 },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna', context: 272000 },
      { id: 'gpt-5.6-sol', name: 'GPT-5.6 Sol', context: 272000 },
      { id: 'gpt-5.6-terra', name: 'GPT-5.6 Terra', context: 272000 },
      { id: 'gpt-5.6-luna', name: 'GPT-5.6 Luna', context: 272000 },
    ],
  },
  {
    id: 'claude-oauth',
    name: 'Claude (Anthropic OAuth)',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Direct Anthropic Claude account authorization',
    models: [
      { id: 'claude-fable-5-1', name: 'Claude Fable 5.1', context: 1000000, recommended: true },
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', context: 1000000, recommended: true },
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', context: 1000000, recommended: true },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5', context: 1000000 },
      { id: 'claude-opus-5', name: 'Claude Opus 5', context: 1000000 },
      { id: 'claude-sonnet-5', name: 'Claude Sonnet 5', context: 1000000 },
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', context: 1000000 },
      { id: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', context: 200000 },
      { id: 'claude-fable-5', name: 'Claude Fable 5', context: 1000000 },
    ],
  },
  {
    id: 'xai-oauth',
    name: 'xAI Grok',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'xAI Grok device authorization',
    models: [
      { id: 'grok-4.7', name: 'Grok 4.7', context: 500000, recommended: true },
      { id: 'grok-4.6', name: 'Grok 4.6', context: 500000 },
      { id: 'grok-4.5', name: 'Grok 4.5', context: 500000 },
      { id: 'grok-build-0.1', name: 'Grok Build 0.1', context: 256000 },
    ],
  },
  {
    id: 'muse',
    name: 'Meta Muse Code',
    category: 'oauth',
    auth_type: 'oauth',
    description: 'Meta Muse Code developer device authorization and SSE streaming',
    models: [
      { id: 'muse-spark-1.3', name: 'Muse Spark 1.3', context: 1000000, recommended: true },
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
      { id: 'gpt-6-astra', name: 'GPT-6 Astra', context: 1050000, recommended: true },
      { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol', context: 1050000, recommended: true },
      { id: 'gpt-6.1-terra', name: 'GPT-6.1 Terra', context: 1050000, recommended: true },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna', context: 1050000 },
      { id: 'gpt-6-sol', name: 'GPT-6 Sol', context: 1050000 },
      { id: 'gpt-5.6-sol', name: 'GPT-5.6 Sol', context: 1050000 },
      { id: 'gpt-5.6-terra', name: 'GPT-5.6 Terra', context: 1050000 },
      { id: 'gpt-5.6-luna', name: 'GPT-5.6 Luna', context: 1050000 },
      { id: 'gpt-5.5', name: 'GPT-5.5', context: 1050000 },
      { id: 'gpt-5.4', name: 'GPT-5.4', context: 1050000 },
      { id: 'gpt-5.4-mini', name: 'GPT-5.4 mini', context: 400000 },
      { id: 'gpt-5.3-codex', name: 'GPT-5.3 Codex', context: 1000000 },
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
      { id: 'claude-fable-5-1', name: 'Claude Fable 5.1', context: 1000000, recommended: true },
      { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', context: 1000000, recommended: true },
      { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', context: 1000000, recommended: true },
      { id: 'claude-haiku-5-5', name: 'Claude Haiku 5.5', context: 1000000 },
      { id: 'claude-opus-5', name: 'Claude Opus 5', context: 1000000 },
      { id: 'claude-sonnet-5', name: 'Claude Sonnet 5', context: 1000000 },
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', context: 1000000 },
      { id: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', context: 200000 },
      { id: 'claude-fable-5', name: 'Claude Fable 5', context: 1000000 },
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
      { id: 'gemini-4-argon', name: 'Gemini 4 Argon', context: 1000000, recommended: true },
      { id: 'gemini-3.8-flash', name: 'Gemini 3.8 Flash', context: 1000000, recommended: true },
      { id: 'gemini-3.1-pro-preview', name: 'Gemini 3.1 Pro', context: 1000000, recommended: true },
      { id: 'gemini-3.6-flash', name: 'Gemini 3.6 Flash', context: 1000000 },
      { id: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash-Lite', context: 1000000 },
      { id: 'gemini-3.1-flash-lite', name: 'Gemini 3.1 Flash-Lite', context: 1000000 },
      { id: 'gemini-3-flash-preview', name: 'Gemini 3 Flash (Preview)', context: 1000000 },
    ],
  },
  {
    id: 'deepseek',
    name: 'DeepSeek (BYOK)',
    category: 'byok',
    auth_type: 'api_key',
    description: 'DeepSeek V4 Pro and V4.1 Flash (1M context)',
    default_base_url: 'https://api.deepseek.com/v1',
    models: [
      { id: 'deepseek-v4-pro', name: 'DeepSeek V4 Pro', context: 1000000, recommended: true },
      { id: 'deepseek-flash', name: 'DeepSeek V4.1 Flash', context: 1000000, recommended: true },
    ],
  },
  {
    id: 'qwen',
    name: 'Qwen Plan / DashScope Global',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Alibaba Cloud international AI gateway (Qwen3.8 Max, Plus, Flash, 27B)',
    default_base_url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
    models: [
      { id: 'qwen3.8-max', name: 'Qwen3.8 Max', context: 1000000, recommended: true },
      { id: 'qwen3.8-flash', name: 'Qwen3.8 Flash', context: 1000000, recommended: true },
      { id: 'qwen3.8-27b', name: 'Qwen3.8 27B', context: 1000000, recommended: true },
      { id: 'qwen3.7-plus', name: 'Qwen3.7 Plus', context: 1000000 },
    ],
  },
  {
    id: 'mistral',
    name: 'Mistral AI',
    category: 'byok',
    auth_type: 'api_key',
    description: 'Mistral AI models including Large 4, Small 4, Codestral, and Ministral 3',
    default_base_url: 'https://api.mistral.ai/v1',
    models: [
      { id: 'mistral-large-latest', name: 'Mistral Large 4', context: 1000000, recommended: true },
      { id: 'mistral-small-latest', name: 'Mistral Small 4', context: 256000, recommended: true },
      { id: 'codestral-latest', name: 'Codestral 25.08', context: 128000, recommended: true },
      { id: 'ministral-3-8b-25-12', name: 'Ministral 3 8B', context: 256000 },
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
      { id: 'openai/gpt-oss-120b', name: 'GPT-OSS 120B (Groq)', context: 131072, recommended: true },
      { id: 'openai/gpt-oss-20b', name: 'GPT-OSS 20B (Groq)', context: 131072, recommended: true },
      { id: 'qwen/qwen3.8-27b', name: 'Qwen3.8 27B (Groq)', context: 131072, recommended: true },
      { id: 'llama-3.3-70b-versatile', name: 'Llama 3.3 70B (Groq)', context: 131072 },
      { id: 'llama-3.1-8b-instant', name: 'Llama 3.1 8B (Groq)', context: 131072 },
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
      { id: 'openrouter/auto', name: 'OpenRouter Auto', context: 2000000, recommended: true },
      { id: 'anthropic/claude-opus-5.5', name: 'Claude Opus 5.5 (OpenRouter)', context: 1000000, recommended: true },
      { id: 'openai/gpt-6-astra', name: 'GPT-6 Astra (OpenRouter)', context: 1050000, recommended: true },
      { id: 'moonshotai/kimi-k3.1-pro', name: 'Kimi K3.1 Pro (OpenRouter)', context: 1048576 },
      { id: 'z-ai/glm-5.3', name: 'GLM 5.3 (OpenRouter)', context: 1048576 },
      { id: 'qwen/qwen3.8-max-prime', name: 'Qwen3.8 Max Prime (OpenRouter)', context: 1000000 },
      { id: 'deepseek/deepseek-v4-pro', name: 'DeepSeek V4 Pro (OpenRouter)', context: 1048576 },
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
      { id: 'qwen3.8:27b', name: 'Qwen3.8 27B', context: 131072, recommended: true },
      { id: 'qwen3-coder:30b', name: 'Qwen3 Coder 30B', context: 131072, recommended: true },
      { id: 'gpt-oss:20b', name: 'GPT-OSS 20B', context: 131072, recommended: true },
      { id: 'glm-4.7-flash:latest', name: 'GLM 4.7 Flash', context: 131072 },
      { id: 'deepseek-v4-flash:latest', name: 'DeepSeek V4 Flash', context: 131072 },
      { id: 'qwen2.5-coder:latest', name: 'Qwen2.5 Coder', context: 32768 },
      { id: 'llama3.2:latest', name: 'Llama 3.2', context: 131072 },
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

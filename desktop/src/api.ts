import {
  ProviderItem,
  Session,
  Message,
  Artifact,
  ToolSchema,
  ApprovalRequest,
  SystemStatus,
  AgentEvent,
  TerminalInfo,
  TerminalExecResult,
} from './types';

export interface OAuthAccountInfo {
  provider: string;
  email: string | null;
  plan: string | null;
  account_id: string | null;
  connected: boolean;
}

export interface OAuthLoginResponse {
  auth_url?: string;
  user_code?: string;
  verification_uri?: string;
  device_code?: string;
  status: 'pending_browser' | 'pending_device' | 'completed';
  account?: OAuthAccountInfo;
}

export interface FsEntryItem {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified_at?: string;
  extension?: string;
}

export interface FsListResult {
  current_path: string;
  workspace_root: string;
  entries: FsEntryItem[];
  error?: string;
}

export interface FsReadResult {
  path: string;
  name: string;
  size: number;
  is_text: boolean;
  content?: string;
  base64?: string;
  error?: string;
}

export interface SshProfile {
  id: string;
  alias: string;
  host: string;
  port: number;
  user: string;
  key_path?: string;
  certificate_filename?: string;
  certificate_data?: string;
  auth_type: string;
  description?: string;
  created_at: string;
}

export interface SkillItemInfo {
  name: string;
  title: string;
  description: string;
  path: string;
  content: string;
}

export interface GitHubUser {
  login: string;
  name: string;
  avatar_url: string;
  bio?: string;
}

export interface GitHubStatusResponse {
  authenticated: boolean;
  auth_source?: string;
  user?: GitHubUser;
  workspace_repo?: { owner: string; repo: string };
}

export interface GitHubRepoItem {
  name: string;
  owner: string;
  full_name: string;
  is_private: boolean;
  description: string;
}

export const api = {
  getBaseUrl(): string {
    if (typeof window !== 'undefined') {
      const explicit = localStorage.getItem('odon_api_base');
      if (explicit) return explicit;
      // In browser development (Vite on port 3000) or served from daemon (port 4096),
      // empty string uses the current host/proxy, avoiding CORS or 127.0.0.1 mismatch.
      return '';
    }
    return 'http://127.0.0.1:4096';
  },

  setBaseUrl(url: string) {
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_api_base', url.replace(/\/+$/, ''));
    }
  },

  async getStatus(): Promise<SystemStatus> {
    const res = await fetch(`${this.getBaseUrl()}/api/status`);
    if (!res.ok) throw new Error(`HTTP error ${res.status}`);
    return res.json();
  },

  async getTools(): Promise<any[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/tools`);
    if (!res.ok) throw new Error(`Failed to list tools: ${res.statusText}`);
    return res.json();
  },

  async listSessions(): Promise<Session[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions`);
    if (!res.ok) throw new Error(`Failed to list sessions: ${res.statusText}`);
    return res.json();
  },

  async createSession(title: string, mode: string = 'general', model: string = 'odon-auto'): Promise<Session> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title, mode, model }),
    });
    if (!res.ok) throw new Error(`Failed to create session: ${res.statusText}`);
    return res.json();
  },

  async getSession(id: string): Promise<{ session: Session; messages: Message[]; artifacts: Artifact[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${id}`);
    if (!res.ok) throw new Error(`Failed to get session: ${res.statusText}`);
    return res.json();
  },

  async deleteSession(id: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async listMessages(sessionId: string): Promise<Message[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${sessionId}/messages`);
    if (!res.ok) return [];
    return res.json();
  },

  async sendMessage(
    sessionId: string,
    text: string,
    model?: string,
    effort?: string,
    attachments?: Array<{ name: string; mime_type: string; data_base64: string }>,
  ): Promise<Message> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${sessionId}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text, model, effort, attachments: attachments || [] }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Send failed with status ${res.status}`);
    }
    const data = await res.json();
    return data.message;
  },

  async testProvider(providerId: string, model?: string): Promise<{
    ok: boolean;
    provider: string;
    model?: string;
    latency_ms?: number;
    sample?: string;
    error?: string;
  }> {
    const res = await fetch(`${this.getBaseUrl()}/api/providers/test`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider_id: providerId, model }),
    });
    if (!res.ok) {
      let detail = '';
      try {
        const body = await res.json();
        detail = body.error || JSON.stringify(body);
      } catch {
        detail = await res.text().catch(() => '');
      }
      return { ok: false, provider: providerId, error: detail || `HTTP ${res.status}` };
    }
    return res.json();
  },

  async listArtifacts(sessionId: string): Promise<Artifact[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/sessions/${sessionId}/artifacts`);
    if (!res.ok) return [];
    return res.json();
  },

  async getArtifact(id: string): Promise<Artifact> {
    const res = await fetch(`${this.getBaseUrl()}/api/artifacts/${id}`);
    if (!res.ok) throw new Error(`Failed to fetch artifact ${id}`);
    return res.json();
  },

  async listTools(): Promise<ToolSchema[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/tools`);
    if (!res.ok) return [];
    return res.json();
  },

  async listApprovals(): Promise<ApprovalRequest[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals`);
    if (!res.ok) return [];
    return res.json();
  },

  async resolveApproval(id: string, decision: 'approve_once' | 'approve_always' | 'deny', reason?: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals/${id}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ decision, reason }),
    });
    return res.ok;
  },

  async getModels(): Promise<{ providers: ProviderItem[]; catalog_as_of?: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/models`);
    if (!res.ok) throw new Error(`Failed to fetch models: ${res.statusText}`);
    return res.json();
  },

  async getTerminalInfo(): Promise<TerminalInfo> {
    const res = await fetch(`${this.getBaseUrl()}/api/terminal/info`);
    if (!res.ok) throw new Error(`Failed to fetch terminal info: ${res.statusText}`);
    return res.json();
  },

  async executeTerminal(command: string, cwd?: string): Promise<TerminalExecResult> {
    const res = await fetch(`${this.getBaseUrl()}/api/terminal/exec`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command, cwd }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Execution failed with status ${res.status}`);
    }
    return res.json();
  },

  async loginOAuth(provider: string): Promise<OAuthLoginResponse> {
    const res = await fetch(`${this.getBaseUrl()}/api/oauth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `OAuth sign-in failed with status ${res.status}`);
    }
    return res.json();
  },

  async getOAuthAccounts(): Promise<OAuthAccountInfo[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/oauth/accounts`);
    if (!res.ok) return [];
    return res.json();
  },

  async getProviderConfigs(): Promise<Array<{
    provider_id: string;
    configured: boolean;
    masked_key: string;
    base_url?: string;
    is_default: boolean;
  }>> {
    const res = await fetch(`${this.getBaseUrl()}/api/providers/config`);
    if (!res.ok) return [];
    return res.json();
  },

  async saveProviderConfig(provider_id: string, api_key: string, base_url?: string, is_default?: boolean): Promise<any> {
    const res = await fetch(`${this.getBaseUrl()}/api/providers/config`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider_id, api_key, base_url, is_default }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Save failed with status ${res.status}`);
    }
    return res.json();
  },

  async deleteProviderConfig(provider_id: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/providers/config/${provider_id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async disconnectOAuth(provider: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/oauth/disconnect`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider }),
    });
    return res.ok;
  },

  // Approvals & Audit Trail
  async getApprovalHistory(): Promise<any[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals/history`);
    if (!res.ok) return [];
    return res.json();
  },

  async getApprovalPolicies(): Promise<string[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals/policies`);
    if (!res.ok) return [];
    return res.json();
  },

  async deleteApprovalPolicy(key: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/approvals/policies/${encodeURIComponent(key)}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async getSettings(): Promise<{ execution_mode: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/status`);
    if (!res.ok) return { execution_mode: 'Autonomous' };
    const data = await res.json();
    return { execution_mode: data.execution_mode || 'Autonomous' };
  },

  async updateSettings(settings: Record<string, unknown>): Promise<{ updated: boolean }> {
    const res = await fetch(`${this.getBaseUrl()}/api/settings`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(settings),
    });
    if (!res.ok) return { updated: true };
    return res.json().catch(() => ({ updated: true }));
  },

  // MCP Servers
  async getMcpServers(): Promise<any[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/mcp/servers`);
    if (!res.ok) return [];
    return res.json();
  },

  async saveMcpServer(server: any): Promise<any> {
    const res = await fetch(`${this.getBaseUrl()}/api/mcp/servers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(server),
    });
    return res.json();
  },

  async deleteMcpServer(id: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/mcp/servers/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async toggleMcpServer(id: string): Promise<any> {
    const res = await fetch(`${this.getBaseUrl()}/api/mcp/servers/${id}/toggle`, {
      method: 'POST',
    });
    return res.json();
  },

  // GitHub
  async getGitHubStatus(): Promise<GitHubStatusResponse> {
    const res = await fetch(`${this.getBaseUrl()}/api/github/status`);
    if (!res.ok) return { authenticated: false };
    return res.json();
  },

  async getGitHubUserRepos(): Promise<{ repos: GitHubRepoItem[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/github/user-repos`);
    if (!res.ok) return { repos: [] };
    return res.json();
  },

  async getGitHubPrs(owner?: string, repo?: string): Promise<any[]> {
    const params = new URLSearchParams();
    if (owner) params.append('owner', owner);
    if (repo) params.append('repo', repo);
    const res = await fetch(`${this.getBaseUrl()}/api/github/prs?${params.toString()}`);
    if (!res.ok) return [];
    return res.json();
  },

  async getGitHubIssues(owner?: string, repo?: string, state: string = 'open'): Promise<any[]> {
    const params = new URLSearchParams();
    if (owner) params.append('owner', owner);
    if (repo) params.append('repo', repo);
    params.append('state', state);
    const res = await fetch(`${this.getBaseUrl()}/api/github/issues?${params.toString()}`);
    if (!res.ok) return [];
    return res.json();
  },

  async createGitHubIssue(owner: string, repo: string, title: string, body?: string): Promise<any> {
    const res = await fetch(`${this.getBaseUrl()}/api/github/issues`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ owner, repo, title, body }),
    });
    return res.json();
  },

  // Automations
  async getAutomations(): Promise<any[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/automations`);
    if (!res.ok) return [];
    return res.json();
  },

  async saveAutomation(job: any): Promise<any> {
    const res = await fetch(`${this.getBaseUrl()}/api/automations`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(job),
    });
    return res.json();
  },

  async deleteAutomation(id: string): Promise<boolean> {
    const res = await fetch(`${this.getBaseUrl()}/api/automations/${id}`, {
      method: 'DELETE',
    });
    return res.ok;
  },

  async triggerAutomation(id: string): Promise<any> {
    const res = await fetch(`${this.getBaseUrl()}/api/automations/${id}/trigger`, {
      method: 'POST',
    });
    return res.json();
  },

  async getAutomationLogs(): Promise<any[]> {
    const res = await fetch(`${this.getBaseUrl()}/api/automations/logs`);
    if (!res.ok) return [];
    return res.json();
  },

  // File Explorer APIs
  async fsList(path?: string): Promise<FsListResult> {
    const url = path ? `${this.getBaseUrl()}/api/fs/list?path=${encodeURIComponent(path)}` : `${this.getBaseUrl()}/api/fs/list`;
    const res = await fetch(url);
    if (!res.ok) return { current_path: '', workspace_root: '', entries: [], error: 'Failed to list directory' };
    return res.json();
  },

  async fsRead(path: string): Promise<FsReadResult> {
    const res = await fetch(`${this.getBaseUrl()}/api/fs/read?path=${encodeURIComponent(path)}`);
    if (!res.ok) return { path, name: '', size: 0, is_text: false, error: 'Failed to read file' };
    return res.json();
  },

  async fsWrite(path: string, content: string): Promise<{ success: boolean; error?: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/fs/write`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, content }),
    });
    return res.json();
  },

  async fsMkdir(path: string): Promise<{ success: boolean; error?: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/fs/mkdir`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    });
    return res.json();
  },

  async fsDelete(path: string): Promise<{ success: boolean; error?: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/fs/delete?path=${encodeURIComponent(path)}`, {
      method: 'DELETE',
    });
    return res.json();
  },

  // SSH Remote APIs
  async getSshProfiles(): Promise<{ profiles: SshProfile[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/ssh/profiles`);
    if (!res.ok) return { profiles: [] };
    return res.json();
  },

  async saveSshProfile(profile: SshProfile): Promise<{ success: boolean; profiles: SshProfile[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/ssh/profiles`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(profile),
    });
    return res.json();
  },

  async deleteSshProfile(id: string): Promise<{ success: boolean; profiles: SshProfile[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/ssh/profiles/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
    return res.json();
  },

  async execSsh(payload: { profile_id?: string; host?: string; port?: number; user?: string; key_path?: string; command: string }): Promise<TerminalExecResult> {
    const res = await fetch(`${this.getBaseUrl()}/api/ssh/exec`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      return { exit_code: 1, stdout: '', stderr: 'SSH execution request failed', success: false, cwd: '' };
    }
    return res.json();
  },

  async importSshCertificate(filename: string, content: string, profile_id?: string): Promise<{ success: boolean; saved_path: string; filename: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/ssh/certificates/import`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ filename, content, profile_id }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `Import certificate failed with status ${res.status}`);
    }
    return res.json();
  },

  getTerminalWsUrl(params?: { shell?: string; cwd?: string; ssh_profile_id?: string; cols?: number; rows?: number }): string {
    const base = this.getBaseUrl().replace(/^http/, 'ws');
    const query = new URLSearchParams();
    if (params?.shell) query.set('shell', params.shell);
    if (params?.cwd) query.set('cwd', params.cwd);
    if (params?.ssh_profile_id) query.set('ssh_profile_id', params.ssh_profile_id);
    if (params?.cols) query.set('cols', params.cols.toString());
    if (params?.rows) query.set('rows', params.rows.toString());
    const qs = query.toString();
    return `${base}/api/terminal/ws${qs ? `?${qs}` : ''}`;
  },

  // Skills APIs
  async getSkills(): Promise<{ skills: SkillItemInfo[] }> {
    const res = await fetch(`${this.getBaseUrl()}/api/skills`);
    if (!res.ok) return { skills: [] };
    return res.json();
  },

  async saveSkill(name: string, content: string): Promise<{ success: boolean; path?: string; error?: string }> {
    const res = await fetch(`${this.getBaseUrl()}/api/skills`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, content }),
    });
    return res.json();
  },

  async deleteSkill(name: string): Promise<{ success: boolean }> {
    const res = await fetch(`${this.getBaseUrl()}/api/skills/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    });
    return res.json();
  },

  createTerminalWs(): WebSocket {
    const baseUrl = this.getBaseUrl();
    const wsUrl = baseUrl.replace(/^http/, 'ws') + '/api/terminal/ws';
    return new WebSocket(wsUrl);
  },

  subscribeEvents(onEvent: (event: AgentEvent) => void): () => void {
    const es = new EventSource(`${this.getBaseUrl()}/api/events`);

    es.onmessage = (e) => {
      try {
        const parsed = JSON.parse(e.data);
        onEvent(parsed);
      } catch (err) {
        console.error('Failed to parse SSE event:', err);
      }
    };

    es.onerror = () => {
      // EventSource reconnects automatically
    };

    return () => {
      es.close();
    };
  },
};

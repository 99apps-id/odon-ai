/**
 * Collapse a run of tool calls into one grey line, the way a coding agent
 * reads its own work back: "Edited 2 files, explored 3 files, ran 2 commands".
 *
 * Two rules keep the line honest:
 *
 * - Clause order is fixed, so the same run always reads the same way no matter
 *   which category happens to be live.
 * - Only a category holding a single item names that item. Anything larger
 *   counts, because "edited 9 files" is the useful reading and a list of nine
 *   paths is not.
 */

export type RunCategory =
  | 'edit'
  | 'explore'
  | 'search'
  | 'read'
  | 'browse'
  | 'analyze'
  | 'run'
  | 'delegate'
  | 'other';

const CATEGORY_ORDER: readonly RunCategory[] = [
  'edit',
  'explore',
  'search',
  'read',
  'browse',
  'analyze',
  'run',
  'delegate',
  'other',
];

interface CategoryCopy {
  noun: [string, string];
  past: string;
  present: string;
}

const CATEGORY_COPY: Record<RunCategory, CategoryCopy> = {
  edit: { noun: ['file', 'files'], past: 'Edited', present: 'Editing' },
  explore: { noun: ['file', 'files'], past: 'Explored', present: 'Exploring' },
  search: { noun: ['query', 'queries'], past: 'Searched', present: 'Searching' },
  read: { noun: ['page', 'pages'], past: 'Read', present: 'Reading' },
  browse: { noun: ['page', 'pages'], past: 'Opened', present: 'Opening' },
  analyze: { noun: ['image', 'images'], past: 'Analyzed', present: 'Analyzing' },
  run: { noun: ['command', 'commands'], past: 'Ran', present: 'Running' },
  delegate: { noun: ['task', 'tasks'], past: 'Delegated', present: 'Delegating' },
  other: { noun: ['tool', 'tools'], past: 'Used', present: 'Using' },
};

/**
 * Routed by exact name, never by substring: a web search must not count as an
 * explored file, and a browser screenshot fetches nothing so it is not a page.
 */
const TOOL_CATEGORY: Record<string, RunCategory> = {
  // edits
  edit: 'edit',
  write_file: 'edit',
  multi_edit: 'edit',
  fs_patch: 'edit',
  delete_file: 'edit',
  move_file: 'edit',
  move_file_recursive: 'edit',
  copy_file: 'edit',
  fs_move_recursive: 'edit',
  fs_copy_recursive: 'edit',
  fs_zip: 'edit',
  fs_unzip: 'edit',
  format_code: 'edit',
  create_directory: 'edit',

  // reading the workspace
  read_file: 'explore',
  list_directory: 'explore',
  glob: 'explore',
  grep: 'explore',
  fs_tree: 'explore',
  fs_find: 'explore',
  fs_stat: 'explore',
  fs_du: 'explore',
  fs_diff: 'explore',
  code_index: 'explore',
  code_search: 'explore',
  file_checksum: 'explore',
  file_hash: 'explore',
  disk_usage: 'explore',

  // looking things up
  web_search: 'search',
  web_search_code: 'search',
  web_search_news: 'search',
  memory_search: 'search',
  memory_recall: 'search',
  audit_search: 'search',
  gmaps_search: 'search',

  // pulling a page in
  web_fetch: 'read',
  web_scrape: 'read',
  http_get: 'read',
  url_preview: 'read',
  mcp_call: 'read',

  // driving a browser
  browser_navigate: 'browse',
  browser_open: 'browse',
  browser_session_navigate: 'browse',
  browser_click: 'browse',
  browser_session_click: 'browse',
  browser_session_create: 'browse',
  browser_session_text: 'browse',
  browser_session_type: 'browse',
  browser_session_select: 'browse',
  browser_session_wait: 'browse',

  // looking at pixels
  browser_screenshot: 'analyze',
  browser_session_screenshot: 'analyze',
  artifact_create_image: 'analyze',

  // executing
  shell_exec: 'run',
  exec_background: 'run',
  exec_stream_stdout: 'run',
  exec_background_kill: 'run',
  run_checks: 'run',
  project_task: 'run',
  test_file: 'run',
  dev_server_start: 'run',
  dev_server_stop: 'run',

  // handing work out
  run_subagent: 'delegate',
  run_subagents: 'delegate',
};

export function toolCategory(toolName: string): RunCategory {
  return TOOL_CATEGORY[toolName] ?? 'other';
}

/** The verb a category reads as while it is still happening. */
export function toolPresentVerb(toolName: string): string {
  return CATEGORY_COPY[toolCategory(toolName)].present;
}

export interface RunnableTool {
  name: string;
  /** One-line argument summary produced by the caller. */
  args?: string;
  isError?: boolean;
}

/** Last path segment, so a clause names `wiring.tsx` and not its whole path. */
function basename(value: string): string {
  const trimmed = value.replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return cut >= 0 ? trimmed.slice(cut + 1) : trimmed;
}

function hostOf(value: string): string {
  try {
    return new URL(value).hostname;
  } catch {
    return value;
  }
}

function compact(value: string, max: number): string {
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}

/**
 * What a lone call acted on. Only used when a category holds exactly one call,
 * so the line can name the thing instead of counting to one.
 */
function toolTarget(tool: RunnableTool, category: RunCategory): string {
  const args = (tool.args ?? '').trim();
  if (!args) return '';

  if (category === 'search') return `"${compact(args, 48)}"`;
  if (category === 'read' || category === 'browse') return hostOf(args);
  if (category === 'edit' || category === 'explore') return basename(args);

  return compact(args, 56);
}

function clause(category: RunCategory, tools: RunnableTool[], live: boolean): string {
  const copy = CATEGORY_COPY[category];
  const verb = live ? copy.present : copy.past;
  const count = tools.length;
  const target = count === 1 ? toolTarget(tools[0], category) : '';

  // A finished command is more useful counted than quoted; a running one earns
  // its space because it is the thing you are waiting on.
  if (target && (live || category !== 'run')) {
    return `${verb} ${target}`;
  }

  return `${verb} ${count} ${copy.noun[count === 1 ? 0 : 1]}`;
}

/**
 * The single grey line that stands in for a run of tool calls.
 *
 * `live` is the caller's to say, not something readable off the calls: a call
 * can be left without a result by a turn that ended or an agent that moved on,
 * and a run like that has to read as finished rather than narrate work that
 * stopped happening.
 */
export function summarizeToolRun(tools: readonly RunnableTool[], live: boolean): string {
  if (tools.length === 0) return '';

  const byCategory = new Map<RunCategory, RunnableTool[]>();
  for (const tool of tools) {
    const category = toolCategory(tool.name);
    const bucket = byCategory.get(category);
    if (bucket) bucket.push(tool);
    else byCategory.set(category, [tool]);
  }

  // While live, the category holding the most recent call speaks in the present
  // tense, so the line reads as work in progress rather than work already done.
  const liveCategory = live ? toolCategory(tools[tools.length - 1].name) : null;

  const clauses: string[] = [];
  for (const category of CATEGORY_ORDER) {
    const bucket = byCategory.get(category);
    if (!bucket || bucket.length === 0) continue;
    clauses.push(clause(category, bucket, live && category === liveCategory));
  }

  return clauses.join(', ');
}

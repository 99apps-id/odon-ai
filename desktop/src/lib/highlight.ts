// Minimal, dependency-free syntax tokenizer for chat code blocks.
//
// Design notes:
// - Returns plain data (no HTML strings) so React renders text nodes and a
//   pasted snippet can never inject markup.
// - One line at a time keeps long blocks cheap and preserves line numbers.
// - Unknown languages fall back to plain text instead of guessing.
// - Colors live in tokenClass -> CSS class and are checked for >=4.5:1
//   contrast on white, on the tinted surface and on diff tints.

export type Token = { text: string; kind: TokenKind };
export type TokenKind =
  | 'plain'
  | 'comment'
  | 'keyword'
  | 'string'
  | 'number'
  | 'fn'
  | 'type'
  | 'punct';

const KEYWORDS: Record<string, string[]> = {
  js: 'const let var function return if else for while do break continue class extends new this super import from export default async await try catch finally throw typeof instanceof delete void yield switch case in of null undefined true false'.split(' '),
  rust: 'fn let mut const static struct enum trait impl for while loop match if else return use pub mod crate self super as where async await dyn move ref type unsafe extern in break continue'.split(' '),
  python: 'def class return if elif else for while import from as with try except finally raise pass break continue lambda yield global nonlocal assert del not and or is in None True False async await'.split(' '),
  bash: 'if then else elif fi for while do done case esac function return export local readonly unset set echo cd source sudo exit in'.split(' '),
  sql: 'select from where insert into values update set delete create table alter drop index join left right inner outer on group by order having limit offset union all as distinct and or not null is like in exists'.split(' '),
  go: 'func package import var const type struct interface map chan go defer select range switch case default if else for return break continue fallthrough nil true false'.split(' '),
  json: 'true false null'.split(' '),
};

const FAMILY: Record<string, keyof typeof KEYWORDS> = {
  js: 'js', javascript: 'js', jsx: 'js', ts: 'js', typescript: 'js', tsx: 'js', mjs: 'js', cjs: 'js',
  rust: 'rust', rs: 'rust',
  python: 'python', py: 'python',
  bash: 'bash', sh: 'bash', shell: 'bash', zsh: 'bash', powershell: 'bash', ps1: 'bash', cmd: 'bash', terminal: 'bash',
  sql: 'sql',
  go: 'go', golang: 'go',
  json: 'json', jsonc: 'json',
};

const ESCAPED = '\\\\[\\s\\S]';

interface Rule {
  kind: TokenKind;
  re: RegExp;
}

function rulesFor(family: string | null): Rule[] {
  const lineComment =
    family === 'python' || family === 'bash' ? '#[^\\n]*' : family === 'sql' ? '--[^\\n]*' : '//[^\\n]*';
  const keywords = family ? KEYWORDS[family] : [];
  const kwAlt = keywords
    .slice()
    .sort((a, b) => b.length - a.length)
    .map((k) => k.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
    .join('|');

  const rules: Rule[] = [
    { kind: 'comment', re: new RegExp(`^(?:${lineComment}|/\\*[\\s\\S]*?\\*/)`) },
    { kind: 'string', re: new RegExp(`^(?:"(?:${ESCAPED}|[^"\\\\])*"|'(?:${ESCAPED}|[^'\\\\])*'|\`(?:${ESCAPED}|[^\`\\\\])*\`)`) },
    { kind: 'number', re: /^(?:0[xb][0-9a-fA-F_]+|\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?)/ },
  ];
  if (kwAlt) {
    rules.push({ kind: 'keyword', re: new RegExp(`^(?:${kwAlt})\\b`) });
  }
  rules.push(
    { kind: 'fn', re: /^[A-Za-z_$][\w$]*(?=\s*\()/ },
    { kind: 'type', re: /^[A-Z][A-Za-z0-9_$]*/ },
    { kind: 'plain', re: /^[A-Za-z_$][\w$]*/ },
    { kind: 'punct', re: /^[^\sA-Za-z0-9_$]+/ },
  );
  return rules;
}

const RULE_CACHE = new Map<string, Rule[]>();

function cachedRules(family: string | null): Rule[] {
  const key = family ?? '';
  const hit = RULE_CACHE.get(key);
  if (hit) return hit;
  const made = rulesFor(family);
  RULE_CACHE.set(key, made);
  return made;
}

export function languageFamily(language: string): string | null {
  const lang = language.trim().toLowerCase();
  return (FAMILY[lang] as string | undefined) ?? null;
}

export function isDiffLanguage(language: string): boolean {
  const lang = language.trim().toLowerCase();
  return lang === 'diff' || lang === 'patch';
}

/** Tokenize one line of source. Never throws. */
export function tokenizeLine(line: string, family: string | null): Token[] {
  if (!line) return [];
  const rules = cachedRules(family);
  const out: Token[] = [];
  let rest = line;
  // Bounded loop: every branch below either advances `rest` or is not taken,
  // so an unexpected regex match on zero length cannot spin forever.
  let guard = 0;
  while (rest.length > 0 && guard < 4096) {
    guard += 1;
    let matched = false;
    for (const rule of rules) {
      const m = rule.re.exec(rest);
      if (m && m[0].length > 0) {
        out.push({ text: m[0], kind: rule.kind });
        rest = rest.slice(m[0].length);
        matched = true;
        break;
      }
    }
    if (!matched) {
      const ch = rest[0];
      const last = out[out.length - 1];
      if (last && last.kind === 'plain') last.text += ch;
      else out.push({ text: ch, kind: 'plain' });
      rest = rest.slice(1);
    }
  }
  return out;
}

export function tokenize(code: string, language: string): Token[][] {
  const family = languageFamily(language);
  if (!family) return code.split('\n').map((l) => (l ? [{ text: l, kind: 'plain' as TokenKind }] : []));
  return code.split('\n').map((line) => tokenizeLine(line, family));
}

export const TOKEN_CLASS: Record<TokenKind, string> = {
  plain: 'text-[#2f332e]',
  comment: 'text-[#6f6a5d] italic',
  keyword: 'text-[#8a4429] font-medium',
  string: 'text-[#3f5d3a]',
  number: 'text-[#3f5b70]',
  fn: 'text-[#245052]',
  type: 'text-[#5b4636]',
  punct: 'text-[#5a5a52]',
};

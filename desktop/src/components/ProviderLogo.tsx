import React from 'react';

export type ProviderLogoId =
  | 'antigravity'
  | 'google'
  | 'gemini'
  | 'chatgpt'
  | 'codex'
  | 'openai'
  | 'claude'
  | 'claude-oauth'
  | 'anthropic'
  | 'deepseek'
  | 'qwen'
  | 'alibaba'
  | 'stepfun'
  | 'stepfun-plan'
  | 'muse'
  | 'meta'
  | 'xai'
  | 'xai-oauth'
  | 'grok'
  | 'github'
  | 'github-copilot'
  | 'copilot'
  | 'groq'
  | 'mistral'
  | 'openrouter'
  | 'ollama'
  | 'lmstudio'
  | 'openai_compatible'
  | 'openai-compatible'
  | 'custom'
  | string;

interface ProviderLogoProps {
  providerId: ProviderLogoId;
  size?: number;
  className?: string;
  colored?: boolean;
}

export const ProviderLogo: React.FC<ProviderLogoProps> = ({
  providerId,
  size = 18,
  className = '',
  colored = true,
}) => {
  const norm = providerId.toLowerCase().replace(/_/g, '-');

  // 1. OpenAI / ChatGPT / Codex (Official Spiral Rosette)
  if (norm.includes('chatgpt') || norm.includes('openai') || norm.includes('codex')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813v6.7227zm1.145-2.0365l2.5484-1.4729 2.5532 1.4729v2.9363l-2.5532 1.473-2.5484-1.473V10.8265z"
          fill={colored ? '#10A37F' : 'currentColor'}
        />
      </svg>
    );
  }

  // 2. Anthropic / Claude (Official Terracotta Sunburst / Asterisk)
  if (norm.includes('claude') || norm.includes('anthropic')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M13.82 2.05a1.2 1.2 0 0 0-1.64.44L9.84 6.63a.6.6 0 0 1-.78.27L4.54 5.23a1.2 1.2 0 0 0-1.55.72 1.2 1.2 0 0 0 .44 1.43l4.31 2.65a.6.6 0 0 1 .28.78L5.7 15.34a1.2 1.2 0 0 0 .44 1.64 1.2 1.2 0 0 0 1.64-.44l2.34-4.14a.6.6 0 0 1 .78-.27l4.52 1.67a1.2 1.2 0 0 0 1.55-.72 1.2 1.2 0 0 0-.44-1.43l-4.31-2.65a.6.6 0 0 1-.28-.78l2.32-4.53a1.2 1.2 0 0 0-.44-1.64z"
          fill={colored ? '#D97757' : 'currentColor'}
        />
        <path
          d="M19.46 8.7a1.2 1.2 0 0 0-1.55-.72l-4.52 1.67a.6.6 0 0 1-.78-.27l-2.34-4.14a1.2 1.2 0 0 0-1.64-.44 1.2 1.2 0 0 0-.44 1.64l2.32 4.53a.6.6 0 0 1-.28.78L6.14 14.4a1.2 1.2 0 0 0-.44 1.43 1.2 1.2 0 0 0 1.55.72l4.52-1.67a.6.6 0 0 1 .78.27l2.34 4.14a1.2 1.2 0 0 0 1.64.44 1.2 1.2 0 0 0 .44-1.64l-2.32-4.53a.6.6 0 0 1 .28-.78l4.31-2.65a1.2 1.2 0 0 0 .44-1.43z"
          fill={colored ? '#D97757' : 'currentColor'}
          opacity="0.8"
        />
      </svg>
    );
  }

  // 3. Google Gemini / Antigravity (Official 4-Point Star Sparkle Gradient)
  if (norm.includes('antigravity') || norm.includes('google') || norm.includes('gemini')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <defs>
          <linearGradient id={`gemini-grad-${size}`} x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#4285F4" />
            <stop offset="50%" stopColor="#9B72CB" />
            <stop offset="100%" stopColor="#D96570" />
          </linearGradient>
        </defs>
        <path
          d="M12 2C12 7.52285 7.52285 12 2 12C7.52285 12 12 16.4771 12 22C12 16.4771 16.4771 12 22 12C16.4771 12 12 7.52285 12 2Z"
          fill={colored ? `url(#gemini-grad-${size})` : 'currentColor'}
        />
      </svg>
    );
  }

  // 4. DeepSeek (Official Whale Emblem)
  if (norm.includes('deepseek')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M21.2 12.8c-.4-3.8-3.4-6.8-7.2-7.2-2.1-.2-4.2.4-5.8 1.8L4.6 4.2C4.2 3.8 3.5 4.1 3.5 4.7v5.6c0 .4.3.7.7.7h5.6c.6 0 .9-.7.5-1.1L8.2 7.8c1.2-1 2.7-1.5 4.3-1.4 3 .3 5.4 2.7 5.7 5.7.3 3.4-2.2 6.3-5.6 6.5-1.7.1-3.3-.6-4.4-1.8-.4-.4-1.1-.4-1.5 0l-.7.7c-.4.4-.4 1.1 0 1.5 1.7 1.7 4 2.6 6.4 2.5 4.6-.3 8.3-4 8.7-8.7z"
          fill={colored ? '#0C8CE9' : 'currentColor'}
        />
        <circle cx="15.5" cy="10" r="1.5" fill={colored ? '#0C8CE9' : 'currentColor'} />
      </svg>
    );
  }

  // 5. Qwen / Alibaba Cloud (Official Aliyun Orange Hex Bracket)
  if (norm.includes('qwen') || norm.includes('alibaba') || norm.includes('dashscope')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M4 8.5L8.5 4H15.5L20 8.5V15.5L15.5 20H8.5L4 15.5V8.5Z"
          stroke={colored ? '#FF6A00' : 'currentColor'}
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M8.5 12H15.5M12 8.5V15.5"
          stroke={colored ? '#FF6A00' : 'currentColor'}
          strokeWidth="2.2"
          strokeLinecap="round"
        />
      </svg>
    );
  }

  // 6. StepFun / 阶跃星辰 (Official Stepped Geometric Mark)
  if (norm.includes('stepfun') || norm.includes('step-')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M3 18H7V14H11V10H15V6H21V10H17V14H13V18H9V22H3V18Z"
          fill={colored ? '#0A84FF' : 'currentColor'}
        />
      </svg>
    );
  }

  // 7. Meta / Muse Code (Official Meta Infinity Loop)
  if (norm.includes('muse') || norm.includes('meta')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M7.5 7C4.46 7 2 9.46 2 12.5C2 15.54 4.46 18 7.5 18C10.04 18 11.45 15.86 12 14.88C12.55 15.86 13.96 18 16.5 18C19.54 18 22 15.54 22 12.5C22 9.46 19.54 7 16.5 7C13.96 7 12.55 9.14 12 10.12C11.45 9.14 10.04 7 7.5 7ZM7.5 9C9.28 9 10.45 10.59 11.15 12C10.45 13.41 9.28 15 7.5 15C6.12 15 5 13.88 5 12.5C5 11.12 6.12 10 7.5 10V9ZM16.5 9C17.88 9 19 10.12 19 12.5C19 13.88 17.88 15 16.5 15C14.72 15 13.55 13.41 12.85 12C13.55 10.59 14.72 9 16.5 9Z"
          fill={colored ? '#0081FB' : 'currentColor'}
        />
      </svg>
    );
  }

  // 8. xAI / Grok (Official Slashed X Mark)
  if (norm.includes('xai') || norm.includes('grok')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"
          fill="currentColor"
        />
      </svg>
    );
  }

  // 9. GitHub / Copilot (Official Octocat Silhouette)
  if (norm.includes('github') || norm.includes('copilot')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          fillRule="evenodd"
          clipRule="evenodd"
          d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.53 1.032 1.53 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z"
          fill="currentColor"
        />
      </svg>
    );
  }

  // 10. Groq (Official Lightning Strike Mark)
  if (norm.includes('groq')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M13 2L3 14H12L11 22L21 10H12L13 2Z"
          fill={colored ? '#F55036' : 'currentColor'}
        />
      </svg>
    );
  }

  // 11. Mistral (Official Orange Chevron Mark)
  if (norm.includes('mistral')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M4 6H8V18H4V6ZM10 10H14V18H10V10ZM16 14H20V18H16V14Z"
          fill={colored ? '#F83A00' : 'currentColor'}
        />
      </svg>
    );
  }

  // 12. OpenRouter (Official Compass Hexagon)
  if (norm.includes('openrouter')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M12 2L20.5 7V17L12 22L3.5 17V7L12 2Z"
          stroke={colored ? '#6366F1' : 'currentColor'}
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <circle cx="12" cy="12" r="3" fill={colored ? '#6366F1' : 'currentColor'} />
      </svg>
    );
  }

  // 13. Ollama (Official Ollama Llama Silhouette)
  if (norm.includes('ollama')) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
      >
        <path
          d="M8 3V9L5 12V21H11V16H13V21H19V12L16 9V3H8ZM10 5H14V8H10V5Z"
          fill="currentColor"
        />
      </svg>
    );
  }

  // Default: Generic Plug / Key
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <path
        d="M9 2V5M15 2V5M5 9H19C20.1 9 21 9.9 21 11V13C21 16.87 17.87 20 14 20H10C6.13 20 3 16.87 3 13V11C3 9.9 3.9 9 5 9Z"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
};

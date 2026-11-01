import React from 'react';

interface CatLogoProps {
  className?: string;
  strokeWidth?: number;
}

export function CatLogo({ className = 'w-8 h-8', strokeWidth = 2.4 }: CatLogoProps) {
  return (
    <svg
      viewBox="0 0 100 86"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      {/* Cat Head Contour with pointed ears */}
      <path
        d="M 27 22
           C 25 17 21 13 18 13
           C 16 13 15 16 16 20
           C 18 30 19 38 20 43
           C 14 50 14 62 21 72
           C 29 82 43 85 50 85
           C 57 85 71 82 79 72
           C 86 62 86 50 80 43
           C 81 38 82 30 84 20
           C 85 16 84 13 82 13
           C 79 13 75 17 73 22
           C 67 30 58 33 50 33
           C 42 33 33 30 27 22 Z"
        stroke="currentColor"
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
      />

      {/* Two Eyes */}
      <circle cx="37" cy="53" r="3.2" fill="currentColor" />
      <circle cx="63" cy="53" r="3.2" fill="currentColor" />

      {/* Small Nose */}
      <ellipse cx="50" cy="60" rx="2.2" ry="1.6" fill="currentColor" />

      {/* Happy Cat Mouth (:3) */}
      <path
        d="M 43.5 63 C 43.5 67 47 68 50 64 C 53 68 56.5 67 56.5 63"
        stroke="currentColor"
        strokeWidth={strokeWidth * 0.9}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

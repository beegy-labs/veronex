export function HexLogo({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Veronex"
    >
      <defs>
        <linearGradient id="hex-grad" x1="2.5" y1="4.3" x2="29.5" y2="27.7" gradientUnits="userSpaceOnUse">
          <stop offset="0%"   style={{ stopColor: 'var(--theme-logo-start)' }} />
          <stop offset="100%" style={{ stopColor: 'var(--theme-logo-end)' }} />
        </linearGradient>
      </defs>
      <polygon
        points="29.5,16 22.8,27.7 9.2,27.7 2.5,16 9.2,4.3 22.8,4.3"
        fill="url(#hex-grad)"
      />
      <polygon
        points="25,16 20.5,23.8 11.5,23.8 7,16 11.5,8.2 20.5,8.2"
        fill="none"
        stroke="white"
        strokeWidth="1.5"
        strokeOpacity="0.55"
      />
    </svg>
  )
}

export function OllamaIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="currentColor"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Ollama"
    >
      <path d="M7.5 1.5 C7 1.5 6.5 2 6.5 2.5 L6.5 5 C6.5 5.5 7 6 7.5 6 L9 6 C9.5 6 10 5.5 10 5 L10 2.5 C10 2 9.5 1.5 9 1.5 Z" />
      <path d="M15 1.5 C14.5 1.5 14 2 14 2.5 L14 5 C14 5.5 14.5 6 15 6 L16.5 6 C17 6 17.5 5.5 17.5 5 L17.5 2.5 C17.5 2 17 1.5 16.5 1.5 Z" />
      <ellipse cx="12" cy="9" rx="5.5" ry="4.5" />
      <path d="M9.5 13 L9.5 16 C9.5 16.5 10 17 10.5 17 L13.5 17 C14 17 14.5 16.5 14.5 16 L14.5 13 Z" />
      <rect x="6.5" y="16.5" width="11" height="6" rx="3" />
    </svg>
  )
}

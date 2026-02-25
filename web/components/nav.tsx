'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { LayoutDashboard, List, Key, FlaskConical, Server, BarChart2, Gauge } from 'lucide-react'
import { cn } from '@/lib/utils'

const links = [
  { href: '/overview',    label: 'Overview',     icon: LayoutDashboard },
  { href: '/jobs',        label: 'Jobs',         icon: List },
  { href: '/keys',        label: 'API Keys',     icon: Key },
  { href: '/backends',    label: 'Backends',     icon: Server },
  { href: '/usage',       label: 'Usage',        icon: BarChart2 },
  { href: '/performance', label: 'Performance',  icon: Gauge },
  { href: '/api-test',    label: 'Test',         icon: FlaskConical },
]

function IQLogo({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="InferQ"
    >
      <defs>
        <linearGradient id="iq-bg" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">
          <stop offset="0%"   stopColor="#4f46e5" />
          <stop offset="100%" stopColor="#7c3aed" />
        </linearGradient>
        <filter id="iq-glow" x="-80%" y="-80%" width="260%" height="260%">
          <feGaussianBlur stdDeviation="1.4" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
      </defs>

      {/* background */}
      <rect width="32" height="32" rx="7" fill="url(#iq-bg)" />

      {/* letters pivoted at canvas centre (16,16), skewX(-12°) for italic lean */}
      <g transform="translate(16,16) skewX(-12) translate(-16,-16)">
        {/* i — intelligence spark */}
        <circle cx="7.5" cy="5.5" r="3.8" fill="white" opacity="0.22" />
        <circle cx="7.5" cy="5.5" r="2.3" fill="white" filter="url(#iq-glow)" />
        <rect x="5.75" y="9.5" width="3.5" height="17.5" rx="1.75" fill="white" />

        {/* Q — queue with directional exit */}
        <circle cx="20.5" cy="16.5" r="7.5" stroke="white" strokeWidth="2.5" />
        <circle cx="20.5" cy="16.5" r="1.6" fill="white" opacity="0.55" />
        <line x1="25.5" y1="22.2" x2="28" y2="26"
              stroke="white" strokeWidth="2.5" strokeLinecap="round" />
        <polyline points="26.3,24.3 28,26 26.7,27.7"
                  stroke="white" strokeWidth="1.6"
                  strokeLinecap="round" strokeLinejoin="round" fill="none" />
      </g>
    </svg>
  )
}

export default function Nav() {
  const pathname = usePathname()

  return (
    <aside className="w-56 flex-shrink-0 bg-card border-r border-border flex flex-col">
      <div className="px-5 py-5 flex items-center gap-2.5 border-b border-border">
        <IQLogo className="h-7 w-7 flex-shrink-0" />
        <span className="text-lg font-semibold tracking-tight">InferQ</span>
      </div>

      <nav className="flex-1 py-4 px-3 space-y-1 overflow-y-auto">
        {links.map(({ href, label, icon: Icon }) => {
          const active = pathname.startsWith(href)
          return (
            <Link
              key={href}
              href={href}
              className={cn(
                'flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors',
                active
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
              )}
            >
              <Icon className="h-4 w-4 flex-shrink-0" />
              {label}
            </Link>
          )
        })}
      </nav>

      <div className="px-5 py-4 border-t border-border">
        <p className="text-xs text-muted-foreground">InferQ v0.1.0</p>
      </div>
    </aside>
  )
}

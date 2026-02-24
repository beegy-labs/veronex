'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { LayoutDashboard, List, Key, FlaskConical, Zap, Server, BarChart2, Gauge } from 'lucide-react'
import { cn } from '@/lib/utils'

const links = [
  { href: '/overview',    label: 'Overview',     icon: LayoutDashboard },
  { href: '/jobs',        label: 'Jobs',         icon: List },
  { href: '/keys',        label: 'API Keys',     icon: Key },
  { href: '/backends',   label: 'Backends',     icon: Server },
  { href: '/usage',       label: 'Usage',        icon: BarChart2 },
  { href: '/performance', label: 'Performance',  icon: Gauge },
  { href: '/api-test',    label: 'Test',         icon: FlaskConical },
]

export default function Nav() {
  const pathname = usePathname()

  return (
    <aside className="w-56 flex-shrink-0 bg-card border-r border-border flex flex-col">
      <div className="px-5 py-5 flex items-center gap-2 border-b border-border">
        <Zap className="h-5 w-5 text-primary" />
        <span className="text-lg font-semibold tracking-tight">inferq</span>
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
        <p className="text-xs text-muted-foreground">inferq v0.1.0</p>
      </div>
    </aside>
  )
}

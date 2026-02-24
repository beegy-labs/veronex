'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { LayoutDashboard, List, Key, FlaskConical, Zap, Server, BarChart2, Gauge } from 'lucide-react'
import { clsx } from 'clsx'

const links = [
  { href: '/overview',    label: 'Overview',    icon: LayoutDashboard },
  { href: '/jobs',        label: 'Jobs',        icon: List },
  { href: '/keys',        label: 'API Keys',    icon: Key },
  { href: '/backends',    label: 'Backends',    icon: Server },
  { href: '/usage',       label: 'Usage',       icon: BarChart2 },
  { href: '/performance', label: 'Performance', icon: Gauge },
  { href: '/api-test',    label: 'Test',        icon: FlaskConical },
]

export default function Nav() {
  const pathname = usePathname()

  return (
    <aside className="w-56 flex-shrink-0 bg-slate-900 border-r border-slate-800 flex flex-col">
      {/* Logo / Brand */}
      <div className="px-5 py-5 flex items-center gap-2 border-b border-slate-800">
        <Zap className="h-5 w-5 text-indigo-400" />
        <span className="text-lg font-semibold text-slate-100 tracking-tight">inferq</span>
      </div>

      {/* Nav links */}
      <nav className="flex-1 py-4 px-3 space-y-1 overflow-y-auto">
        {links.map(({ href, label, icon: Icon }) => {
          const active = pathname.startsWith(href)
          return (
            <Link
              key={href}
              href={href}
              className={clsx(
                'flex items-center gap-3 px-3 py-2 rounded-md text-sm font-medium transition-colors',
                active
                  ? 'bg-indigo-600 text-white'
                  : 'text-slate-400 hover:bg-slate-800 hover:text-slate-100',
              )}
            >
              <Icon className="h-4 w-4 flex-shrink-0" />
              {label}
            </Link>
          )
        })}
      </nav>

      {/* Footer */}
      <div className="px-5 py-4 border-t border-slate-800">
        <p className="text-xs text-slate-500">inferq v0.1.0</p>
      </div>
    </aside>
  )
}

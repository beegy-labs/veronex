import { NextRequest, NextResponse } from 'next/server'

const PUBLIC_PATHS = ['/login', '/setup']
const SESSION_COOKIE = 'veronex_session'
const API_BASE = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? 'http://localhost:3001'

export async function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl
  const hasSession = request.cookies.has(SESSION_COOKIE)
  const isPublic = PUBLIC_PATHS.includes(pathname)

  // No session on a protected page — check setup status before redirecting
  if (!hasSession && !isPublic) {
    try {
      const res = await fetch(`${API_BASE}/v1/setup/status`, {
        signal: AbortSignal.timeout(2000),
      })
      if (res.ok) {
        const { needs_setup } = await res.json() as { needs_setup: boolean }
        if (needs_setup) {
          return NextResponse.redirect(new URL('/setup', request.url))
        }
      }
    } catch {
      // API unreachable — fall back to login
    }
    return NextResponse.redirect(new URL('/login', request.url))
  }

  // Has session but visiting /login -> redirect to home
  if (hasSession && pathname === '/login') {
    return NextResponse.redirect(new URL('/', request.url))
  }

  return NextResponse.next()
}

export const config = {
  matcher: [
    /*
     * Match all paths except:
     *   - _next/static, _next/image (Next.js internals)
     *   - favicon, static assets
     *   - api routes
     */
    '/((?!_next/static|_next/image|favicon.*|api/).*)',
  ],
}

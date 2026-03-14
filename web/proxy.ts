import { NextRequest, NextResponse } from 'next/server'

const PUBLIC_PATHS = ['/login', '/setup']
const SESSION_COOKIE = 'veronex_session'
const SETUP_DONE_COOKIE = 'veronex_setup_done'

async function checkNeedsSetup(): Promise<boolean> {
  try {
    const apiUrl = process.env.NEXT_PUBLIC_VERONEX_API_URL ?? ''
    const res = await fetch(`${apiUrl}/v1/setup/status`, {
      signal: AbortSignal.timeout(2000),
    })
    if (!res.ok) return false
    const { needs_setup } = await res.json()
    return needs_setup === true
  } catch {
    return false
  }
}

export async function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl
  const hasSession = request.cookies.has(SESSION_COOKIE)

  // Authenticated: block /login and /setup
  if (hasSession) {
    if (pathname === '/login' || pathname === '/setup') {
      return NextResponse.redirect(new URL('/', request.url))
    }
    return NextResponse.next()
  }

  // Unauthenticated: check if first-run setup is still needed.
  // Skip the API call if we already confirmed setup is done (SETUP_DONE_COOKIE),
  // or if the user is already on /setup.
  if (!request.cookies.has(SETUP_DONE_COOKIE) && pathname !== '/setup') {
    if (await checkNeedsSetup()) {
      return NextResponse.redirect(new URL('/setup', request.url))
    }
    // Setup is done — cache so we don't call the API on every request
    const res = PUBLIC_PATHS.includes(pathname)
      ? NextResponse.next()
      : NextResponse.redirect(new URL('/login', request.url))
    res.cookies.set(SETUP_DONE_COOKIE, '1', { maxAge: 3600, sameSite: 'lax', path: '/' })
    return res
  }

  // Normal flow
  if (PUBLIC_PATHS.includes(pathname)) {
    return NextResponse.next()
  }
  return NextResponse.redirect(new URL('/login', request.url))
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

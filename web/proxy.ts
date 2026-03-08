import { NextRequest, NextResponse } from 'next/server'

const PUBLIC_PATHS = ['/login', '/setup']
const SESSION_COOKIE = 'veronex_session'

export function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl
  const hasSession = request.cookies.has(SESSION_COOKIE)
  const isPublic = PUBLIC_PATHS.includes(pathname)

  // No session indicator on a protected page -> redirect to login
  if (!hasSession && !isPublic) {
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

'use client'

// NOTE: CSS import must live here (client-only component) to avoid SSR issues.
import 'swagger-ui-react/swagger-ui.css'
import SwaggerUI from 'swagger-ui-react'

interface SwaggerUiWrapperProps {
  specUrl: string
}

/**
 * Thin wrapper around swagger-ui-react.
 * Loaded via dynamic() with ssr:false from the /api-docs/swagger page.
 * Theme overrides are applied via inline <style> to match Veronex brand.
 */
export default function SwaggerUiWrapper({ specUrl }: SwaggerUiWrapperProps) {
  return (
    <>
      <style>{`
        /* Veronex brand overrides for Swagger UI */
        .swagger-ui .topbar { background-color: var(--theme-bg-card); border-bottom: 1px solid var(--theme-border); }
        .swagger-ui .topbar .download-url-wrapper .select-label select { border: 1px solid var(--theme-border); }
        .swagger-ui .topbar-wrapper img { display: none; }
        .swagger-ui .info h1, .swagger-ui .info h2, .swagger-ui .info h3 { color: var(--theme-text-primary); }
        .swagger-ui .opblock-tag { color: var(--theme-text-primary); border-bottom-color: var(--theme-border); }
        .swagger-ui section.models { border-color: var(--theme-border); }
        .swagger-ui .opblock.opblock-get .opblock-summary { border-color: var(--theme-status-info); }
        .swagger-ui .opblock.opblock-post .opblock-summary { border-color: var(--theme-status-success); }
        .swagger-ui .opblock.opblock-delete .opblock-summary { border-color: var(--theme-status-error); }
        .swagger-ui .opblock.opblock-patch .opblock-summary { border-color: var(--theme-status-warning); }
        .swagger-ui .btn.authorize { background-color: var(--theme-primary); border-color: var(--theme-primary); }
        .swagger-ui .btn.execute { background-color: var(--theme-primary); border-color: var(--theme-primary); }
      `}</style>
      <SwaggerUI
        url={specUrl}
        deepLinking
        tryItOutEnabled
        persistAuthorization
        displayRequestDuration
      />
    </>
  )
}

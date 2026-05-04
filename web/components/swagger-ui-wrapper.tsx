'use client'

// NOTE: CSS imports must live here (client-only component) to avoid SSR issues.
import 'swagger-ui-react/swagger-ui.css'
import '@/app/swagger-overrides.css'
import SwaggerUI from 'swagger-ui-react'

interface SwaggerUiWrapperProps {
  specUrl: string
}

export default function SwaggerUiWrapper({ specUrl }: SwaggerUiWrapperProps) {
  return (
    <SwaggerUI
      url={specUrl}
      deepLinking
      tryItOutEnabled
      persistAuthorization
      displayRequestDuration
    />
  )
}

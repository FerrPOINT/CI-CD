/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_AUTH_CLIENT_ID?: string
  readonly VITE_AUTH_ISSUER?: string
  readonly VITE_API_BASE_URL?: string
  readonly VITE_PLATFORM_BRANDING_URL?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

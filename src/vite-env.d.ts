/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_CID_CORE_PORT?: string;
  readonly VITE_CID_CORE_HOST?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

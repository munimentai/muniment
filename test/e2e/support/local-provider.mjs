// The macOS runner forwards this endpoint through the loopback and names the
// address it listens on. Every other platform reaches the model host directly:
// MUNIMENT_E2E_OLLAMA_HOST names that host, and the loopback is the default.
const OLLAMA_HOST = process.env.MUNIMENT_E2E_OLLAMA_HOST || '127.0.0.1'
export const OLLAMA_BASE_URL = process.env.MUNIMENT_E2E_OLLAMA_BASE_URL || `http://${OLLAMA_HOST}:52000/v1`

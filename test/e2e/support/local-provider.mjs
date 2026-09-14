// The macOS runner forwards this endpoint through the loopback and names the
// address it listens on. Every other platform reaches the model host directly.
export const OLLAMA_BASE_URL = process.env.MUNIMENT_E2E_OLLAMA_BASE_URL || 'http://10.1.10.105:52000/v1'

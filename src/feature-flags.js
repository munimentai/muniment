// Build-time UI availability. These flags do not grant backend permissions.
// Only the literal value "true" enables a feature in a development or release build.
export function readFeatureFlags(env = {}) {
  return Object.freeze({
    cloud: env.VITE_MUNIMENT_CLOUD === 'true',
    companyRecord: env.VITE_MUNIMENT_COMPANY_RECORD === 'true',
  })
}

export const featureFlags = readFeatureFlags(import.meta.env)

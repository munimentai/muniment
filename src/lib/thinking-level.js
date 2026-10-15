// The composer's thinking level: the levels the chip's model thinks at, from
// the inventory, the one a send carries, and their labels.
const LABELS = {
  off: 'Off',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra high',
  max: 'Max',
}

export function thinkingLabel(level) {
  return LABELS[level] ?? level
}

// The chip model's levels and the level a send carries, or null for a model
// without reasoning.
export function modelThinking(inventory, current) {
  if (!inventory || !current) return null
  const model = inventory.providers
    ?.find((provider) => provider.id === current.provider)
    ?.models?.find((entry) => entry.id === current.model)
  const levels = model?.thinking_levels ?? []
  if (!levels.length) return null
  const level = levels.includes(model.thinking_default) ? model.thinking_default : levels[0]
  return { levels, level }
}

// The inventory with `level` as the model's default, so the chip shows a
// choice before the runtime confirms it.
export function withThinkingLevel(inventory, provider, model, level) {
  if (!inventory?.providers) return inventory
  return {
    ...inventory,
    providers: inventory.providers.map((entry) => entry.id !== provider ? entry : {
      ...entry,
      models: entry.models.map((row) => row.id === model ? { ...row, thinking_default: level } : row),
    }),
  }
}

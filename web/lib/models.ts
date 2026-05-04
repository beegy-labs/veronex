/**
 * Model utility helpers shared across model selector components and test panel.
 */

/**
 * Returns true if the model is enabled (undefined = enabled by default;
 * false only when explicitly disabled on all carrying providers).
 */
export function isModelEnabled(m: { is_enabled?: boolean }): boolean {
  return m.is_enabled !== false
}
